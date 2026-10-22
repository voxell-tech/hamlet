//! In-memory authority and provider leases for credential refresh.

use std::collections::{HashMap, VecDeque};

use anyhow::{Result, bail, ensure};
use hamlet_protocol::runtime::{
    self, CredentialRequest, CredentialRequestAction as Action,
    CredentialResult,
};

use crate::credentials::{Credentials, pack, unpack};

#[derive(Default)]
pub(super) struct Authority {
    scopes: HashMap<(usize, String), (String, bool)>,
    /// The lease held on each provider's credential, by provider.
    held: HashMap<String, Lease>,
    waiting: VecDeque<(usize, CredentialRequest)>,
}

/// A runtime's hold on a provider's credential while it rotates it.
struct Lease {
    runtime: usize,
    scope: String,
    /// The lease's id, which the runtime commits or releases by.
    id: String,
    /// The credential's revision when the lease was taken; a commit
    /// after it changed is refused.
    revision: u64,
}

impl Authority {
    pub fn grant(
        &mut self,
        runtime: usize,
        scope: &str,
        provider: &str,
    ) {
        self.scopes
            .insert((runtime, scope.into()), (provider.into(), true));
    }

    pub fn owns(&self, runtime: usize, scope: &str) -> bool {
        self.scopes
            .get(&(runtime, scope.into()))
            .is_some_and(|(_, open)| *open)
    }

    pub fn finish(&mut self, runtime: usize, scope: &str) {
        if let Some((_, open)) =
            self.scopes.get_mut(&(runtime, scope.into()))
        {
            *open = false;
        }
    }

    pub fn gone(&mut self, runtime: usize) {
        self.scopes.retain(|(at, _), _| *at != runtime);
        self.held.retain(|_, lease| lease.runtime != runtime);
        self.waiting.retain(|(at, _)| *at != runtime);
    }

    pub fn request(
        &mut self,
        runtime: usize,
        request: CredentialRequest,
    ) {
        self.waiting.push_back((runtime, request));
    }

    pub fn flush(
        &mut self,
        credentials: &mut Credentials,
        serving: &str,
    ) -> Vec<(usize, CredentialResult)> {
        let mut answers = Vec::new();
        // A commit behind a waiting acquire must release its lease
        // first.
        loop {
            let mut progress = false;
            for _ in 0..self.waiting.len() {
                let (at, request) =
                    self.waiting.pop_front().expect("queued request");
                let mut answer = CredentialResult {
                    v: runtime::VERSION,
                    id: String::new(),
                    re: request.id.clone(),
                    credential: None,
                    lease: None,
                    error: None,
                };
                match self.apply(
                    credentials,
                    serving,
                    at,
                    &request,
                    &mut answer,
                ) {
                    Ok(false) => {
                        self.waiting.push_back((at, request))
                    }
                    result => {
                        if let Err(error) = result {
                            answer.error = Some(error.to_string());
                        }
                        answers.push((at, answer));
                        progress = true;
                    }
                }
            }
            if !progress {
                break;
            }
        }
        self.scopes.retain(|(at, scope), (_, open)| {
            *open
                || self.held.values().any(|lease| {
                    lease.runtime == *at && lease.scope == *scope
                })
        });
        answers
    }

    fn apply(
        &mut self,
        credentials: &mut Credentials,
        serving: &str,
        at: usize,
        request: &CredentialRequest,
        answer: &mut CredentialResult,
    ) -> Result<bool> {
        if request.action == Action::End {
            self.finish(at, &request.scope);
            return Ok(true);
        }
        let Some((provider, open)) =
            self.scopes.get(&(at, request.scope.clone()))
        else {
            bail!("No credential authority for this request.");
        };
        let provider = provider.clone();
        if matches!(request.action, Action::Read | Action::Acquire)
            && !open
        {
            bail!("This credential scope has ended.");
        }
        match request.action {
            Action::End => unreachable!(),
            Action::Read => {}
            Action::Acquire => {
                if self.held.contains_key(&provider) {
                    return Ok(false);
                }
                let lease = Lease {
                    runtime: at,
                    scope: request.scope.clone(),
                    id: uuid::Uuid::new_v4().to_string(),
                    revision: credentials.revision(&provider),
                };
                answer.lease = Some(lease.id.clone());
                self.held.insert(provider.clone(), lease);
            }
            Action::Commit | Action::Release => {
                if !self.held.get(&provider).is_some_and(|lease| {
                    lease.runtime == at
                        && lease.scope == request.scope
                        && Some(&lease.id) == request.lease.as_ref()
                }) {
                    bail!("This request holds no credential lease.");
                }
                // A started refresh persists even if its caller
                // stopped meanwhile.
                let revision = self
                    .held
                    .remove(&provider)
                    .expect("held lease")
                    .revision;
                let saved = if request.action == Action::Commit {
                    request
                        .credential
                        .clone()
                        .map(|value| {
                            let value = unpack(value)?;
                            credentials.remember_model(&value);
                            // Logout invalidates a rotation,
                            // including a logout followed by a new
                            // login.
                            ensure!(
                                credentials.revision(&provider)
                                    == revision,
                                "The credential changed while this \
                                 request was running."
                            );
                            credentials
                                .save_model(&provider, Some(value))
                        })
                        .transpose()
                } else {
                    Ok(None)
                };
                saved?;
            }
        }
        let serving = if request.action == Action::Read {
            serving
        } else {
            ""
        };
        answer.credential = credentials
            .model(&provider, serving)
            .map(pack)
            .transpose()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;
    use serde_json::{Value, json};

    use super::*;

    fn request(
        scope: &str,
        id: &str,
        action: &str,
        lease: Option<&str>,
        credential: Option<Value>,
    ) -> CredentialRequest {
        let mut request =
            json!({"v":1,"id":id,"scope":scope,"action":action});
        if let Some(lease) = lease {
            request["lease"] = lease.into();
        }
        if let Some(credential) = credential {
            request["credential"] = credential;
        }
        serde_json::from_value(request).unwrap()
    }

    #[test]
    fn leases_serialize_latest_reads_refuse_foreign_owners_and_finish_started_rotations()
     {
        let dir = Scratch::new("credential-authority");
        let mut credentials = Credentials::open(&dir).unwrap();
        credentials.save_model("chatgpt", Some(json!({"type":"oauth","access":"old-access","refresh":"old-refresh","expires":0,"clientId":"installation","scopes":["profile"]}))).unwrap();
        let mut authority = Authority::default();
        authority.grant(0, "first", "chatgpt");
        authority.grant(1, "second", "chatgpt");
        authority.grant(1, "cancelled", "chatgpt");
        authority.request(
            0,
            request("first", "acquire-1", "acquire", None, None),
        );
        let first =
            authority.flush(&mut credentials, "").pop().unwrap().1;
        let lease = first.lease.unwrap();
        authority.request(
            1,
            request("second", "acquire-2", "acquire", None, None),
        );
        authority.request(
            1,
            request(
                "cancelled",
                "acquire-cancelled",
                "acquire",
                None,
                None,
            ),
        );
        assert!(authority.flush(&mut credentials, "").is_empty());
        authority.request(
            1,
            request("first", "stolen-scope", "read", None, None),
        );
        authority.request(
            1,
            request(
                "second",
                "stolen-lease",
                "commit",
                Some(&lease),
                None,
            ),
        );
        let refused = authority.flush(&mut credentials, "");
        assert_eq!(refused.len(), 2);
        assert!(
            refused.iter().all(|(_, answer)| answer.error.is_some())
        );
        authority.finish(0, "first");
        authority.finish(1, "cancelled");
        let rotated = json!({"type":"oauth","access":"new-access","refresh":"new-refresh","expires":99.0,"extra":{"clientId":"installation","scopes":["profile"]}});
        authority.request(
            0,
            request(
                "first",
                "commit",
                "commit",
                Some(&lease),
                Some(rotated.clone()),
            ),
        );
        let answers = authority.flush(&mut credentials, "");
        assert!(
            answers
                .iter()
                .find(|(_, a)| a.re == "acquire-cancelled")
                .unwrap()
                .1
                .error
                .is_some()
        );
        assert!(
            answers
                .iter()
                .find(|(_, a)| a.re == "commit")
                .unwrap()
                .1
                .error
                .is_none()
        );
        let second = &answers
            .iter()
            .find(|(_, a)| a.re == "acquire-2")
            .unwrap()
            .1;
        assert_eq!(
            serde_json::to_value(&second.credential).unwrap(),
            rotated
        );
        let saved = Credentials::open(&dir)
            .unwrap()
            .model("chatgpt", "")
            .unwrap();
        assert_eq!(saved["access"], "new-access");
        assert_eq!(saved["clientId"], "installation");
        assert_eq!(saved["scopes"], json!(["profile"]));
    }

    #[test]
    fn logout_cannot_be_undone_by_a_refresh_and_failed_persistence_never_reports_success()
     {
        let dir = Scratch::new("credential-invalidated");
        let mut credentials = Credentials::open(&dir).unwrap();
        let original = json!({"type":"oauth","access":"original-access","refresh":"original-refresh","expires":0.0});
        credentials
            .save_model("openrouter", Some(original.clone()))
            .unwrap();
        let mut authority = Authority::default();
        authority.grant(0, "refresh", "openrouter");
        authority.request(
            0,
            request("refresh", "start", "acquire", None, None),
        );
        let lease = authority
            .flush(&mut credentials, "")
            .pop()
            .unwrap()
            .1
            .lease
            .unwrap();
        credentials.save_model("openrouter", None).unwrap();
        credentials
            .save_model("openrouter", Some(original.clone()))
            .unwrap();
        authority.request(0, request("refresh", "stale", "commit", Some(&lease), Some(json!({"type":"oauth","access":"late-access","refresh":"late-refresh","expires":99.0,"extra":{}}))));
        let refused =
            authority.flush(&mut credentials, "").pop().unwrap().1;
        assert!(refused.error.unwrap().contains("changed"));
        assert_eq!(
            credentials.model("openrouter", ""),
            Some(original.clone())
        );
        let mut echoed = json!(
            "late-access late-refresh original-access \
             original-refresh"
        );
        credentials.scrub(&mut echoed);
        assert_eq!(
            echoed,
            "[REDACTED] [REDACTED] [REDACTED] [REDACTED]"
        );
        authority.request(
            0,
            request("refresh", "again", "acquire", None, None),
        );
        let lease = authority
            .flush(&mut credentials, "")
            .pop()
            .unwrap()
            .1
            .lease
            .unwrap();
        // A directory at the destination makes atomic replacement
        // fail even as root.
        std::fs::remove_file(dir.join("credentials.json")).unwrap();
        std::fs::create_dir(dir.join("credentials.json")).unwrap();
        authority.request(
            0,
            request(
                "refresh",
                "failure",
                "commit",
                Some(&lease),
                Some(json!({"type":"api_key","key":"unsaved-key"})),
            ),
        );
        assert!(
            authority
                .flush(&mut credentials, "")
                .pop()
                .unwrap()
                .1
                .error
                .is_some()
        );
        assert_eq!(
            credentials.model("openrouter", ""),
            Some(original)
        );
        authority.grant(1, "next", "openrouter");
        authority.request(
            1,
            request("next", "next", "acquire", None, None),
        );
        assert!(
            authority
                .flush(&mut credentials, "")
                .pop()
                .unwrap()
                .1
                .lease
                .is_some()
        );
    }
}

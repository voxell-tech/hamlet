//! The gate's decision of a tool call: its identity checked, then the
//! decision kept for it, else one made now and kept, raising a wait
//! when it asks a chief.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use hamlet_protocol::{HOME, MAYOR};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::records::{self, Place};
use super::{
    IDENTITY_REFUSAL, NOBODY, OBJECTING, UNREADABLE, WAITING,
};
use crate::gate::{self, Asked, Standing, TrustKey, Verdict};
use crate::harness::{Binding, BindingId};
use crate::oversight::held::{self, CallState};
use crate::oversight::{
    ChatPlace, Danger, Holding, Raising, WaitKind,
};
use crate::record::{AgentRef, Hamlet};
use crate::service::Service;
use crate::{OperationId, operation, tools};

/// What the gate decided of a call.
#[derive(Debug)]
pub(super) enum Decision {
    Run {
        net: bool,
    },
    Block(String),
    /// It waits, in these words.
    Wait(&'static str),
}

/// The gate's answer to a call.
pub(super) enum Gate {
    Decided(Decision),
    /// The decision waits for the file a remote write replaces.
    Preview(Reading),
}

/// A remote file to read before a write's decision.
pub(crate) struct Reading {
    pub remote: crate::workspace::Remote,
    pub path: String,
    /// The work document it was read under.
    pub signature: Value,
}

/// A call whose identity is reserved for its request.
pub(super) struct Admitted<'a> {
    pub bound: Binding,
    pub id: OperationId,
    pub input: Option<&'a str>,
    pub tool: &'a str,
    pub arguments: &'a BTreeMap<String, Value>,
    /// Whether a hand-added tool is called, and with the network.
    pub hand: Option<bool>,
}

/// Why a call was not admitted.
pub(super) enum Unadmitted {
    /// Its conversation is gone.
    Unbound,
    /// Its identity was reserved for another request, or for none.
    Changed,
}

/// The call `key` of `binding`, once its identity is reserved for
/// this request. A legacy call or orphaned marker has no verifiable
/// request.
pub(super) fn admit<'a>(
    service: &mut Service,
    binding: BindingId,
    key: &str,
    input: Option<&'a str>,
    tool: &'a str,
    arguments: &'a BTreeMap<String, Value>,
) -> Result<std::result::Result<Admitted<'a>, Unadmitted>> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(Err(Unadmitted::Unbound));
    };
    let id = OperationId {
        runtime: bound.runtime.clone(),
        key: key.into(),
    };
    let mut canonical = String::new();
    operation::canonical(
        &json!([input, tool, arguments]),
        &mut canonical,
    );
    let digest = Sha256::digest(canonical);
    if !held::reserve(
        service.home.store().connection(),
        &id,
        binding,
        &digest,
    )? {
        return Ok(Err(Unadmitted::Changed));
    }
    let hand = crate::hand::load(service.home.layout().root())
        .0
        .into_iter()
        .find(|manifest| manifest.spec.name == tool)
        .map(|manifest| manifest.net());
    Ok(Ok(Admitted {
        bound,
        id,
        input,
        tool,
        arguments,
        hand,
    }))
}

/// Whether the call `key` of `binding` may run, before it is kept as
/// begun.
pub(super) fn allow(
    service: &mut Service,
    binding: BindingId,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<Gate> {
    let call =
        match admit(service, binding, key, input, tool, arguments)? {
            Ok(call) => call,
            Err(Unadmitted::Unbound) => {
                return Ok(Gate::Decided(Decision::Block(
                    NOBODY.into(),
                )));
            }
            Err(Unadmitted::Changed) => {
                return Ok(Gate::Decided(Decision::Block(
                    IDENTITY_REFUSAL.into(),
                )));
            }
        };
    if tool != "object" && super::objecting(service, binding, input)?
    {
        return Ok(Gate::Decided(Decision::Wait(OBJECTING)));
    }
    if crate::society::withheld(service, binding, tool)? {
        return Ok(Gate::Decided(Decision::Run { net: false }));
    }
    decide(service, &call, preview)
}

/// The gate's decision of `call`: the one kept, else one made now and
/// kept.
pub(super) fn decide(
    service: &mut Service,
    call: &Admitted<'_>,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<Gate> {
    if let Some(decision) = kept(service, &call.id)? {
        return Ok(Gate::Decided(decision));
    }
    if preview.is_none()
        && call.tool == "write"
        && !service.home.store().cancelling(call.bound.id)?
        && let Some(remote) =
            service.remote_tools(&call.bound.agent, false)?
        && let Some(path) =
            call.arguments.get("path").and_then(Value::as_str)
    {
        return Ok(Gate::Preview(Reading {
            remote,
            path: path.into(),
            signature: serde_json::to_value(
                service.work(&call.bound.agent.hamlet)?,
            )?,
        }));
    }
    fresh(service, call, preview).map(Gate::Decided)
}

/// The decision kept for the call `id` that stands until it runs: a
/// result it ran to, a refusal, a chief's ruling or its wait.
/// Automatic allowances depend on the current grants and session
/// trust, so they are made again.
pub(super) fn kept(
    service: &mut Service,
    id: &OperationId,
) -> Result<Option<Decision>> {
    let db = service.home.store().connection();
    let Some(call) = held::decision(db, id)? else {
        return Ok(None);
    };
    if operation::is_marked(db, id.runtime.as_str(), &id.key)? {
        let net = match call.state {
            CallState::Allowed { net }
            | CallState::Ruled { net, .. } => net,
            _ => false,
        };
        return Ok(Some(Decision::Run { net }));
    }
    Ok(match call.state {
        CallState::Blocked(words) => Some(Decision::Block(words)),
        CallState::Ruled { ruling, net, .. } if ruling.allows() => {
            Some(Decision::Run { net })
        }
        CallState::Ruled {
            ruling: crate::oversight::Ruling::Deny,
            text,
            ..
        } => Some(Decision::Block(format!(
            "the chief refused it: {}",
            if text.is_empty() {
                "no reason given"
            } else {
                &text
            }
        ))),
        CallState::Ruled { .. } => {
            Some(Decision::Block(NOBODY.into()))
        }
        CallState::Held { .. } => Some(Decision::Wait(WAITING)),
        CallState::Allowed { .. } | CallState::Open => None,
    })
}

/// The decision of `call` made now, kept, and asked of a chief when
/// the gate asks.
fn fresh(
    service: &mut Service,
    call: &Admitted<'_>,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<Decision> {
    let (id, binding, input) = (&call.id, call.bound.id, call.input);
    let db = service.home.store().connection();
    let Some(conversation) = records::kept(db, binding)? else {
        held::allow(db, id, binding, input, false, None)?;
        return Ok(Decision::Run { net: false });
    };
    if service.home.store().cancelling(binding)? {
        let words = crate::harness::answers::STOPPED_WORDS;
        held::block(
            service.home.store().connection(),
            id,
            binding,
            input,
            words,
            None,
        )?;
        return Ok(Decision::Block(words.into()));
    }
    let place = conversation.place;
    let reach = match reach(service, call, &place, preview)? {
        Ok(reach) => reach,
        Err(words) => return Ok(Decision::Block(words)),
    };
    let Some(record) =
        service.home.store().record(&place.agent.hamlet)?
    else {
        return Ok(Decision::Block(NOBODY.into()));
    };
    let by = wait_by(&record, &place.agent);
    let standing = Standing {
        house_edit: reach.house_edit,
        ..standing(service, &place, &record)?
    };
    let about = service
        .credentials
        .scrub_text(&gate::call_text(&reach.call));
    let db = service.home.store().connection();
    let asked = Asked::of(db, binding, input)?;
    let verdict = gate::rule(
        &reach.call,
        &standing,
        &asked,
        &by,
        &about,
        &record.limits.chief,
        record.mayor == place.agent.number,
    );
    let change = reach.change;
    let danger = match verdict {
        Verdict::Run { net } => {
            held::allow(
                db,
                id,
                binding,
                input,
                net,
                change.as_ref(),
            )?;
            return Ok(Decision::Run { net });
        }
        Verdict::Block(words) => {
            asked.keep(db, binding)?;
            held::block(
                db,
                id,
                binding,
                input,
                &words,
                change.as_ref(),
            )?;
            return Ok(Decision::Block(words));
        }
        Verdict::Ask(danger) => danger,
    };
    asked.keep(db, binding)?;
    if service.chats.closing
        && super::reflecting(service, binding, input)?
    {
        held::block(
            service.home.store().connection(),
            id,
            binding,
            input,
            NOBODY,
            change.as_ref(),
        )?;
        return Ok(Decision::Block(NOBODY.into()));
    }
    held::raise(
        service.home.store().connection(),
        Raising {
            kind: WaitKind::Approval(danger),
            by,
            agent: Some(place.agent.clone()),
            about,
            change: change.clone(),
            questions: None,
            answers: danger.answers(),
            chat: chat_of(&place),
            call: Some(Holding {
                id: id.clone(),
                binding,
                input: input.map(String::from),
                change,
            }),
        },
        super::super::now(),
    )?;
    Ok(Decision::Wait(WAITING))
}

/// A call classified by its reach, with the change an edit makes.
struct Reach {
    call: gate::Call,
    change: Option<Value>,
    /// The call edits a file in the agent's own house.
    house_edit: bool,
}

/// The reach of `call` made at `place`; the refusal when a remote
/// write's preview failed.
fn reach(
    service: &mut Service,
    call: &Admitted<'_>,
    place: &Place,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<std::result::Result<Reach, String>> {
    let args =
        Value::Object(call.arguments.clone().into_iter().collect());
    let mut classified = match call.hand {
        Some(net) => gate::Call::Hand {
            name: call.tool.into(),
            args,
            net,
        },
        None => gate::call_of(call.tool, &args),
    };
    let at = places(service, &place.agent)?;
    let remote = service.remote_tools(&place.agent, false)?.is_some();
    if let gate::Call::Edit {
        write: true,
        path,
        old,
        created,
        ..
    } = &mut classified
    {
        let previous = if remote {
            match preview {
                Some(Ok(before)) => before.clone(),
                Some(Err(words)) => return Ok(Err(words.clone())),
                None => {
                    return Ok(Err("The remote edit preview is \
                                   not available."
                        .into()));
                }
            }
        } else {
            tools::footing(path, &at)
                .ok()
                .and_then(|full| before(service, &at, &full))
        };
        if let Some(previous) = previous {
            *old = previous;
            *created = false;
        }
    }
    let gate::Call::Edit {
        write,
        path,
        old,
        now,
        created,
    } = &classified
    else {
        return Ok(Ok(Reach {
            call: classified,
            change: None,
            house_edit: false,
        }));
    };
    let mut change =
        serde_json::to_value(crate::change::change_between(
            path,
            old,
            now,
            *write && *created,
        ))?;
    service.credentials.scrub(&mut change);
    let house_edit = !remote
        && tools::footing(path, &at).is_ok_and(|full| {
            crate::sandbox::within(&full, &at.house)
        });
    Ok(Ok(Reach {
        call: classified,
        change: Some(change),
        house_edit,
    }))
}

/// The text of the file at `path` as a write's preview shows it, or
/// `None` when there is none. It is read as the agent at `at` may,
/// so the preview shows the chief nothing the agent could not read
/// itself.
fn before(
    service: &Service,
    at: &tools::Place,
    path: &std::path::Path,
) -> Option<String> {
    let path = crate::sandbox::path::text(path);
    let Ok(env) =
        tools::environment(&service.chats.sandbox, at, false)
    else {
        return Some(UNREADABLE.into());
    };
    match env.read_text(&path) {
        Ok(text) => Some(service.credentials.scrub_text(&text)),
        Err(error) if error.code == "not_found" => None,
        Err(error) => {
            let size = env
                .file_info(&path)
                .map(|info| format!(", {} bytes", info.size))
                .unwrap_or_default();
            let reason = if error.code == "invalid" {
                "is not UTF-8 text"
            } else {
                "could not be read as text"
            };
            Some(format!("[a file that {reason}{size}]"))
        }
    }
}

/// The grants, limits, mode and session trust in force for `place`.
pub(super) fn standing(
    service: &mut Service,
    place: &Place,
    record: &Hamlet,
) -> Result<Standing> {
    let hamlet = &place.agent.hamlet;
    let work = service.work(hamlet)?;
    let limits = &record.limits.chief;
    let allow_net = service.settings()?.allow_net;
    let db = service.home.store().connection();
    let grants = gate::grants(db)?;
    let by = record.address_of(place.agent.number);
    let chief = place.chief.clone().unwrap_or_default();
    let (edit, command) = gate::trusted(
        db,
        &TrustKey {
            agent: &place.agent,
            chief: &chief,
            session: place.session,
        },
    )?;
    let manual = work.asks();
    Ok(Standing {
        house_edit: false,
        net: allow_net
            || gate::holds(
                limits,
                &grants,
                by,
                &place.agent,
                Danger::Network.as_str(),
            ),
        asks_edits: manual && !edit,
        asks_commands: manual && !command,
    })
}

/// The name a wait shows for `agent`.
pub(super) fn wait_by(record: &Hamlet, agent: &AgentRef) -> String {
    let name = record.address_of(agent.number);
    if name == MAYOR && agent.hamlet != HOME {
        format!("mayor@{}", agent.hamlet)
    } else {
        name.into()
    }
}

pub(super) fn chat_of(place: &Place) -> Option<ChatPlace> {
    place.chief.as_ref().map(|chief| ChatPlace {
        hamlet: place.agent.hamlet.clone(),
        number: place.agent.number,
        chief: chief.clone(),
        session: place.session,
    })
}

/// Where the tools of `agent` work.
pub(super) fn places(
    service: &mut Service,
    agent: &AgentRef,
) -> Result<tools::Place> {
    let root = service.home.layout().root().to_owned();
    let work = service.work(&agent.hamlet)?;
    Ok(tools::Place {
        house: root
            .join("houses")
            .join(&agent.hamlet)
            .join(agent.number.to_string()),
        folder: work.folder.map(PathBuf::from),
        home: root.clone(),
        shared: vec![root.join("tools"), root.join("library")],
    })
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::RuntimeInstance;
    use crate::harness::Answer;
    use crate::harness::link::ModelRef;
    use crate::operation::{Marked, Request};

    fn decide(
        service: &mut Service,
        binding: BindingId,
        key: &str,
        input: Option<&str>,
        tool: &str,
        arguments: &BTreeMap<String, Value>,
    ) -> Result<Decision> {
        match allow(
            service, binding, key, input, tool, arguments, None,
        )? {
            Gate::Decided(decision) => Ok(decision),
            Gate::Preview(_) => {
                panic!("a local write read a preview")
            }
        }
    }

    /// A service on a scratch home, with the founder's conversation
    /// bound to a runtime.
    fn served(scratch: &Scratch) -> (Service, BindingId, String) {
        let home = crate::Home::open(&scratch.join("home")).unwrap();
        let mut service = Service::new(home, true).unwrap();
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        let store = service.home.store();
        let runtime = store.runtime_for("pi").unwrap();
        let model = ModelRef {
            provider: "faux".into(),
            id: "faux-1".into(),
            base_url: None,
        };
        let binding = store
            .add_binding(&agent, false, &model, &runtime)
            .unwrap();
        let place = Place {
            agent,
            chief: Some(service.chief.clone()),
            session: 1,
        };
        records::keep(store.connection(), binding, &place, None, 0)
            .unwrap();
        (service, binding, runtime.as_str().to_owned())
    }

    #[test]
    fn a_write_s_preview_shows_only_what_the_agent_may_read() {
        let scratch = Scratch::new("calls-preview");
        let (mut service, _, _) = served(&scratch);
        let root = service.home.layout().root().to_owned();
        std::fs::write(root.join("private.txt"), "the chief's own\n")
            .unwrap();
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        let at = places(&mut service, &agent).unwrap();
        std::fs::write(at.house.join("NOTES.md"), "# Notes\n")
            .unwrap();

        assert_eq!(
            before(&service, &at, &root.join("private.txt"))
                .as_deref(),
            Some(UNREADABLE)
        );
        assert_eq!(
            before(&service, &at, &at.house.join("NOTES.md"))
                .as_deref(),
            Some("# Notes\n")
        );
        assert_eq!(
            before(&service, &at, &at.house.join("new.md")),
            None
        );
    }

    #[test]
    fn a_gate_identity_cannot_authorize_a_changed_request() {
        let scratch = Scratch::new("calls-identity");
        let (mut service, binding, _) = served(&scratch);
        let arguments = BTreeMap::from([(
            "command".to_owned(),
            Value::String("echo harmless".into()),
        )]);
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        let changed = BTreeMap::from([(
            "command".to_owned(),
            Value::String("rm -rf important".into()),
        )]);
        assert!(
            matches!(
                decide(
                    &mut service,
                    binding,
                    "same-key",
                    None,
                    "bash",
                    &changed
                )
                .unwrap(),
                Decision::Block(_)
            ),
            "a different command reused an earlier allowance"
        );
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "write",
                &arguments
            )
            .unwrap(),
            Decision::Block(_)
        ));
        let bound =
            service.home.store().binding(binding).unwrap().unwrap();
        let other = service
            .home
            .store()
            .add_binding(
                &bound.agent,
                false,
                &bound.model,
                &bound.runtime,
            )
            .unwrap();
        assert!(matches!(
            decide(
                &mut service,
                other,
                "same-key",
                None,
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Block(_)
        ));
        service.home.close().unwrap();
        let mut service = Service::new(
            crate::Home::open(&scratch.join("home")).unwrap(),
            true,
        )
        .unwrap();
        let marker = OperationId {
            runtime: bound.runtime.clone(),
            key: "same-key".into(),
        };
        let request = Request::tool("bash", &arguments).unwrap();
        service
            .home
            .store()
            .once(&marker, &request, |_| {
                Ok(json!({"text": "done", "error": false}))
            })
            .unwrap();
        service.home.store().begin_cancel(binding).unwrap();
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                Some("different-input"),
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Block(_)
        ));
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "bash",
                &changed
            )
            .unwrap(),
            Decision::Block(_)
        ));
    }

    #[test]
    fn a_waiting_identity_asked_with_other_arguments_is_refused() {
        let scratch = Scratch::new("calls-waiting");
        let (mut service, binding, _) = served(&scratch);
        let mut limits = service.limits_of("home").unwrap();
        limits.mayor.build_roads = false;
        crate::limits::set(
            service.home.store().connection(),
            "home",
            limits,
        )
        .unwrap();
        let road = |b: &str| {
            BTreeMap::from([
                ("a".to_owned(), json!("founder")),
                ("b".to_owned(), json!(b)),
                ("reason".to_owned(), json!("trade")),
            ])
        };
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "k",
                None,
                "build_road",
                &road("ana")
            )
            .unwrap(),
            Decision::Wait(_)
        ));
        assert!(matches!(
            decide(&mut service, binding, "k", None, "build_road", &road("bo"))
                .unwrap(),
            Decision::Block(words) if words == IDENTITY_REFUSAL
        ));
    }

    #[test]
    fn a_grant_taken_back_before_execution_does_not_authorize_the_call()
     {
        let scratch = Scratch::new("calls-revoked");
        let (mut service, binding, _) = served(&scratch);
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        gate::give(
            service.home.store().connection(),
            &agent,
            "mayor",
            "network",
            "chief",
            0,
        )
        .unwrap();
        let args = BTreeMap::from([
            ("command".to_owned(), json!("echo harmless")),
            ("network".to_owned(), json!(true)),
        ]);
        assert!(matches!(
            decide(&mut service, binding, "k", None, "bash", &args)
                .unwrap(),
            Decision::Run { net: true }
        ));
        gate::revoke(
            service.home.store().connection(),
            &agent,
            "network",
            "chief",
            1,
        )
        .unwrap();
        assert!(
            matches!(
                decide(
                    &mut service,
                    binding,
                    "k",
                    None,
                    "bash",
                    &args
                )
                .unwrap(),
                Decision::Wait(_)
            ),
            "a revoked grant still allowed an unexecuted call"
        );
    }

    #[test]
    fn upgrading_old_gate_rows_never_invents_a_request_or_replays_an_effect()
     {
        let scratch = Scratch::new("calls-upgrade");
        let (mut service, binding, runtime) = served(&scratch);
        let args = BTreeMap::from([(
            "command".to_owned(),
            json!("echo harmless"),
        )]);
        for key in ["allowed", "finished"] {
            assert!(matches!(
                decide(
                    &mut service,
                    binding,
                    key,
                    None,
                    "bash",
                    &args
                )
                .unwrap(),
                Decision::Run { .. }
            ));
        }
        let finished = OperationId {
            runtime: RuntimeInstance::new(&runtime).unwrap(),
            key: "finished".into(),
        };
        let request = Request::tool("bash", &args).unwrap();
        service
            .home
            .store()
            .once(&finished, &request, |_| {
                Ok("kept outcome".to_owned())
            })
            .unwrap();
        let db = service.home.store().connection();
        // Reconstruct the version-8 schema on disposable data.
        db.execute_batch(
            "ALTER TABLE wait DROP COLUMN questions; ALTER TABLE \
             gate_call DROP COLUMN digest; CREATE TABLE \
             harness_wait (runtime TEXT NOT NULL, key TEXT NOT \
             NULL, PRIMARY KEY (runtime, key)) STRICT, WITHOUT \
             ROWID; PRAGMA user_version = 8;",
        )
        .unwrap();
        service.home.close().unwrap();
        let mut service = Service::new(
            crate::Home::open(&scratch.join("home")).unwrap(),
            true,
        )
        .unwrap();
        for key in ["allowed", "finished"] {
            assert!(matches!(
                decide(
                    &mut service,
                    binding,
                    key,
                    None,
                    "bash",
                    &args
                )
                .unwrap(),
                Decision::Block(_)
            ));
        }
        let db = service.home.store().connection();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            12
        );
        assert_eq!(
            operation::marked(db, &finished, &request).unwrap(),
            Some(Marked::Outcome("kept outcome".to_owned()))
        );
        assert!(matches!(
            decide(&mut service, binding, "new", None, "bash", &args)
                .unwrap(),
            Decision::Run { .. }
        ));
    }

    #[test]
    fn a_call_allowed_before_a_stop_runs_nothing_after_it() {
        let scratch = Scratch::new("calls-stopped");
        let (mut service, binding, runtime) = served(&scratch);
        let arguments = BTreeMap::from([(
            "command".to_owned(),
            Value::String("echo hi".into()),
        )]);
        let allowed = decide(
            &mut service,
            binding,
            "call-1",
            Some("input-1"),
            "bash",
            &arguments,
        )
        .unwrap();
        assert!(matches!(allowed, Decision::Run { .. }));

        let question =
            BTreeMap::from([("question".into(), json!("Continue?"))]);
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "question-1",
                Some("input-1"),
                "ask_chief",
                &question
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        service.home.store().begin_cancel(binding).unwrap();
        let refused = decide(
            &mut service,
            binding,
            "call-1",
            Some("input-1"),
            "bash",
            &arguments,
        )
        .unwrap();
        assert!(
            matches!(&refused, Decision::Block(words) if words == crate::harness::answers::STOPPED_WORDS),
            "a call allowed before the stop still runs after it"
        );
        // So does a call of the stopped turn first heard of after it.
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "unasked",
                Some("input-1"),
                "bash",
                &arguments,
            )
            .unwrap(),
            Decision::Block(_)
        ));

        // The refusal stands once the runtime has acknowledged the
        // cancel, as the result of a call that ran would, and the
        // next turn's calls run.
        service.home.store().cancelled(binding).unwrap();
        let call = OperationId {
            runtime: RuntimeInstance::new(&runtime).unwrap(),
            key: "call-1".into(),
        };
        let still = kept(&mut service, &call).unwrap();
        assert!(matches!(still, Some(Decision::Block(_))));
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "next",
                Some("input-2"),
                "bash",
                &arguments,
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        let Ok(asked) = admit(
            &mut service,
            binding,
            "question-1",
            Some("input-1"),
            "ask_chief",
            &question,
        )
        .unwrap() else {
            panic!("the question's identity changed");
        };
        assert!(
            matches!(
                super::super::ask::ask(&mut service, &asked).unwrap(),
                Answer::Refusal(_)
            ),
            "a stopped question bypassed the gate's refusal"
        );
    }
}

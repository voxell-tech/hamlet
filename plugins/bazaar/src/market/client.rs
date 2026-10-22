//! The bazaar's server as it is called: each route of
//! `hamlet-server/docs/api.md` the bazaar uses, as the fetch that
//! asks it, and what an answer comes to. A plugin's fetch sets no
//! headers and sends only GET and POST, so the session token goes in
//! the query and a POST names the method it stands for.

use hamlet_engine_bevy::api::Method;
use hamlet_engine_bevy::{Failure, FetchReply, FetchRequest, Reply};
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::market::lot::Draft;

/// The server's address where `HAMLET_BAZAAR_URL` does not say.
pub(crate) const DEFAULT_SERVER: &str = "http://localhost:8787";

/// The variable of the environment that names the server.
pub(crate) const SERVER_VARIABLE: &str = "HAMLET_BAZAAR_URL";

/// The most listings asked for at once.
pub(crate) const PAGE: u32 = 100;

/// The server named by `variable`, or the one by default.
pub(crate) fn server_of(variable: Option<String>) -> String {
    variable
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

/// The bazaar's server, called as the one signed in, if anyone is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Server {
    pub base: String,
    pub token: Option<String>,
}

/// Why the server did not do what it was asked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Refusal {
    /// The status it answered, `None` when it was not reached.
    pub status: Option<u16>,
    /// The server's code for it, such as `pending` or `conflict`;
    /// `unreachable` when it was not reached, `busy` when the engine
    /// held the fetch back for its budget.
    pub code: String,
    pub message: String,
}

impl Refusal {
    fn new(status: Option<u16>, code: &str, message: String) -> Self {
        Refusal {
            status,
            code: code.into(),
            message,
        }
    }

    /// The token was refused: the one signed in is not.
    pub fn signed_out(&self) -> bool {
        self.status == Some(401)
    }

    /// In words for the chief.
    pub fn words(&self) -> String {
        self.message.clone()
    }
}

impl Server {
    fn url(&self, path: &str, query: &[(&str, String)]) -> String {
        let mut pairs = query.to_vec();
        if let Some(token) = &self.token {
            pairs.push(("token", token.clone()));
        }
        let mut url = format!("{}{path}", self.base);
        for (at, (name, value)) in pairs.iter().enumerate() {
            url.push(if at == 0 { '?' } else { '&' });
            url.push_str(name);
            url.push('=');
            url.push_str(&escaped(value));
        }
        url
    }

    fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> FetchRequest {
        FetchRequest {
            method: Method::Get,
            url: self.url(path, query),
            content_type: None,
            body: Vec::new(),
        }
    }

    fn post(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> FetchRequest {
        let (content_type, body) = match body {
            Some(body) => (
                Some("application/json".to_string()),
                serde_json::to_vec(&body).unwrap_or_default(),
            ),
            None => (None, Vec::new()),
        };
        FetchRequest {
            method: Method::Post,
            url: self.url(path, query),
            content_type,
            body,
        }
    }

    /// A POST standing for `method`.
    fn as_method(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> FetchRequest {
        self.post(path, &[("_method", method.to_string())], body)
    }

    /// Listings on the shelves, of every kind, from `offset` on.
    pub fn listings(&self, offset: u32) -> FetchRequest {
        let query = [
            ("limit", PAGE.to_string()),
            ("offset", offset.to_string()),
        ];
        self.get("/v1/listings", &query)
    }

    pub fn detail(&self, id: &str) -> FetchRequest {
        self.get(&format!("/v1/listings/{}", escaped(id)), &[])
    }

    /// Begins a sign-in with GitHub.
    pub fn device(&self) -> FetchRequest {
        self.post("/v1/auth/device", &[], None)
    }

    /// Asks whether the sign-in of `code` was approved.
    pub fn poll(&self, code: &str) -> FetchRequest {
        let body = json!({ "deviceCode": code });
        self.post("/v1/auth/device/poll", &[], Some(body))
    }

    pub fn sign_out(&self) -> FetchRequest {
        self.as_method("DELETE", "/v1/auth/session", None)
    }

    pub fn me(&self) -> FetchRequest {
        self.get("/v1/me", &[])
    }

    /// Everything the one signed in owns.
    pub fn library(&self) -> FetchRequest {
        self.get("/v1/me/library", &[])
    }

    /// The listings of the one signed in, in every state.
    pub fn mine(&self) -> FetchRequest {
        self.get("/v1/me/listings", &[])
    }

    /// The page on Stripe where the one signed in sets up how they
    /// are paid.
    pub fn payouts(&self) -> FetchRequest {
        self.post("/v1/me/payouts", &[], None)
    }

    /// Buys a listing: owned at once when it is free, or a page on
    /// Stripe to pay on.
    pub fn purchase(&self, id: &str) -> FetchRequest {
        let path = format!("/v1/listings/{}/purchase", escaped(id));
        self.post(&path, &[], None)
    }

    pub fn purchase_state(&self, purchase: &str) -> FetchRequest {
        self.get(&format!("/v1/purchases/{}", escaped(purchase)), &[])
    }

    /// `limit` bytes of the package of `version` from `offset` on.
    pub fn package_part(
        &self,
        id: &str,
        version: &str,
        offset: u64,
        limit: u64,
    ) -> FetchRequest {
        let path = format!(
            "/v1/listings/{}/versions/{}/package",
            escaped(id),
            escaped(version)
        );
        let query = [
            ("offset", offset.to_string()),
            ("limit", limit.to_string()),
        ];
        self.get(&path, &query)
    }

    /// Puts up a listing, not yet on the shelves.
    pub fn create(&self, draft: &Draft) -> FetchRequest {
        self.post("/v1/listings", &[], Some(json!(draft)))
    }

    /// Changes a listing's words, price or tags.
    pub fn change(&self, id: &str, draft: &Draft) -> FetchRequest {
        let draft = Draft {
            kind: None,
            ..draft.clone()
        };
        let path = format!("/v1/listings/{}", escaped(id));
        self.as_method("PATCH", &path, Some(json!(draft)))
    }

    /// Sets a listing's cover, `bytes` of a picture of `kind`.
    pub fn cover(
        &self,
        id: &str,
        bytes: Vec<u8>,
        kind: &str,
    ) -> FetchRequest {
        let path = format!("/v1/listings/{}/cover", escaped(id));
        FetchRequest {
            content_type: Some(kind.to_string()),
            body: bytes,
            ..self.as_method("PUT", &path, None)
        }
    }

    /// Begins an upload of a package in parts.
    pub fn start_upload(&self, id: &str) -> FetchRequest {
        let path = format!("/v1/listings/{}/uploads", escaped(id));
        self.post(&path, &[], None)
    }

    /// Sends the part `index` of `upload`.
    pub fn upload_part(
        &self,
        id: &str,
        upload: &str,
        index: usize,
        bytes: Vec<u8>,
    ) -> FetchRequest {
        let path = format!(
            "/v1/listings/{}/uploads/{}/parts/{index}",
            escaped(id),
            escaped(upload)
        );
        FetchRequest {
            content_type: Some("application/octet-stream".into()),
            body: bytes,
            ..self.post(&path, &[], None)
        }
    }

    /// Makes `version` of a listing from the `parts` of `upload`.
    pub fn release(
        &self,
        id: &str,
        version: &str,
        changes: &str,
        upload: &str,
        parts: usize,
    ) -> FetchRequest {
        let path = format!("/v1/listings/{}/versions", escaped(id));
        let body = json!({
            "version": version,
            "changes": changes,
            "upload": upload,
            "parts": parts,
        });
        self.post(&path, &[], Some(body))
    }

    /// Puts a listing on the shelves.
    pub fn publish(&self, id: &str) -> FetchRequest {
        let path = format!("/v1/listings/{}/publish", escaped(id));
        self.post(&path, &[], None)
    }

    /// Takes a listing off the shelves. Its owners keep it.
    pub fn unlist(&self, id: &str) -> FetchRequest {
        let path = format!("/v1/listings/{}/unlist", escaped(id));
        self.post(&path, &[], None)
    }
}

/// `text` with all but the unreserved characters of a URL escaped.
pub(crate) fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The bytes of a successful answer, or why there are none.
pub(crate) fn bytes(
    answer: Result<Reply, Failure>,
) -> Result<Vec<u8>, Refusal> {
    let FetchReply { status, body, .. } = match answer {
        Ok(Reply::Fetched(fetched)) => fetched,
        Ok(_) => {
            return Err(Refusal::new(
                None,
                "internal",
                "The bazaar answered oddly.".into(),
            ));
        }
        Err(Failure::OutOfBudget(why)) => {
            return Err(Refusal::new(
                None,
                "busy",
                format!(
                    "Hamlet held the bazaar's fetch back: {why}."
                ),
            ));
        }
        Err(why) => {
            return Err(Refusal::new(
                None,
                "unreachable",
                format!("The bazaar could not be reached: {why}."),
            ));
        }
    };
    if (200..300).contains(&status) {
        return Ok(body);
    }
    #[derive(serde::Deserialize)]
    struct Fault {
        error: FaultBody,
    }
    #[derive(serde::Deserialize)]
    struct FaultBody {
        code: String,
        message: String,
    }
    Err(match serde_json::from_slice::<Fault>(&body) {
        Ok(Fault {
            error: FaultBody { code, message },
        }) => Refusal::new(Some(status), &code, sentence(&message)),
        Err(_) => Refusal::new(
            Some(status),
            "internal",
            format!("The bazaar answered {status}."),
        ),
    })
}

/// The answer read as `T`, or why it cannot be.
pub(crate) fn read<T: DeserializeOwned>(
    answer: Result<Reply, Failure>,
) -> Result<T, Refusal> {
    let body = bytes(answer)?;
    serde_json::from_slice(&body).map_err(|e| {
        Refusal::new(
            None,
            "internal",
            format!("The bazaar's answer could not be read: {e}."),
        )
    })
}

/// That a successful answer came, whatever it held.
pub(crate) fn done(
    answer: Result<Reply, Failure>,
) -> Result<(), Refusal> {
    bytes(answer).map(drop)
}

/// `message` begun with a capital and ended with a stop.
fn sentence(message: &str) -> String {
    let mut chars = message.trim().chars();
    let mut out = match chars.next() {
        Some(first) => {
            first.to_uppercase().chain(chars).collect::<String>()
        }
        None => return "The bazaar refused.".into(),
    };
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

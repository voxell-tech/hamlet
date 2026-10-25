//! The bazaar's server as it is called: each of its routes the
//! bazaar uses, as the request that
//! asks it, and what an answer comes to. The window sends the session
//! token as a bearer header and each route's own method.

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::bazaar::errand::Fetched;
use crate::bazaar::market::lot::Draft;

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

/// A request of the server, as it is sent.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Request {
    pub method: Method,
    pub url: String,
    /// The session token, sent as a bearer header.
    pub token: Option<String>,
    /// The body and its media type.
    pub body: Option<(String, Vec<u8>)>,
    /// It carries a package, which takes longer than the rest.
    pub long: bool,
}

/// Why the server did not do what it was asked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Refusal {
    /// The status it answered, `None` when it was not reached.
    pub status: Option<u16>,
    /// The server's code for it, such as `pending` or `conflict`;
    /// `unreachable` when it was not reached.
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

    /// It may go another way when asked again: the server was not
    /// reached, or failed on its side, as it does while it restarts.
    pub fn passing(&self) -> bool {
        match self.status {
            None => self.code == "unreachable",
            Some(status) => status >= 500,
        }
    }

    /// In words for the chief.
    pub fn words(&self) -> String {
        self.message.clone()
    }
}

impl Server {
    fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
    ) -> Request {
        let mut url = format!("{}{path}", self.base);
        for (at, (name, value)) in query.iter().enumerate() {
            url.push(if at == 0 { '?' } else { '&' });
            url.push_str(name);
            url.push('=');
            url.push_str(&escaped(value));
        }
        Request {
            method,
            url,
            token: self.token.clone(),
            body: None,
            long: false,
        }
    }

    fn get(&self, path: &str) -> Request {
        self.request(Method::GET, path, &[])
    }

    fn post(
        &self,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Request {
        self.with(Method::POST, path, body)
    }

    /// A request of `method` with `body` as JSON.
    fn with(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Request {
        Request {
            body: body.map(|body| {
                (
                    "application/json".to_string(),
                    serde_json::to_vec(&body).unwrap_or_default(),
                )
            }),
            ..self.request(method, path, &[])
        }
    }

    /// Listings on the shelves, of every kind, from `offset` on.
    pub fn listings(&self, offset: u32) -> Request {
        let query = [
            ("limit", PAGE.to_string()),
            ("offset", offset.to_string()),
        ];
        self.request(Method::GET, "/v1/listings", &query)
    }

    pub fn detail(&self, id: &str) -> Request {
        self.get(&format!("/v1/listings/{}", escaped(id)))
    }

    /// Begins a sign-in with GitHub.
    pub fn device(&self) -> Request {
        self.post("/v1/auth/device", None)
    }

    /// Asks whether the sign-in of `code` was approved.
    pub fn poll(&self, code: &str) -> Request {
        let body = json!({ "deviceCode": code });
        self.post("/v1/auth/device/poll", Some(body))
    }

    pub fn sign_out(&self) -> Request {
        self.with(Method::DELETE, "/v1/auth/session", None)
    }

    pub fn me(&self) -> Request {
        self.get("/v1/me")
    }

    /// Everything the one signed in owns.
    pub fn library(&self) -> Request {
        self.get("/v1/me/library")
    }

    /// The listings of the one signed in, in every state.
    pub fn mine(&self) -> Request {
        self.get("/v1/me/listings")
    }

    /// The page on Stripe where the one signed in sets up how they
    /// are paid.
    pub fn payouts(&self) -> Request {
        self.post("/v1/me/payouts", None)
    }

    /// Buys a listing: owned at once when it is free, or a page on
    /// Stripe to pay on.
    pub fn purchase(&self, id: &str) -> Request {
        let path = format!("/v1/listings/{}/purchase", escaped(id));
        self.post(&path, None)
    }

    pub fn purchase_state(&self, purchase: &str) -> Request {
        self.get(&format!("/v1/purchases/{}", escaped(purchase)))
    }

    /// Every official asset on the shelves of the LOOKS stall.
    pub fn assets(&self) -> Request {
        self.get("/v1/assets")
    }

    /// Every official asset the one signed in owns, those taken off
    /// the shelves among them.
    pub fn my_assets(&self) -> Request {
        self.get("/v1/me/assets")
    }

    /// Buys the asset `id`: owned at once when it is free, or a page
    /// on Stripe to pay on.
    pub fn asset_purchase(&self, id: &str) -> Request {
        let path = format!("/v1/assets/{}/purchase", escaped(id));
        self.post(&path, None)
    }

    pub fn asset_purchase_state(&self, purchase: &str) -> Request {
        self.get(&format!(
            "/v1/asset-purchases/{}",
            escaped(purchase)
        ))
    }

    /// `limit` bytes of the file of the asset `id`, which the one
    /// signed in owns, from `offset` on.
    pub fn asset_file_part(
        &self,
        id: &str,
        offset: u64,
        limit: u64,
    ) -> Request {
        let path = format!("/v1/assets/{}/file", escaped(id));
        let query = [
            ("offset", offset.to_string()),
            ("limit", limit.to_string()),
        ];
        self.request(Method::GET, &path, &query)
    }

    /// The package of `version` of a listing, whole.
    pub fn package(&self, id: &str, version: &str) -> Request {
        Request {
            long: true,
            ..self.get(&format!(
                "/v1/listings/{}/versions/{}/package",
                escaped(id),
                escaped(version)
            ))
        }
    }

    /// Puts up a listing, not yet on the shelves.
    pub fn create(&self, draft: &Draft) -> Request {
        self.post("/v1/listings", Some(json!(draft)))
    }

    /// Changes a listing's words, price or tags.
    pub fn change(&self, id: &str, draft: &Draft) -> Request {
        let draft = Draft {
            kind: None,
            ..draft.clone()
        };
        let path = format!("/v1/listings/{}", escaped(id));
        self.with(Method::PATCH, &path, Some(json!(draft)))
    }

    /// The picture at `path`, an address on the server that may hold
    /// a query of its own.
    pub fn picture(&self, path: &str) -> Request {
        Request {
            method: Method::GET,
            url: format!("{}{path}", self.base),
            token: self.token.clone(),
            body: None,
            long: false,
        }
    }

    /// Sets a listing's cover, `bytes` of a picture of `kind`.
    pub fn cover(
        &self,
        id: &str,
        bytes: Vec<u8>,
        kind: &str,
    ) -> Request {
        let path = format!("/v1/listings/{}/cover", escaped(id));
        Request {
            body: Some((kind.to_string(), bytes)),
            ..self.request(Method::PUT, &path, &[])
        }
    }

    /// Makes `version` of a listing of `package`, the bytes of its
    /// JSON as the home packed it.
    pub fn release(
        &self,
        id: &str,
        version: &str,
        changes: &str,
        package: &[u8],
    ) -> Request {
        let path = format!("/v1/listings/{}/versions", escaped(id));
        let package =
            serde_json::from_slice::<serde_json::Value>(package)
                .unwrap_or_default();
        let body = json!({
            "version": version,
            "changes": changes,
            "package": package,
        });
        Request {
            long: true,
            ..self.post(&path, Some(body))
        }
    }

    /// Puts a listing on the shelves.
    pub fn publish(&self, id: &str) -> Request {
        let path = format!("/v1/listings/{}/publish", escaped(id));
        self.post(&path, None)
    }

    /// Takes a listing off the shelves. Its owners keep it.
    pub fn unlist(&self, id: &str) -> Request {
        let path = format!("/v1/listings/{}/unlist", escaped(id));
        self.post(&path, None)
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
pub(crate) fn bytes(answer: Fetched) -> Result<Vec<u8>, Refusal> {
    let (status, body) = answer.map_err(|why| {
        Refusal::new(
            None,
            "unreachable",
            format!("The bazaar could not be reached: {why}."),
        )
    })?;
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
    answer: Fetched,
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
pub(crate) fn done(answer: Fetched) -> Result<(), Refusal> {
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

//! The bazaar's server as it is called: each of its routes the
//! bazaar uses, as the request that
//! asks it, and what an answer comes to. The window sends the session
//! token as a bearer header and each route's own method.

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::bazaar::errand::Fetched;
use crate::bazaar::market::lot::Kind;
use crate::bazaar::market::versions::Details;

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
    /// The server's code for it, such as `conflict` or
    /// `EMAIL_NOT_VERIFIED`; `unreachable` when it was not reached.
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

    /// In words for the chief, saying what to do of a limit on
    /// selling.
    pub fn words(&self) -> String {
        match self.code.as_str() {
            "email_unverified" => "Verify your email before you \
                                   sell: sign out, sign in again \
                                   and type the code we send to it."
                .into(),
            "account_too_new" => {
                // The message ends with the day and time, as
                // 2026-11-03T12:00:00.000Z.
                let from = self
                    .message
                    .split_whitespace()
                    .find(|word| word.get(10..11) == Some("T"))
                    .and_then(|word| word.get(..10));
                let mut words = "Your account needs to be 7 days \
                                 old before you can sell."
                    .to_string();
                if let Some(from) = from {
                    words.push_str(&format!(
                        " You can sell from {from}."
                    ));
                }
                words
            }
            "too_many_submissions" => "You can send 3 listings or \
                                       versions to be checked a \
                                       day. Try again tomorrow."
                .into(),
            "too_many_pending" => "Two of yours are being checked. \
                                   Send this once one of them is \
                                   done."
                .into(),
            _ => self.message.clone(),
        }
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

    /// Makes a Hamlet account, which waits for the code emailed to
    /// `email`.
    pub fn sign_up(
        &self,
        email: &str,
        password: &str,
        username: &str,
    ) -> Request {
        // Better Auth needs a name; the server sets it to the
        // username.
        let body = json!({
            "email": email,
            "password": password,
            "name": username,
            "username": username,
        });
        self.post("/api/auth/sign-up/email", Some(body))
    }

    /// Verifies the email of an account with the code sent to it,
    /// which signs it in.
    pub fn verify(&self, email: &str, code: &str) -> Request {
        let body = json!({ "email": email, "otp": code });
        self.post("/api/auth/email-otp/verify-email", Some(body))
    }

    /// Sends another code to verify `email` with.
    pub fn resend(&self, email: &str) -> Request {
        let body =
            json!({ "email": email, "type": "email-verification" });
        self.post(
            "/api/auth/email-otp/send-verification-otp",
            Some(body),
        )
    }

    pub fn sign_in(&self, email: &str, password: &str) -> Request {
        let body = json!({ "email": email, "password": password });
        self.post("/api/auth/sign-in/email", Some(body))
    }

    /// Sends a code to `email` to choose a new password with.
    pub fn forgot(&self, email: &str) -> Request {
        let body = json!({ "email": email });
        self.post(
            "/api/auth/email-otp/request-password-reset",
            Some(body),
        )
    }

    /// Chooses a new password with the code sent to `email`.
    pub fn reset(
        &self,
        email: &str,
        code: &str,
        password: &str,
    ) -> Request {
        let body = json!({ "email": email, "otp": code, "password": password });
        self.post("/api/auth/email-otp/reset-password", Some(body))
    }

    pub fn sign_out(&self) -> Request {
        self.post("/api/auth/sign-out", None)
    }

    pub fn rename(&self, username: &str) -> Request {
        let body = json!({ "username": username });
        self.post("/api/auth/update-user", Some(body))
    }

    /// Changes the password, which signs out every other session.
    pub fn change_password(
        &self,
        current: &str,
        fresh: &str,
    ) -> Request {
        let body = json!({
            "currentPassword": current,
            "newPassword": fresh,
            "revokeOtherSessions": true,
        });
        self.post("/api/auth/change-password", Some(body))
    }

    pub fn me(&self) -> Request {
        self.get("/v1/me")
    }

    /// The vault of the one signed in: its salt, its locked data key
    /// and its sealed OpenRouter key.
    pub fn vault(&self) -> Request {
        self.get("/v1/me/vault")
    }

    /// Keeps the vault's `salt` and its data key, `locked`.
    pub fn keep_vault(&self, salt: &str, locked: &str) -> Request {
        let body = json!({ "salt": salt, "dataKey": locked });
        self.with(Method::PUT, "/v1/me/vault", Some(body))
    }

    /// Keeps the OpenRouter key, `sealed`, in the vault.
    pub fn keep_sealed(&self, sealed: &str) -> Request {
        let body = json!({ "sealed": sealed });
        self.with(Method::PUT, "/v1/me/vault/openrouter", Some(body))
    }

    /// Forgets the OpenRouter key kept in the vault.
    pub fn drop_sealed(&self) -> Request {
        self.with(Method::DELETE, "/v1/me/vault/openrouter", None)
    }

    /// The seller terms as they stand.
    pub fn seller_terms(&self) -> Request {
        self.get("/v1/seller-terms")
    }

    /// Agrees to `version` of the seller terms.
    pub fn agree(&self, version: &str) -> Request {
        let body = json!({ "version": version });
        self.post("/v1/me/seller-terms", Some(body))
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

    /// Whether Stripe lets the one signed in be paid, as it says now.
    pub fn payouts_state(&self) -> Request {
        self.get("/v1/me/payouts")
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

    /// The file of the asset `id`, veiled, to be viewed by anyone.
    pub fn asset_view(&self, id: &str) -> Request {
        self.get(&format!("/v1/assets/{}/view", escaped(id)))
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

    /// Puts up a listing of `kind` and `title` with `package`, the
    /// bytes of its JSON as the home packed it, as its draft
    /// first version.
    pub fn create_with(
        &self,
        kind: Kind,
        title: &str,
        package: &[u8],
    ) -> Request {
        let package =
            serde_json::from_slice::<serde_json::Value>(package)
                .unwrap_or_default();
        let body = json!({ "kind": kind, "title": title, "package": package });
        Request {
            long: true,
            ..self.post("/v1/listings", Some(body))
        }
    }

    /// Where a payment stands, asked of Stripe while it is pending.
    pub fn payment(&self, id: &str) -> Request {
        self.get(&format!("/v1/payments/{}", escaped(id)))
    }

    /// The bazaar's terms for selling: the kinds sold, the fees to
    /// verify and the least price.
    pub fn sell_config(&self) -> Request {
        self.get("/v1/sell/config")
    }

    /// The tags most used that hold `words`.
    pub fn tags(&self, words: &str) -> Request {
        let query = [("q", words.to_string())];
        self.request(Method::GET, "/v1/tags", &query)
    }

    /// The path of `version` of the listing `id`, and `rest` after
    /// it.
    fn version_path(id: &str, version: &str, rest: &str) -> String {
        format!(
            "/v1/listings/{}/versions/{}{rest}",
            escaped(id),
            escaped(version)
        )
    }

    /// Uploads `package`, the bytes of its JSON as the home packed
    /// it, as the draft `version` of the listing `id`.
    pub fn upload(
        &self,
        id: &str,
        version: &str,
        package: &[u8],
    ) -> Request {
        let path = format!("/v1/listings/{}/versions", escaped(id));
        let package =
            serde_json::from_slice::<serde_json::Value>(package)
                .unwrap_or_default();
        let body = json!({ "version": version, "package": package });
        Request {
            long: true,
            ..self.post(&path, Some(body))
        }
    }

    /// Changes the details of a draft version.
    pub fn change_version(
        &self,
        id: &str,
        version: &str,
        details: &Details,
    ) -> Request {
        let path = Self::version_path(id, version, "");
        self.with(Method::PATCH, &path, Some(json!(details)))
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

    /// Sets the cover of a draft version, `bytes` of a picture of
    /// `kind`.
    pub fn version_cover(
        &self,
        id: &str,
        version: &str,
        bytes: Vec<u8>,
        kind: &str,
    ) -> Request {
        let path = Self::version_path(id, version, "/cover");
        Request {
            body: Some((kind.to_string(), bytes)),
            ..self.request(Method::PUT, &path, &[])
        }
    }

    /// Throws a draft version away.
    pub fn discard(&self, id: &str, version: &str) -> Request {
        let path = Self::version_path(id, version, "");
        self.with(Method::DELETE, &path, None)
    }

    /// Puts a draft or rejected version on the shelves, unverified.
    pub fn publish(&self, id: &str, version: &str) -> Request {
        self.post(&Self::version_path(id, version, "/publish"), None)
    }

    /// Stripe's page to pay for a draft version to be verified.
    pub fn verify_version(&self, id: &str, version: &str) -> Request {
        self.post(&Self::version_path(id, version, "/verify"), None)
    }

    /// The files of a version, against the version `against`.
    pub fn files(
        &self,
        id: &str,
        version: &str,
        against: Option<&str>,
    ) -> Request {
        let path = Self::version_path(id, version, "/files");
        let query = against
            .map(|against| ("against", against.to_string()))
            .into_iter()
            .collect::<Vec<_>>();
        self.request(Method::GET, &path, &query)
    }

    /// Asks a person at the bazaar to look again at `version` of a
    /// listing, which the checks rejected.
    pub fn ask_human(&self, id: &str, version: &str) -> Request {
        let path = format!(
            "/v1/listings/{}/versions/{}/human-review",
            escaped(id),
            escaped(version)
        );
        self.post(&path, Some(json!({})))
    }

    /// Tells the bazaar what is wrong with someone else's listing.
    pub fn report(&self, id: &str, reason: &str) -> Request {
        let path = format!("/v1/listings/{}/report", escaped(id));
        self.post(&path, Some(json!({ "reason": reason })))
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
    // The bazaar's own routes wrap it in `error`; its accounts' do
    // not.
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Fault {
        Wrapped { error: FaultBody },
        Bare(FaultBody),
    }
    #[derive(serde::Deserialize)]
    struct FaultBody {
        #[serde(default)]
        code: String,
        message: String,
    }
    Err(match serde_json::from_slice::<Fault>(&body) {
        Ok(Fault::Wrapped { error } | Fault::Bare(error)) => {
            Refusal::new(
                Some(status),
                &error.code,
                sentence(&error.message),
            )
        }
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

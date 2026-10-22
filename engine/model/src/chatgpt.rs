//! Signing in with ChatGPT, so that models run on the chief's ChatGPT
//! plan, by the flow of
//! <https://developers.openai.com/siwc/token-sharing-open-source/sign-in>.

mod id_token;

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::Endpoint;
use crate::config::keys::{load_secret, save_secret};

const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str =
    "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str =
    "https://auth.openai.com/api/accounts/oauth/token";
const REVOKE: &str =
    "https://auth.openai.com/api/accounts/oauth/revoke";
const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";

/// The address models are served at, and the resource the tokens are
/// for.
pub const BASE_URL: &str = "https://api.openai.com/v1";

/// The page where the chief sees and limits what Hamlet uses of the
/// plan.
pub const USAGE: &str = "https://chatgpt.com/settings/usage";

/// The name of the endpoint in the settings.
pub const ENDPOINT: &str = "chatgpt";

/// The client id a first sign-in registers with.
const REGISTER: &str = "dynamic_agent_client";

/// The name the chief is shown for Hamlet when it registers.
const AGENT_NAME: &str = "Hamlet";

/// The scope that lets the plan be used.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";

const SCOPES: &str = "openid profile email offline_access \
                      resource.invoke chatgpt.tokens.use.direct";

/// The port asked for first. Any other will do when it is taken.
const PORT: u16 = 1455;

const CALLBACK: &str = "/auth/callback";

/// Time the chief has to sign in in the browser.
const PATIENCE: Duration = Duration::from_secs(300);

/// Seconds left on an access token below which it is renewed before
/// it is used.
const RENEW_BEFORE: u64 = 300;

/// The names the account and this computer's host id are kept under.
const ACCOUNT: &str = "chatgpt.account";
const HOST: &str = "chatgpt.host";

/// The error refresh codes after which the sign-in cannot be renewed.
const ENDED: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

const SIGNED_OUT: &str = "Hamlet is not signed in with ChatGPT. \
                          Sign in again from Settings, Models";

/// The page the browser is shown once the sign-in has come back,
/// around what came of it.
const PAGE: [&str; 2] = [
    "<!doctype html><meta charset=utf-8><title>Hamlet</title><body \
     style=\"font-family:sans-serif;background:#14110d;color:#\
     e8dcc0;text-align:center;padding-top:20vh\"><h1>",
    "</h1><p>You can close this tab and go back to Hamlet.</p>",
];

/// The ChatGPT account signed in with, as kept between launches.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Account {
    /// The client registered for this account, `oaiapp_...`.
    client_id: String,
    /// The account's id in its verified ID token.
    subject: String,
    email: String,
    id_token: String,
    access_token: String,
    refresh_token: String,
    /// Unix seconds at which `access_token` ends.
    expires_at: u64,
    scopes: Vec<String>,
}

impl Account {
    /// Takes the tokens of an answer of the token endpoint, and the
    /// scopes it granted, or `scope` where it names none.
    fn take(&mut self, tokens: &Value, scope: &str) -> Result<()> {
        let text = |key: &str| {
            tokens[key].as_str().filter(|text| !text.is_empty())
        };
        self.access_token = text("access_token")
            .context("ChatGPT gave no access token")?
            .into();
        if let Some(refresh) = text("refresh_token") {
            self.refresh_token = refresh.into();
        }
        self.expires_at =
            now() + tokens["expires_in"].as_u64().unwrap_or(3600);
        let granted = text("scope").unwrap_or(scope);
        if !granted.is_empty() {
            self.scopes = granted
                .split_whitespace()
                .map(String::from)
                .collect();
        }
        Ok(())
    }

    fn may_use_plan(&self) -> bool {
        self.scopes.iter().any(|scope| scope == PLAN_SCOPE)
    }

    /// Forgets the tokens, and keeps the registration for the next
    /// sign-in.
    fn sign_out(&mut self) {
        self.id_token.clear();
        self.access_token.clear();
        self.refresh_token.clear();
        self.expires_at = 0;
    }
}

/// The endpoint of the plan.
pub fn endpoint() -> Endpoint {
    Endpoint {
        name: ENDPOINT.into(),
        base_url: BASE_URL.into(),
        chatgpt: true,
        api_key: None,
    }
}

fn load(home: &Path) -> Option<Account> {
    let text = load_secret(home, ACCOUNT)?;
    serde_json::from_str(&text).ok()
}

fn save(home: &Path, account: &Account) -> Result<()> {
    save_secret(home, ACCOUNT, Some(&serde_json::to_string(account)?))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .ok()
        .context("the system gave no randomness")?;
    Ok(bytes)
}

/// A fresh value nobody can guess, for a state, a nonce or a PKCE
/// verifier.
fn unguessable() -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(random::<32>()?))
}

/// This computer's host id: a version 4 UUID made at the first
/// sign-in and kept from then on.
fn host_id(home: &Path) -> Result<String> {
    if let Some(known) = load_secret(home, HOST) {
        return Ok(known);
    }
    let mut bytes = random::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex =
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let id = format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    save_secret(home, HOST, Some(&id))?;
    Ok(id)
}

/// An access token with time left, renewed first when it is near its
/// end.
pub async fn access_token(home: &Path) -> Result<String> {
    // A renewal replaces the refresh token, so two at once would
    // spend the same one twice and end the sign-in.
    static RENEWING: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());
    let _alone = RENEWING.lock().await;
    let mut account = load(home)
        .filter(|account| !account.access_token.is_empty())
        .context(SIGNED_OUT)?;
    if account.expires_at > now() + RENEW_BEFORE {
        return Ok(account.access_token);
    }
    let answer = reqwest::Client::new()
        .post(TOKEN)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &account.client_id),
            ("refresh_token", &account.refresh_token),
            ("resource", BASE_URL),
        ])
        .send()
        .await
        .context("could not reach ChatGPT to renew the sign-in")?;
    let status = answer.status();
    let body = answer.json::<Value>().await.unwrap_or_default();
    if !status.is_success() {
        let code = body["error"]
            .as_str()
            .or(body["error"]["code"].as_str())
            .unwrap_or_default();
        if ENDED.contains(&code) {
            account.sign_out();
            save(home, &account)?;
            bail!(
                "the ChatGPT sign-in has ended ({code}). \
                 {SIGNED_OUT}"
            );
        }
        bail!(
            "ChatGPT did not renew the sign-in (HTTP {status}): \
             {body}"
        );
    }
    account.take(&body, "")?;
    save(home, &account)?;
    Ok(account.access_token)
}

/// Revokes the sign-in at OpenAI and forgets its tokens. The
/// registration and the host id are kept for the next sign-in.
pub async fn sign_out(home: &Path) -> Result<()> {
    let Some(mut account) = load(home) else {
        return Ok(());
    };
    let mut revoked = Ok(());
    if !account.refresh_token.is_empty() {
        for wait in [1, 2, 4] {
            revoked = revoke(&account).await;
            if revoked.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    }
    account.sign_out();
    save(home, &account)?;
    revoked
}

async fn revoke(account: &Account) -> Result<()> {
    let answer = reqwest::Client::new()
        .post(REVOKE)
        .form(&[
            ("token", &*account.refresh_token),
            ("token_type_hint", "refresh_token"),
            ("client_id", &account.client_id),
        ])
        .send()
        .await
        .context("could not reach ChatGPT to sign out")?;
    let status = answer.status();
    anyhow::ensure!(
        status.is_success(),
        "ChatGPT did not confirm the sign-out (HTTP {status})"
    );
    Ok(())
}

/// A sign-in that has begun.
pub struct SignIn {
    /// The page the chief signs in on.
    pub url: String,
    wait: TcpListener,
    redirect: String,
    verifier: String,
    state: String,
    nonce: String,
    /// The account signed in with before, whose registration is
    /// used again.
    known: Option<Account>,
}

/// Opens a port for the answer and says which page to open.
pub async fn begin(home: &Path) -> Result<SignIn> {
    let host = host_id(home)?;
    let known =
        load(home).filter(|account| !account.client_id.is_empty());
    let wait = match TcpListener::bind(("127.0.0.1", PORT)).await {
        Ok(wait) => wait,
        Err(_) => TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("could not open a port for the answer")?,
    };
    let port = wait.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK}");
    let (verifier, state, nonce) =
        (unguessable()?, unguessable()?, unguessable()?);
    let challenge =
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut asked = vec![
        ("response_type", "code"),
        ("redirect_uri", &*redirect),
        ("scope", SCOPES),
        ("resource", BASE_URL),
        ("state", &*state),
        ("nonce", &*nonce),
        ("code_challenge_method", "S256"),
        ("code_challenge", &*challenge),
        ("ext_agent_host_id", &*host),
    ];
    match &known {
        Some(account) => {
            asked.push(("client_id", &account.client_id));
            if !account.id_token.is_empty() {
                asked.push(("id_token_hint", &account.id_token));
            }
            if !account.email.is_empty() {
                asked.push(("login_hint", &account.email));
            }
            // Asked again for the plan it was refused before.
            if !account.may_use_plan() {
                asked.push(("prompt", "consent"));
            }
        }
        None => {
            asked.push(("client_id", REGISTER));
            asked.push(("agent_name_hint", AGENT_NAME));
        }
    }
    let url = reqwest::Url::parse_with_params(AUTHORIZE, &asked)?;
    Ok(SignIn {
        url: url.into(),
        wait,
        redirect,
        verifier,
        state,
        nonce,
        known,
    })
}

/// What came back to the callback, once its state was checked.
#[derive(Debug, PartialEq)]
struct Callback {
    code: String,
    client_id: String,
    scope: String,
}

/// The pairs of the query of a request's first line to the callback.
fn query(line: &str) -> Option<Vec<(String, String)>> {
    let path = line.split_whitespace().nth(1)?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))
        .ok()
        .filter(|url| url.path() == CALLBACK)?;
    Some(url.query_pairs().into_owned().collect())
}

/// Reads what came back to the callback: an error, or the code and
/// the client it is for. `known` is the client asked with when it
/// was registered before.
fn read_callback(
    pairs: &[(String, String)],
    known: Option<&str>,
) -> Result<Callback> {
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty())
    };
    match get("error") {
        Some("access_denied") => {
            bail!("you chose not to let Hamlet use your ChatGPT plan")
        }
        Some(error) => bail!(
            "ChatGPT said {error}: {}",
            get("error_description").unwrap_or_default()
        ),
        None => {}
    }
    let code = get("code").context("ChatGPT sent back no code")?;
    let client_id = match (known, get("client_id")) {
        (Some(known), Some(given)) if known != given => {
            bail!("ChatGPT answered for another registration")
        }
        (Some(known), _) => known,
        (None, Some(given)) if given != REGISTER => given,
        (None, _) => bail!("ChatGPT did not finish the registration"),
    };
    Ok(Callback {
        code: code.into(),
        client_id: client_id.into(),
        scope: get("scope").unwrap_or_default().into(),
    })
}

/// Reads one request and answers it. Its query, if this was the
/// answer to this sign-in.
async fn hear(
    mut from: TcpStream,
    state: &str,
) -> Option<Vec<(String, String)>> {
    let mut bytes = [0u8; 8192];
    let read = from.read(&mut bytes).await.ok()?;
    let text = String::from_utf8_lossy(&bytes[..read]);
    let pairs = query(text.lines().next().unwrap_or_default())
        .filter(|pairs| {
            pairs
                .iter()
                .any(|(key, value)| key == "state" && value == state)
        });
    let (status, body) = match &pairs {
        Some(pairs) if pairs.iter().any(|(key, _)| key == "code") => {
            ("200 OK", PAGE.join("Hamlet is signed in with ChatGPT"))
        }
        Some(_) => ("200 OK", PAGE.join("Hamlet was not signed in")),
        None => ("404 Not Found", String::new()),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: \
         text/html\r\nContent-Length: {}\r\nConnection: \
         close\r\n\r\n{body}",
        body.len()
    );
    let _ = from.write_all(reply.as_bytes()).await;
    pairs
}

/// Posts a form to the token endpoint and returns its answer.
async fn tokens(form: &[(&str, &str)]) -> Result<Value> {
    let answer = reqwest::Client::new()
        .post(TOKEN)
        .form(form)
        .send()
        .await
        .context("could not reach ChatGPT")?;
    let status = answer.status();
    let body = answer.json::<Value>().await.unwrap_or_default();
    anyhow::ensure!(
        status.is_success(),
        "ChatGPT gave no tokens (HTTP {status}): {}",
        body["error"]
    );
    Ok(body)
}

impl SignIn {
    /// Waits for the chief to sign in, trades the code for tokens,
    /// checks them and keeps them. The email of the account.
    pub async fn finish(self, home: &Path) -> Result<String> {
        let pairs = tokio::time::timeout(PATIENCE, async {
            loop {
                let Ok((from, _)) = self.wait.accept().await else {
                    continue;
                };
                if let Some(pairs) = hear(from, &self.state).await {
                    return pairs;
                }
            }
        })
        .await
        .ok()
        .context("nobody signed in in the browser in time")?;
        let known = self.known.as_ref();
        let back = read_callback(
            &pairs,
            known.map(|account| &*account.client_id),
        )?;
        let answer = tokens(&[
            ("grant_type", "authorization_code"),
            ("client_id", &back.client_id),
            ("code", &back.code),
            ("code_verifier", &self.verifier),
            ("redirect_uri", &self.redirect),
            ("resource", BASE_URL),
        ])
        .await?;
        let id_token = answer["id_token"]
            .as_str()
            .context("ChatGPT gave no ID token")?;
        let keys = reqwest::get(JWKS)
            .await
            .context("could not reach ChatGPT for its keys")?
            .json::<Value>()
            .await?;
        let claims = id_token::verify(
            id_token,
            &keys,
            ISSUER,
            &back.client_id,
            &self.nonce,
            now(),
        )?;
        if let Some(known) = known
            && !known.subject.is_empty()
            && known.subject != claims.sub
        {
            save_secret(home, ACCOUNT, None)?;
            bail!(
                "that is another ChatGPT account than the one this \
                 home signed in with before; sign in again to use it"
            );
        }
        let mut account = Account {
            client_id: back.client_id,
            subject: claims.sub,
            email: claims.email,
            id_token: id_token.into(),
            ..Account::default()
        };
        account.take(&answer, &back.scope)?;
        if !account.may_use_plan() {
            account.sign_out();
            save(home, &account)?;
            bail!(
                "you signed in, but Hamlet was not allowed to use \
                 your ChatGPT plan; sign in again and allow it"
            );
        }
        save(home, &account)?;
        Ok(account.email)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callback_gives_a_code_only_for_the_registration_asked_for()
    {
        let line = |asked: &str| {
            query(&format!("GET {CALLBACK}?{asked} HTTP/1.1"))
                .unwrap()
        };
        let first = line(
            "code=c%2B1&state=s&client_id=oaiapp_1&\
             scope=openid+chatgpt.tokens.use.direct",
        );
        assert_eq!(
            read_callback(&first, None).unwrap(),
            Callback {
                code: "c+1".into(),
                client_id: "oaiapp_1".into(),
                scope: "openid chatgpt.tokens.use.direct".into(),
            }
        );
        // A first registration must say which client it made.
        let unnamed = line("code=c&state=s");
        assert!(read_callback(&unnamed, None).is_err());
        let placeholder =
            line(&format!("code=c&client_id={REGISTER}"));
        assert!(read_callback(&placeholder, None).is_err());
        // A sign-in again keeps its client, and refuses another.
        assert_eq!(
            read_callback(&unnamed, Some("oaiapp_1"))
                .unwrap()
                .client_id,
            "oaiapp_1"
        );
        assert!(read_callback(&first, Some("oaiapp_2")).is_err());
        let refused = line("error=access_denied&state=s");
        let why =
            read_callback(&refused, Some("oaiapp_1")).unwrap_err();
        assert!(why.to_string().contains("chose not to"));
        // Only the callback's own path is read.
        assert!(query("GET /favicon.ico HTTP/1.1").is_none());
    }
}

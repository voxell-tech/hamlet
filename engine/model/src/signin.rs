//! Signing in to OpenRouter from the app: the chief approves in a browser and a key comes back, by the exchange of <https://openrouter.ai/docs/use-cases/oauth-pkce>.

use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const AUTHORISE: &str = "https://openrouter.ai/auth";
const EXCHANGE: &str = "https://openrouter.ai/api/v1/auth/keys";

/// Time the chief has to approve in the browser.
const PATIENCE: Duration = Duration::from_secs(300);

/// The page the browser is shown once the code has come back.
const DONE: &str =
    "<!doctype html><meta charset=utf-8><title>Hamlet</title><body \
     style=\"font-family:sans-serif;background:#14110d;color:#\
     e8dcc0;text-align:center;padding-top:20vh\"><h1>Hamlet is \
     connected</h1><p>You can close this tab and go back to \
     Hamlet.</p>";

/// A sign-in that has begun.
pub struct SignIn {
    /// The page the chief approves on.
    pub url: String,
    waits: Vec<TcpListener>,
    secret: String,
    state: String,
}

/// Sixty-four letters nobody can guess, from the randomness every
/// process is given for its hash maps.
fn secret() -> String {
    let mut hash = Sha256::new();
    for _ in 0..4 {
        let seed = std::collections::hash_map::RandomState::new();
        hash.update(seed.build_hasher().finish().to_le_bytes());
    }
    hash.update(std::process::id().to_le_bytes());
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// `bytes` in the web's own base 64, with no padding.
fn base64url(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
        abcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b))
                << (8 * (3 - chunk.len()));
        for at in 0..=chunk.len() {
            let six = (n >> (18 - 6 * at)) & 63;
            out.push(LETTERS[six as usize] as char);
        }
    }
    out
}

/// The value of `name` in the query of a request's first line.
fn query<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let path = line.split_whitespace().nth(1)?;
    let (_, query) = path.split_once('?')?;
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// Opens a port for the answer and says which page to open.
pub async fn begin() -> Result<SignIn> {
    let first = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("could not open a port for the answer")?;
    let port = first.local_addr()?.port();
    let mut waits = vec![first];
    // A browser may look for `localhost` at either address.
    if let Ok(other) = TcpListener::bind(("::1", port)).await {
        waits.push(other);
    }
    let (secret, state) = (secret(), secret()[..16].to_string());
    let proof = base64url(&Sha256::digest(secret.as_bytes()));
    let url = format!(
        "{AUTHORISE}?callback_url=http%3A%2F%2Flocalhost%3A{port}%\
         2F&code_challenge={proof}&code_challenge_method=S256&\
         state={state}&key_label=Hamlet"
    );
    Ok(SignIn {
        url,
        waits,
        secret,
        state,
    })
}

/// Reads one request and answers it. The code, if this was the
/// answer that carries it.
async fn hear(mut from: TcpStream, state: &str) -> Option<String> {
    let mut bytes = [0u8; 4096];
    let read = from.read(&mut bytes).await.ok()?;
    let text = String::from_utf8_lossy(&bytes[..read]);
    let line = text.lines().next().unwrap_or_default();
    let code = query(line, "code")
        .filter(|_| query(line, "state") == Some(state))
        .map(str::to_string);
    let (status, body) = match code {
        Some(_) => ("200 OK", DONE),
        None => ("404 Not Found", ""),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: \
         text/html\r\nContent-Length: {}\r\nConnection: \
         close\r\n\r\n{body}",
        body.len()
    );
    let _ = from.write_all(reply.as_bytes()).await;
    code
}

impl SignIn {
    /// Waits for the chief to approve, and trades the code that
    /// comes back for the key.
    pub async fn finish(self) -> Result<String> {
        let code = tokio::time::timeout(PATIENCE, async {
            loop {
                let heard = match self.waits.as_slice() {
                    [one] => one.accept().await,
                    [one, other, ..] => tokio::select! {
                        heard = one.accept() => heard,
                        heard = other.accept() => heard,
                    },
                    [] => unreachable!("a port was opened"),
                };
                let Ok((from, _)) = heard else {
                    continue;
                };
                if let Some(code) = hear(from, &self.state).await {
                    return code;
                }
            }
        })
        .await
        .ok()
        .context("nobody approved in the browser in time")?;
        let answer = reqwest::Client::new()
            .post(EXCHANGE)
            .json(&json!({
                "code": code,
                "code_verifier": self.secret,
                "code_challenge_method": "S256",
            }))
            .send()
            .await
            .context("could not reach OpenRouter")?;
        let status = answer.status();
        let body = answer.json::<Value>().await.unwrap_or_default();
        match body["key"].as_str() {
            Some(key) if status.is_success() => Ok(key.to_string()),
            _ => bail!("OpenRouter gave no key ({status}): {body}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proof_is_written_as_openrouter_reads_it() {
        // The pair RFC 7636 gives as its example.
        let secret = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            base64url(&Sha256::digest(secret.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(base64url(b"a"), "YQ");
        assert_eq!(base64url(b"ab"), "YWI");
    }

    #[tokio::test]
    async fn the_code_that_comes_back_is_heard_and_no_other() {
        let sign_in = begin().await.unwrap();
        let port = sign_in.waits[0].local_addr().unwrap().port();
        assert!(sign_in.url.contains(&format!("localhost%3A{port}")));
        let state = sign_in.state.clone();
        let ask = |path: String| async move {
            let mut to = TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let line = format!("GET {path} HTTP/1.1\r\n\r\n");
            to.write_all(line.as_bytes()).await.unwrap();
        };
        let waits = &sign_in.waits[0];
        // A browser asks for more than the page it was sent to.
        ask("/favicon.ico".into()).await;
        let (from, _) = waits.accept().await.unwrap();
        assert_eq!(hear(from, &state).await, None);
        ask("/?code=abc&state=wrong".into()).await;
        let (from, _) = waits.accept().await.unwrap();
        assert_eq!(hear(from, &state).await, None);
        ask(format!("/?code=abc&state={state}")).await;
        let (from, _) = waits.accept().await.unwrap();
        assert_eq!(hear(from, &state).await, Some("abc".into()));
    }
}

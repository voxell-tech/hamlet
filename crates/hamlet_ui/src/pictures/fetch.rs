//! A picture fetched from the web, which happens only when a chief
//! asks for it (docs/chat.md).

use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};

use super::load::{Bounds, Pixels, decode};

/// The time a picture is given to arrive whole.
const PATIENCE: Duration = Duration::from_secs(20);

/// The most addresses a fetch is passed on to.
const HOPS: usize = 5;

/// Who a fetch says it is. Some servers, Wikimedia's among them,
/// refuse one that does not say.
const AGENT: &str = concat!(
    "Hamlet/",
    env!("CARGO_PKG_VERSION"),
    " (https://hamlet.gg)"
);

/// True when `scheme` is one a picture is fetched over.
fn webbed(scheme: &str) -> bool {
    matches!(scheme, "http" | "https")
}

/// The host `address` would be fetched from, if it is an address a
/// picture may be fetched from.
pub(crate) fn host(address: &str) -> Option<String> {
    let url = Url::parse(address).ok()?;
    let host = url.host_str().filter(|_| webbed(url.scheme()))?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// True when a server says what it sends is a picture.
fn pictured(kind: &str) -> bool {
    kind.trim_start().to_ascii_lowercase().starts_with("image/")
}

/// The bytes at `address`, if they are a picture no larger than
/// `most` bytes.
async fn bytes(address: &str, most: u64) -> Option<Vec<u8>> {
    let client = Client::builder()
        .timeout(PATIENCE)
        .user_agent(AGENT)
        .referer(false)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= HOPS
                || !webbed(attempt.url().scheme())
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .ok()?;
    let mut answer = client.get(address).send().await.ok()?;
    let kind = answer.headers().get(CONTENT_TYPE)?.to_str().ok()?;
    if !answer.status().is_success()
        || !pictured(kind)
        || answer.content_length().is_some_and(|all| all > most)
    {
        return None;
    }
    let mut bytes = Vec::<u8>::new();
    while let Some(chunk) = answer.chunk().await.ok()? {
        if (bytes.len() + chunk.len()) as u64 > most {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

/// The picture at `address`, within `bounds`.
pub(crate) async fn get(
    address: String,
    bounds: Bounds,
) -> Option<Pixels> {
    let bytes = bytes(&address, bounds.bytes).await?;
    tokio::task::spawn_blocking(move || decode(&bytes, bounds.pixels))
        .await
        .ok()?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_fetched_from_the_web_alone() {
        let host = |address: &str| host(address);
        assert_eq!(
            host("https://orchard.example/pear.png?row=3"),
            Some("orchard.example".to_string())
        );
        // The host is the one after the name a fetch would sign in
        // with.
        assert_eq!(
            host("http://orchard.example@other.example:8080/p.png"),
            Some("other.example:8080".to_string())
        );
        assert_eq!(host("mailto:chief@hamlet.gg"), None);
        assert_eq!(host("file:///etc/passwd"), None);
        assert_eq!(host("https://"), None);
        assert!(pictured("image/png"));
        assert!(pictured(" Image/WebP; q=1"));
        assert!(!pictured("text/html; charset=utf-8"));
    }
}

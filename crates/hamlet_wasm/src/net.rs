//! A plugin's fetches over https. A fetch needs the grant `net`, goes
//! only to a host the ask lists, follows no redirect and is held to
//! limits of bytes, time, requests waiting and requests a minute.

use std::time::Duration;

use bevy::prelude::*;
use hamlet_core::core::Tokio;
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::plugin::PluginInfo;
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    FetchAnswer, FetchRequest, FetchResponse, HttpMethod, Incoming,
    Intent,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;

/// The port of an https address that names none.
const HTTPS: u16 = 443;

/// The characters of an address that a message repeats.
const CUT: usize = 60;

/// A fetch that finished: the plugin, its number for the request and
/// the answer.
type Finished = (Entity, u32, Result<FetchResponse, String>);

/// The fetches under way, and the means to make them.
#[derive(Resource)]
pub struct Fetching {
    client: Option<Client>,
    tx: UnboundedSender<Finished>,
    rx: UnboundedReceiver<Finished>,
    /// The plugin of each fetch under way.
    waiting: Vec<Entity>,
    rate: Rate,
}

impl Default for Fetching {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            client: client(),
            tx,
            rx,
            waiting: Vec::new(),
            rate: Rate::default(),
        }
    }
}

impl Fetching {
    /// Whether any fetch is under way.
    pub fn any(fetching: Res<Fetching>) -> bool {
        !fetching.waiting.is_empty()
    }
}

/// The client a fetch is made with: it follows no redirect, which
/// could lead off the listed hosts.
fn client() -> Option<Client> {
    Client::builder()
        .redirect(Policy::none())
        .referer(false)
        .build()
        .ok()
}

/// A request that may be sent.
pub struct Sent {
    pub method: HttpMethod,
    pub url: Url,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// The address `address`, if it is an https address on a host of
/// `hosts`: a host such as `api.example.com`, or `host:port` for a
/// port other than 443. An address with a user is not one.
pub fn allowed(
    address: &str,
    hosts: &[String],
) -> Result<Url, String> {
    let shown = address.chars().take(CUT).collect::<String>();
    let url = Url::parse(address)
        .map_err(|_| format!("`{shown}` is not an address"))?;
    if url.scheme() != "https" {
        return Err(format!("`{shown}` is not an https address"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("`{shown}` names a user"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| format!("`{shown}` has no host"))?
        .to_ascii_lowercase();
    let port = url.port_or_known_default().unwrap_or(HTTPS);
    let listed = hosts.iter().any(|entry| {
        let entry = entry.to_ascii_lowercase();
        match entry.rsplit_once(':') {
            Some((name, number)) if number.parse::<u16>().is_ok() => {
                name == host && number.parse::<u16>() == Ok(port)
            }
            _ => entry == host && port == HTTPS,
        }
    });
    if hosts.is_empty() {
        return Err("the ask lists no hosts".into());
    }
    if !listed {
        return Err(format!("{host} is not a host the ask lists"));
    }
    Ok(url)
}

/// Sends `sent` and reads the answer, whatever its status. The whole
/// takes no longer than `patience`, and an answer over `most` bytes
/// is an error.
pub async fn transfer(
    client: &Client,
    sent: &Sent,
    patience: Duration,
    most: usize,
) -> Result<FetchResponse, String> {
    let mut request = match sent.method {
        HttpMethod::Get => client.get(sent.url.clone()),
        HttpMethod::Post => {
            let post = client.post(sent.url.clone());
            let post = match &sent.content_type {
                Some(kind) => post.header(CONTENT_TYPE, kind),
                None => post,
            };
            post.body(sent.body.clone())
        }
    };
    request = request.timeout(patience);
    let failed = |why: reqwest::Error| match why.is_timeout() {
        true => "the request took too long".to_string(),
        false => format!("the request failed: {}", why.without_url()),
    };
    let mut answer = request.send().await.map_err(failed)?;
    let over = || format!("the answer has more than {most} bytes");
    if answer.content_length().is_some_and(|all| all > most as u64) {
        return Err(over());
    }
    let status = answer.status().as_u16();
    let content_type = answer
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .map(str::to_string)
        .unwrap_or_default();
    let mut body = Vec::new();
    while let Some(chunk) = answer.chunk().await.map_err(failed)? {
        if body.len() + chunk.len() > most {
            return Err(over());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(FetchResponse {
        status,
        content_type,
        body,
    })
}

/// Why a fetch is not made, and whether the chief is to be asked.
struct Refused {
    why: String,
    ask: bool,
}

impl Refused {
    fn because(why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            ask: false,
        }
    }
}

/// Starts the fetches the plugins that were stepped asked for. One
/// that is not made is answered at once with why.
pub fn start(
    mut done: MessageReader<Done>,
    mut plugins: Query<(&PluginInfo, &Granted, &mut Inbox)>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    tokio: Option<Res<Tokio>>,
    mut fetching: ResMut<Fetching>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        let Ok((info, granted, mut inbox)) = plugins.get_mut(*plugin)
        else {
            continue;
        };
        for request in
            intents.iter().filter_map(|intent| match intent {
                Intent::Fetch(request) => Some(request),
                _ => None,
            })
        {
            let id = request.id;
            let made = begin(
                request,
                granted,
                &limits,
                &mut fetching,
                *plugin,
                now,
            );
            let sent =
                match (made, tokio.as_deref(), &fetching.client) {
                    (Ok(sent), Some(tokio), Some(client)) => {
                        let tx = fetching.tx.clone();
                        let client = client.clone();
                        let entity = *plugin;
                        let patience =
                            Duration::from_millis(limits.fetch_ms);
                        let most = limits.fetch_response_bytes;
                        info!(
                            "plugin {}: fetches {}",
                            info.id,
                            sent.url.host_str().unwrap_or_default()
                        );
                        tokio.0.spawn(async move {
                            let out = transfer(
                                &client, &sent, patience, most,
                            )
                            .await;
                            let _ = tx.send((entity, id, out));
                        });
                        fetching.waiting.push(*plugin);
                        continue;
                    }
                    (Ok(_), ..) => {
                        Refused::because("there is no network")
                    }
                    (Err(refused), ..) => refused,
                };
            warn!("plugin {}: fetch refused: {}", info.id, sent.why);
            if sent.ask {
                commands.trigger(Wanted {
                    plugin: *plugin,
                    capability: Capability::Net,
                    target: None,
                    reason: format!("fetch {}", cut(&request.url)),
                });
            }
            inbox.push(Incoming::Fetched(FetchAnswer {
                id,
                outcome: Err(sent.why),
            }));
        }
    }
}

/// The request `request` as it may be sent, or why not.
fn begin(
    request: &FetchRequest,
    granted: &Granted,
    limits: &Limits,
    fetching: &mut Fetching,
    plugin: Entity,
    now: f32,
) -> Result<Sent, Refused> {
    let verdict = granted.verdict(Capability::Net);
    if !matches!(verdict, Some(Verdict::Allowed | Verdict::Ask)) {
        return Err(Refused::because("net not granted"));
    }
    let url = allowed(&request.url, granted.scope(Capability::Net))
        .map_err(Refused::because)?;
    if verdict == Some(Verdict::Ask) {
        return Err(Refused {
            why: "net is yet to be decided".into(),
            ask: true,
        });
    }
    if request.body.len() > limits.fetch_request_bytes {
        return Err(Refused::because(format!(
            "the request has {} bytes, the most is {}",
            request.body.len(),
            limits.fetch_request_bytes
        )));
    }
    let waiting = fetching
        .waiting
        .iter()
        .filter(|held| **held == plugin)
        .count();
    if waiting >= limits.fetches_in_flight {
        return Err(Refused::because(format!(
            "{waiting} requests are waiting, the most is {}",
            limits.fetches_in_flight
        )));
    }
    if !fetching.rate.admit(plugin, now, limits.fetches_per_minute) {
        return Err(Refused::because(format!(
            "over {} fetches a minute",
            limits.fetches_per_minute
        )));
    }
    Ok(Sent {
        method: request.method,
        url,
        content_type: (!request.content_type.is_empty())
            .then(|| request.content_type.clone()),
        body: request.body.clone(),
    })
}

fn cut(address: &str) -> String {
    address.chars().take(CUT).collect()
}

/// Gives each plugin the answers to its fetches that came in.
pub fn finish(
    mut fetching: ResMut<Fetching>,
    mut inboxes: Query<&mut Inbox>,
) {
    while let Ok((plugin, id, outcome)) = fetching.rx.try_recv() {
        if let Some(at) =
            fetching.waiting.iter().position(|held| *held == plugin)
        {
            fetching.waiting.swap_remove(at);
        }
        if let Ok(mut inbox) = inboxes.get_mut(plugin) {
            inbox
                .push(Incoming::Fetched(FetchAnswer { id, outcome }));
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn hosts(list: &[&str]) -> Vec<String> {
        list.iter().map(|host| host.to_string()).collect()
    }

    #[test]
    fn only_https_addresses_on_the_listed_hosts_are_allowed() {
        let listed =
            hosts(&["api.example.com", "Other.example:8443"]);
        let ok = |address| allowed(address, &listed).is_ok();
        assert!(ok("https://api.example.com/v1?a=b"));
        assert!(ok("https://API.example.com:443/x"));
        assert!(ok("https://other.example:8443/x"));
        assert!(!ok("http://api.example.com/"));
        assert!(!ok("ftp://api.example.com/"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("https://example.com/"));
        assert!(!ok("https://evil.api.example.com/"));
        assert!(!ok("https://api.example.com.evil.com/"));
        assert!(!ok("https://api.example.com@evil.com/"));
        assert!(!ok("https://user:pw@api.example.com/"));
        assert!(!ok("https://api.example.com:8443/"));
        assert!(!ok("https://other.example/"));
        assert!(!ok("https://127.0.0.1/"));
        assert!(!ok("not an address"));
        assert!(allowed("https://api.example.com/", &[]).is_err());
    }

    /// A server on the loopback that answers one request with
    /// `answer`, after `pause`, and the request it was sent.
    async fn serving(
        answer: &'static [u8],
        pause: Duration,
    ) -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
        let listener =
            TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut got = Vec::new();
            let mut chunk = [0; 1024];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&chunk[..n]);
            }
            tokio::time::sleep(pause).await;
            let _ = stream.write_all(answer).await;
            let _ = stream.shutdown().await;
            got
        });
        (port, served)
    }

    fn sent(port: u16, method: HttpMethod) -> Sent {
        Sent {
            method,
            url: Url::parse(&format!(
                "http://127.0.0.1:{port}/p?q=1"
            ))
            .unwrap(),
            content_type: Some("text/plain".into()),
            body: b"hello".to_vec(),
        }
    }

    const PATIENCE: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn a_fetch_returns_the_status_type_and_body_of_the_answer()
    {
        let client = client().unwrap();
        let (port, served) = serving(
            b"HTTP/1.1 201 Created\r\nContent-Type: text/x\r\n\
              Content-Length: 2\r\n\r\nok",
            Duration::ZERO,
        )
        .await;
        let got = transfer(
            &client,
            &sent(port, HttpMethod::Post),
            PATIENCE,
            100,
        )
        .await
        .unwrap();
        assert_eq!(got.status, 201);
        assert_eq!(got.content_type, "text/x");
        assert_eq!(got.body, b"ok");
        let request =
            String::from_utf8(served.await.unwrap()).unwrap();
        assert!(
            request.starts_with("POST /p?q=1 HTTP/1.1"),
            "{request}"
        );
        assert!(request.contains("content-type: text/plain"));
    }

    #[tokio::test]
    async fn an_answer_over_the_cap_or_over_the_time_is_an_error() {
        let client = client().unwrap();
        let (port, _) = serving(
            b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabcdef",
            Duration::ZERO,
        )
        .await;
        let big = transfer(
            &client,
            &sent(port, HttpMethod::Get),
            PATIENCE,
            5,
        )
        .await;
        assert!(big.unwrap_err().contains("more than 5 bytes"));

        let (port, _) = serving(
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
            Duration::from_secs(5),
        )
        .await;
        let slow = transfer(
            &client,
            &sent(port, HttpMethod::Get),
            Duration::from_millis(100),
            5,
        )
        .await;
        assert_eq!(slow.unwrap_err(), "the request took too long");
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let client = client().unwrap();
        let (port, _) = serving(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\n\
              Content-Length: 0\r\n\r\n",
            Duration::ZERO,
        )
        .await;
        let got = transfer(
            &client,
            &sent(port, HttpMethod::Get),
            PATIENCE,
            5,
        )
        .await
        .unwrap();
        assert_eq!(got.status, 302);
    }
}

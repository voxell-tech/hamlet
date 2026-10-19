//! A plugin's fetches over https. A fetch needs the grant `net`, goes
//! only to a host the ask lists, follows no redirect and is held to
//! limits of bytes, time, requests waiting and requests a minute.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use bevy::prelude::*;
use hamlet_base::Tokio;
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
/// could lead off the listed hosts. Where `pin` names a host and the
/// addresses it was checked to have, the client connects to those
/// and to no other, whatever a later lookup would answer.
fn client(pin: Option<(&str, &[SocketAddr])>) -> Option<Client> {
    let builder =
        Client::builder().redirect(Policy::none()).referer(false);
    match pin {
        Some((host, addresses)) => {
            builder.resolve_to_addrs(host, addresses)
        }
        None => builder,
    }
    .build()
    .ok()
}

/// Whether `address` is one a plugin may reach by a name it was not
/// given the address of: not loopback, private, link-local,
/// unique-local, shared, multicast or unspecified.
fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => public(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    || first & 0xfe00 == 0xfc00
                    || first & 0xffc0 == 0xfe80)
            }
        },
    }
}

/// The addresses of `found` a connection may go to: the public ones,
/// and those that are an address the ask lists in `hosts`.
fn screen(
    found: Vec<SocketAddr>,
    hosts: &[String],
) -> Vec<SocketAddr> {
    let listed = hosts
        .iter()
        .filter_map(|entry| literal(entry))
        .collect::<Vec<_>>();
    found
        .into_iter()
        .filter(|found| {
            public(found.ip()) || listed.contains(&found.ip())
        })
        .collect()
}

/// The address a scope entry such as `127.0.0.1`, `10.0.0.5:8443`
/// or `[::1]:8443` names, if it names one.
fn literal(entry: &str) -> Option<IpAddr> {
    entry
        .parse::<IpAddr>()
        .ok()
        .or_else(|| entry.parse::<SocketAddr>().ok().map(|a| a.ip()))
        .or_else(|| {
            entry
                .strip_prefix('[')?
                .strip_suffix(']')?
                .parse::<IpAddr>()
                .ok()
        })
}

/// The client to send `sent` with. A host that is a name is looked
/// up here, and the client connects only to the addresses of the
/// answer that `screen` lets through; an address or `localhost` is
/// there only because the ask lists it, and is connected to as it is.
async fn prepared(
    sent: &Sent,
    hosts: &[String],
    patience: Duration,
) -> Result<Client, String> {
    let unreachable = || "the client cannot be made".to_string();
    let Some(name) = sent.url.domain() else {
        return client(None).ok_or_else(unreachable);
    };
    if name.eq_ignore_ascii_case("localhost") {
        return client(None).ok_or_else(unreachable);
    }
    let port = sent.url.port_or_known_default().unwrap_or(HTTPS);
    let found = tokio::time::timeout(
        patience,
        tokio::net::lookup_host((name, port)),
    )
    .await
    .map_err(|_| "the lookup took too long".to_string())?
    .map_err(|why| format!("the lookup failed: {why}"))?;
    let reach = screen(found.collect(), hosts);
    if reach.is_empty() {
        return Err(format!(
            "{name} is not at an address a plugin may reach"
        ));
    }
    client(Some((name, &reach))).ok_or_else(unreachable)
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
            let sent = match (made, tokio.as_deref()) {
                (Ok(sent), Some(tokio)) => {
                    let tx = fetching.tx.clone();
                    let hosts =
                        granted.scope(Capability::Net).to_vec();
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
                        let out =
                            match prepared(&sent, &hosts, patience)
                                .await
                            {
                                Ok(client) => {
                                    transfer(
                                        &client, &sent, patience,
                                        most,
                                    )
                                    .await
                                }
                                Err(why) => Err(why),
                            };
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

    #[test]
    fn an_address_is_reached_only_if_listed_or_public() {
        let found = |list: &[&str]| {
            let all = [
                "10.0.0.5",
                "127.0.0.1",
                "169.254.1.1",
                "8.8.8.8",
                "::1",
                "fd00::1",
                "fe80::1",
                "::ffff:10.0.0.5",
                "::",
                "100.64.0.1",
                "2606:4700::1",
            ]
            .map(|ip| SocketAddr::new(ip.parse().unwrap(), 443));
            screen(all.to_vec(), &hosts(list))
                .iter()
                .map(|found| found.ip().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            found(&["api.example.com"]),
            ["8.8.8.8", "2606:4700::1"]
        );
        assert_eq!(
            found(&["10.0.0.5:8443", "[::1]"]),
            ["10.0.0.5", "8.8.8.8", "::1", "2606:4700::1"]
        );
    }

    #[test]
    fn a_literal_address_or_localhost_is_allowed_only_where_listed() {
        let ok = |address, list: &[&str]| {
            allowed(address, &hosts(list)).is_ok()
        };
        assert!(!ok("https://127.0.0.1/", &["api.example.com"]));
        assert!(!ok("https://localhost/", &["api.example.com"]));
        assert!(!ok("https://[::1]/", &["api.example.com"]));
        assert!(ok("https://127.0.0.1/", &["127.0.0.1"]));
        assert!(ok("https://localhost/", &["localhost"]));
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
        let client = client(None).unwrap();
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
        let client = client(None).unwrap();
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
        let client = client(None).unwrap();
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

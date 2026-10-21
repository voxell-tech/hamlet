//! Where a fetch may go: an `https` address on a host the plugin
//! asked for, resolved here and pinned to the public addresses of
//! the answer, so a later lookup cannot send it elsewhere.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use hamlet_engine_api::{Capability, Failure};
use hamlet_engine_plugins::PluginInfo;
use reqwest::Url;

/// The port of an `https` address that names none.
const HTTPS: u16 = 443;

/// The characters of an address that a refusal repeats.
const CUT: usize = 60;

/// The network's methods on [`PluginInfo`].
pub(crate) trait NetPluginInfoExt {
    /// The address `url`, and the target of the plugin's ask it is
    /// on: an `https` address with no user, on a host such as
    /// `api.example.org`, or `host:port` for a port other than 443.
    fn listed(&self, url: &str) -> Result<(Url, String), Failure>;
}

impl NetPluginInfoExt for PluginInfo {
    fn listed(&self, url: &str) -> Result<(Url, String), Failure> {
        let shown = url.chars().take(CUT).collect::<String>();
        let refused = |why: String| Failure::Refused(why);
        let parsed = Url::parse(url).map_err(|_| {
            refused(format!("{shown:?} is not an address"))
        })?;
        if parsed.scheme() != "https" {
            return Err(refused(format!("{shown:?} is not https")));
        }
        if !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(refused(format!("{shown:?} names a user")));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| refused(format!("{shown:?} has no host")))?
            .to_ascii_lowercase();
        let port = parsed.port_or_known_default().unwrap_or(HTTPS);
        let target = self
            .asks
            .iter()
            .filter(|ask| ask.capability == Capability::NetFetch)
            .map(|ask| &ask.target)
            .find(|target| {
                let target = target.to_ascii_lowercase();
                match target.rsplit_once(':') {
                    Some((name, number))
                        if number.parse::<u16>().is_ok() =>
                    {
                        name == host
                            && number.parse::<u16>() == Ok(port)
                    }
                    _ => target == host && port == HTTPS,
                }
            })
            .ok_or_else(|| {
                refused(format!(
                    "{host} is not a host {} asked for",
                    self.id
                ))
            })?;
        Ok((parsed, target.clone()))
    }
}

/// Whether `address` may be reached by a plugin: not loopback,
/// private, link-local, unique-local, shared, multicast, broadcast,
/// reserved or unspecified, nor an IPv6 address that carries one of
/// those IPv4 addresses.
pub fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a, b, c) == (192, 0, 0)
                || (a == 198 && (18..20).contains(&b)))
        }
        IpAddr::V6(v6) => {
            let [first, second, ..] = v6.segments();
            let octets = v6.octets();
            let last = |at: usize| {
                IpAddr::from([
                    octets[at],
                    octets[at + 1],
                    octets[at + 2],
                    octets[at + 3],
                ])
            };
            // Mapped, compatible, NAT64 and 6to4 addresses reach
            // the IPv4 address they carry.
            let carried = if v6.to_ipv4_mapped().is_some()
                || octets[..12].iter().all(|byte| *byte == 0)
                || (first, second) == (0x64, 0xff9b)
            {
                Some(last(12))
            } else if first == 0x2002 {
                Some(last(2))
            } else {
                None
            };
            match carried {
                Some(v4) => public(v4),
                None => {
                    !(v6.is_loopback()
                        || v6.is_unspecified()
                        || v6.is_multicast()
                        || first & 0xfe00 == 0xfc00
                        || first & 0xffc0 == 0xfe80
                        || first & 0xffc0 == 0xfec0)
                }
            }
        }
    }
}

/// The public addresses `host` is at, resolved within `patience`.
/// A host that is an address is that one, if it is public.
pub async fn reach(
    host: &str,
    port: u16,
    patience: Duration,
) -> Result<Vec<SocketAddr>, Failure> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let found = match bare.parse::<IpAddr>() {
        Ok(address) => vec![SocketAddr::new(address, port)],
        Err(_) => tokio::time::timeout(
            patience,
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| {
            Failure::Failed("the lookup took too long".into())
        })?
        .map_err(|why| {
            Failure::Failed(format!("the lookup failed: {why}"))
        })?
        .collect(),
    };
    let reachable = found
        .into_iter()
        .filter(|found| public(found.ip()))
        .collect::<Vec<_>>();
    if reachable.is_empty() {
        return Err(Failure::Refused(format!(
            "{host} is not at an address a plugin may reach"
        )));
    }
    Ok(reachable)
}

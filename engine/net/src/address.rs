//! Where a fetch may go: an `http` or `https` address on a host the
//! plugin asked for, resolved here and pinned to the addresses of the
//! answer, so a later lookup cannot send it elsewhere.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use hamlet_engine_api::{Capability, Failure};
use hamlet_engine_plugins::{PluginInfo, default_port};
use reqwest::Url;

/// The characters of an address that a refusal repeats.
const CUT: usize = 60;

/// The network's methods on [`PluginInfo`].
pub(crate) trait NetPluginInfoExt {
    /// The address `url`, and the target of the plugin's ask it is
    /// on: an `http` or `https` address with no user, on a host such
    /// as `api.example.org` at the scheme's port, 80 or 443, or
    /// `host:port` at that port.
    fn listed(&self, url: &str) -> Result<(Url, String), Failure>;
}

impl NetPluginInfoExt for PluginInfo {
    fn listed(&self, url: &str) -> Result<(Url, String), Failure> {
        let shown = url.chars().take(CUT).collect::<String>();
        let refused = |why: String| Failure::Refused(why);
        let parsed = Url::parse(url).map_err(|_| {
            refused(format!("{shown:?} is not an address"))
        })?;
        let default =
            default_port(parsed.scheme()).ok_or_else(|| {
                refused(format!("{shown:?} is not http or https"))
            })?;
        if !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(refused(format!("{shown:?} names a user")));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| refused(format!("{shown:?} has no host")))?
            .to_ascii_lowercase();
        let port = parsed.port_or_known_default().unwrap_or(default);
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
                    _ => target == host && port == default,
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

/// Whether `address` may be reached by a plugin: not multicast,
/// broadcast or unspecified (`0.0.0.0/8`, `::`), nor an IPv6 address
/// that maps one of those IPv4 addresses.
pub fn reachable(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            !(v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.octets()[0] == 0)
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => reachable(IpAddr::V4(v4)),
            None => !(v6.is_unspecified() || v6.is_multicast()),
        },
    }
}

/// The addresses `host` is at that a plugin may reach, resolved
/// within `patience`. A host that is an address is that one, if a
/// plugin may reach it.
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
        .filter(|found| reachable(found.ip()))
        .collect::<Vec<_>>();
    if reachable.is_empty() {
        return Err(Failure::Refused(format!(
            "{host} is not at an address a plugin may reach"
        )));
    }
    Ok(reachable)
}

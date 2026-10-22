//! A plugin's environment: the variables its manifest names, and the
//! `net.fetch` asks on one of them.

use hamlet_engine_api::{Ask, Capability, Manifest, variable_of};
use url::Url;

/// The port of an address of `scheme` that names none, for the
/// schemes a plugin may fetch over.
pub fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    }
}

/// The target of a `net.fetch` ask on the host of `address`, an
/// `http` or `https` address naming no user: the host, or
/// `host:port` for a port other than the scheme's.
pub fn fetch_target(address: &str) -> Option<String> {
    let parsed = Url::parse(address).ok()?;
    let default = default_port(parsed.scheme())?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    match parsed.port() {
        Some(port) if port != default => {
            Some(format!("{host}:{port}"))
        }
        _ => Some(host),
    }
}

/// The asks of `manifest`, each `net.fetch` on `$NAME` on the target
/// of the address `NAME` holds in `env`, and the targets so made,
/// each with its variable's name. An ask whose variable holds no
/// such address is left on `$NAME`, which no host is; one asked
/// twice so is kept once.
pub(crate) fn resolve(
    manifest: &Manifest,
    env: &[(String, String)],
) -> (Vec<Ask>, Vec<(String, String)>) {
    let mut asks = Vec::<Ask>::new();
    let mut variables = Vec::new();
    for ask in &manifest.asks {
        let mut ask = ask.clone();
        if let Some(name) = variable_of(&ask.target)
            && ask.capability == Capability::NetFetch
            && let Some(target) = env
                .iter()
                .find(|(named, _)| named == name)
                .and_then(|(_, value)| fetch_target(value))
        {
            variables.push((target.clone(), name.to_owned()));
            ask.target = target;
        }
        if !asks.iter().any(|before| {
            before.capability == ask.capability
                && before.target == ask.target
        }) {
            asks.push(ask);
        }
    }
    (asks, variables)
}

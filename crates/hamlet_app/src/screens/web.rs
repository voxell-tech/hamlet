//! The Web panels: each shows a web page under a bar of its address
//! (ADR 0014). The addresses are kept with the view in every build;
//! the panels are only in a macOS build with the `web` feature.

#[cfg(all(feature = "web", target_os = "macos"))]
pub mod engine;
#[cfg(all(feature = "web", target_os = "macos"))]
mod fetch;
#[cfg(all(feature = "web", target_os = "macos"))]
mod keys;
#[cfg(all(feature = "web", target_os = "macos"))]
mod page;
#[cfg(all(feature = "web", target_os = "macos"))]
mod panel;

use std::collections::BTreeMap;

use bevy::app::App;
use bevy::ecs::resource::Resource;

/// The id of the first Web panel. The others are `web-2`, `web-3`
/// and so on.
pub const PANEL: &str = "web";

pub fn plugin(app: &mut App) {
    app.init_resource::<Addresses>();
    #[cfg(all(feature = "web", target_os = "macos"))]
    {
        app.add_plugins((panel::plugin, fetch::plugin));
        // The engine panics on start without its framework, so it is
        // started only when both it and the helper are there.
        if engine::located().is_some() && page::helper().exists() {
            app.add_plugins((page::plugin, keys::plugin));
        }
    }
}

/// The address each Web panel shows, by panel id, as the view keeps
/// it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Addresses(pub BTreeMap<String, String>);

/// The address typed into a Web panel's bar as a page to load: as it
/// is when it names a scheme, a page on this machine for `localhost`
/// and the like, and a secure site otherwise.
pub fn address_of(typed: &str) -> Option<String> {
    let typed = typed.trim();
    if typed.is_empty() || typed.contains(char::is_whitespace) {
        return None;
    }
    if has_scheme(typed) {
        return Some(typed.to_owned());
    }
    let host = typed
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit_once(':')
        .map_or(typed, |(host, _)| host);
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok();
    Some(if local {
        format!("http://{typed}")
    } else {
        format!("https://{typed}")
    })
}

/// Whether `typed` starts with a URL scheme, such as `https:` or
/// `file:`, rather than a host and its port.
fn has_scheme(typed: &str) -> bool {
    let Some((scheme, rest)) = typed.split_once(':') else {
        return false;
    };
    let named = scheme
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && scheme.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
        });
    // `localhost:5173` is a host and a port, not a scheme.
    let port =
        rest.split(['/', '?', '#']).next().is_some_and(|port| {
            !port.is_empty()
                && port.chars().all(|c| c.is_ascii_digit())
        });
    named && !port
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_address_is_read_as_a_page() {
        for (typed, page) in [
            ("https://example.com", "https://example.com"),
            ("file:///tmp/a.html", "file:///tmp/a.html"),
            ("about:blank", "about:blank"),
            ("example.com", "https://example.com"),
            ("example.com/a?b#c", "https://example.com/a?b#c"),
            ("localhost:5173", "http://localhost:5173"),
            ("localhost:5173/app", "http://localhost:5173/app"),
            ("localhost", "http://localhost"),
            ("app.localhost:3000", "http://app.localhost:3000"),
            ("127.0.0.1:8000", "http://127.0.0.1:8000"),
            ("  example.com  ", "https://example.com"),
        ] {
            assert_eq!(
                address_of(typed).as_deref(),
                Some(page),
                "{typed}"
            );
        }
        assert_eq!(address_of(""), None);
        assert_eq!(address_of("two words"), None);
    }
}

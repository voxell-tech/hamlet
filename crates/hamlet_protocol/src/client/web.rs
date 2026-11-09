//! The agents' web tools: the server asks a window with Web panels to
//! act on an agent's page, and the window answers in words.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::client::{AgentId, RequestId};

/// The longest address a page is asked to load.
pub const WEB_ADDRESS_MOST: usize = 8192;

/// The most seconds a page is waited on.
pub const WEB_WAIT_MOST: u32 = 30;

/// Tells the server this window draws Web panels and is the one the
/// chief used last, so it is the one asked to act on agents' pages.
/// A window sends it on connecting and each time it comes to the
/// front.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct WebWindow {}

/// The server asks a window to act on `agent`'s page, in its own Web
/// tab. The window answers with a [`WebAnswer`] to `id`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct WebAsk {
    pub id: RequestId,
    pub agent: AgentId,
    /// Whether the page may be on a site other than this machine.
    /// Without it, a page that leaves for one is blanked and the act
    /// fails.
    pub net: bool,
    pub act: WebAct,
}

/// What a [`WebAsk`] asks done to an agent's page.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "act", deny_unknown_fields)]
pub enum WebAct {
    /// Loads `address` in the agent's tab, opening the tab first if
    /// it has none, and waits for it to load.
    #[serde(rename = "open")]
    Open {
        #[schemars(length(min = 1, max = WEB_ADDRESS_MOST))]
        address: String,
    },
    /// Reads the page as the browser built it: its title, address,
    /// text and its links, buttons and fields, numbered for the acts
    /// that take one; with `html`, the page's HTML instead of its
    /// text and elements.
    #[serde(rename = "read")]
    Read { html: bool },
    /// Gives what the page wrote to its console, the errors that went
    /// uncaught in it and the loads that failed, since the agent last
    /// asked.
    #[serde(rename = "console")]
    Console {},
    /// Waits up to `seconds` for `text` to show on the page, or for
    /// an element `selector` matches to be on it, or, given
    /// neither, for the page to finish loading.
    #[serde(rename = "wait")]
    Wait {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[schemars(range(min = 1, max = WEB_WAIT_MOST))]
        seconds: u32,
    },
    /// Clicks `target`: the number a read gave an element, or a CSS
    /// selector.
    #[serde(rename = "click")]
    Click { target: String },
    /// Puts `text` in the field `target` names, as `click`'s does, in
    /// place of what it held, then presses Enter when `submit`.
    #[serde(rename = "type")]
    Type {
        target: String,
        text: String,
        submit: bool,
    },
}

impl WebAct {
    /// How long the act may wait on the page, past the window's own
    /// time to answer.
    pub fn waits(&self) -> u32 {
        match self {
            Self::Wait { seconds, .. } => *seconds,
            _ => 0,
        }
    }
}

/// A window's answer to the [`WebAsk`] `to`: words for the agent,
/// and whether the act failed.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct WebAnswer {
    pub to: RequestId,
    pub text: String,
    pub failed: bool,
}

/// A typed address as a page to load: as it
/// is when it names a scheme, a page on this machine for `localhost`
/// and the like, and a secure site otherwise.
pub fn web_address(typed: &str) -> Option<String> {
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

/// Whether the page at `address` is on this machine: a file, a blank
/// page, or a site on `localhost` or a loopback address.
pub fn is_on_this_machine(address: &str) -> bool {
    let Ok(url) = Url::parse(address) else {
        return false;
    };
    match url.scheme() {
        "file" | "about" | "data" => true,
        "http" | "https" | "ws" | "wss" => match url.host() {
            Some(Host::Domain(name)) => {
                name == "localhost" || name.ends_with(".localhost")
            }
            Some(Host::Ipv4(ip)) => {
                ip.is_loopback() || ip.is_unspecified()
            }
            Some(Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        },
        _ => false,
    }
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
                web_address(typed).as_deref(),
                Some(page),
                "{typed}"
            );
        }
        assert_eq!(web_address(""), None);
        assert_eq!(web_address("two words"), None);
    }

    #[test]
    fn only_pages_on_this_machine_are_on_it() {
        for address in [
            "http://localhost:5173/",
            "http://app.localhost/",
            "http://127.0.0.1:8000/a",
            "http://[::1]:3000/",
            "http://0.0.0.0:8080/",
            "file:///tmp/a.html",
            "about:blank",
        ] {
            assert!(is_on_this_machine(address), "{address}");
        }
        for address in [
            "https://example.com/",
            "http://localhost.example.com/",
            "http://192.168.1.4/",
            "javascript:alert(1)",
            "not an address",
        ] {
            assert!(!is_on_this_machine(address), "{address}");
        }
    }
}

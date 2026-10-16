//! Where the bazaar's server is, and the token that signs the chief
//! in to it, kept like the API keys.

use std::path::Path;

use anyhow::Result;

use crate::config::Config;
use crate::config::keys::{load_secret, save_secret};

/// The server asked when nothing else names one.
pub const DEFAULT_SERVER: &str = "http://localhost:8787";

/// The variable that names the server, over the settings.
pub const SERVER_VAR: &str = "HAMLET_BAZAAR_URL";

/// The name the token is kept under.
const TOKEN: &str = "bazaar_token";

/// The bazaar's server: `HAMLET_BAZAAR_URL`, else the settings', else
/// the default.
pub fn server(config: &Config) -> String {
    let set = |url: Option<String>| {
        url.map(|url| url.trim().to_string())
            .filter(|u| !u.is_empty())
    };
    set(std::env::var(SERVER_VAR).ok())
        .or_else(|| set(config.bazaar_url.clone()))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

/// The token of the chief signed in, if one is.
pub fn token(home: &Path) -> Option<String> {
    load_secret(home, TOKEN)
}

/// Keeps `token`, or forgets the one kept.
pub fn keep_token(home: &Path, token: Option<&str>) -> Result<()> {
    save_secret(home, TOKEN, token)
}

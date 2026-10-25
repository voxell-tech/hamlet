//! The sign-in kept for the next run: the server, the session token
//! and who it signs in, as JSON in `bazaar-token` in the window's
//! view folder, readable by its owner alone. A token is used only
//! with the server it came from.

use std::path::{Path, PathBuf};

use bevy::ecs::world::World;
use bevy::log::warn;
use hamlet_client::ViewFolder;
use serde::{Deserialize, Serialize};

use crate::bazaar::market::lot::User;
use crate::bazaar::market::{Account, Market, ask_account};

/// The file in the view folder.
const FILE: &str = "bazaar-token";

/// What the file holds.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub(crate) struct Session {
    pub server: String,
    /// Empty once signed out.
    pub token: String,
    /// Who the token signs in, shown signed in before the server
    /// says, and while it cannot be reached.
    #[serde(default)]
    pub user: Option<User>,
}

fn file(world: &World) -> Option<PathBuf> {
    world
        .get_resource::<ViewFolder>()
        .map(|view| view.0.join(FILE))
}

/// Signs in with the token kept for the server, if one is.
pub(super) fn restore(world: &mut World) {
    let Some(path) = file(world) else {
        return;
    };
    let session = read(&path).unwrap_or_default();
    let mut market = world.resource_mut::<Market>();
    let token = token_for(&session, &market.server.base);
    if token.is_some() && market.server.token.is_none() {
        market.server.token = token;
        if let Some(user) = session.user {
            market.account = Account::In(user);
        }
        ask_account(world);
    }
}

/// Keeps `token` of the server signed in to, and who is signed in,
/// for the next run, or forgets the one kept.
pub(crate) fn keep(world: &mut World, token: Option<&str>) {
    let Some(path) = file(world) else {
        return;
    };
    let market = world.resource::<Market>();
    let session = Session {
        server: market.server.base.clone(),
        token: token.unwrap_or_default().to_string(),
        user: token.and(market.user().cloned()),
    };
    if let Err(why) = write(&path, &session) {
        warn!("the bazaar's sign-in could not be kept: {why}");
    }
}

fn read(path: &Path) -> Option<Session> {
    let text = std::fs::read(path).ok()?;
    serde_json::from_slice(&text).ok()
}

/// Writes `session` beside `path` first, readable by its owner
/// alone, then in its place.
fn write(path: &Path, session: &Session) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let fresh = path.with_extension("new");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let bytes = serde_json::to_vec(session)?;
    std::io::Write::write_all(&mut options.open(&fresh)?, &bytes)?;
    std::fs::rename(fresh, path)
}

/// The token of `session` for the server `base`, if it has one.
pub(crate) fn token_for(
    session: &Session,
    base: &str,
) -> Option<String> {
    (session.server == base && !session.token.is_empty())
        .then(|| session.token.clone())
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_token_is_kept_for_its_server_alone_and_by_its_owner() {
        let scratch = Scratch::new("bazaar-session");
        let path = scratch.join(FILE);
        let kept = Session {
            server: "http://localhost:8787".into(),
            token: "hms_1".into(),
            user: Some(User {
                login: "ada".into(),
                payouts: None,
            }),
        };
        write(&path, &kept).unwrap();
        let read = read(&path).unwrap();
        assert_eq!(read, kept);
        assert_eq!(
            token_for(&read, "http://localhost:8787").as_deref(),
            Some("hms_1")
        );
        assert_eq!(token_for(&read, "https://elsewhere"), None);
        let older =
            br#"{"server":"http://localhost:8787","token":"hms_1"}"#;
        std::fs::write(&path, older).unwrap();
        assert_eq!(super::read(&path).unwrap().user, None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(mode.mode() & 0o777, 0o600);
        }
    }
}

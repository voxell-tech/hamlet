//! Updates looked for while the window is open: shortly after it
//! opens, every hour and when the chief asks from the Hamlet menu,
//! downloaded, and offered in a toast that stays until the chief
//! restarts or closes it, and in the menu (docs/releases.md#updates).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::app::{App, PreUpdate, Startup};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use hamlet_client::Tokio;
use hamlet_protocol::Version;
use hamlet_ui::theme::StatusKind;
use hamlet_update::{Feed, Install};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::PanelSystems;
use crate::toasts::Toasts;

/// The wait after the window opens before the first look, which
/// leaves the start to itself.
const FIRST: Duration = Duration::from_secs(15);

/// The wait between looks.
const EVERY: Duration = Duration::from_secs(60 * 60);

pub fn plugin(app: &mut App) {
    app.init_resource::<Updates>()
        .add_systems(Startup, begin)
        .add_systems(PreUpdate, hear.in_set(PanelSystems::Bind));
}

/// A newer Hamlet.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offer {
    pub version: Version,
    pub ready: Ready,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ready {
    /// Downloaded and checked, at this path.
    File(PathBuf),
    /// The release's page: this copy cannot update itself.
    Page(String),
}

/// What the looks found, for the toasts and the Hamlet menu. It is
/// there only while this copy looks for updates.
#[derive(Resource, Default)]
pub(crate) struct Updates {
    /// Whether this copy looks for updates at all.
    pub looking: bool,
    /// The newest release found, kept after its toast is closed.
    pub offer: Option<Offer>,
    /// The version whose offer the chief closed, not toasted again
    /// while the window is open unless the chief asks.
    pub declined: Option<Version>,
    /// Whether a look the chief asked for is on its way.
    pub checking: bool,
}

/// The way to the looks: what they found, and a look asked for now.
#[derive(Resource)]
struct Looks {
    rx: UnboundedReceiver<Look>,
    asks: UnboundedSender<()>,
}

/// What a look found, and whether the chief asked for it.
struct Look {
    found: Result<Option<Offer>, String>,
    asked: bool,
}

/// Says when the last update did not take, and starts looking.
fn begin(
    mut commands: Commands,
    mut updates: ResMut<Updates>,
    tokio: Option<Res<Tokio>>,
    time: Res<Time>,
    toasts: Option<ResMut<Toasts>>,
) {
    if !hamlet_update::is_enabled() {
        return;
    }
    if let (Some(version), Some(mut toasts)) =
        (hamlet_update::missed(), toasts)
    {
        toasts.note(
            StatusKind::Danger,
            "Hamlet did not update".into(),
            format!(
                "The update to {version} did not install. Hamlet \
                 offers it again."
            ),
            time.elapsed_secs(),
        );
    }
    let (Some(tokio), Some(feed)) = (tokio, Feed::here()) else {
        return;
    };
    let (tx, rx) = unbounded_channel();
    let (asks, asked) = unbounded_channel();
    tokio.0.spawn(watch(feed, Install::here(), tx, asked));
    commands.insert_resource(Looks { rx, asks });
    updates.looking = true;
}

/// Looks for an update after [`FIRST`], then every [`EVERY`], and at
/// once when asked, and sends what each look found.
async fn watch(
    feed: Feed,
    install: Install,
    tx: UnboundedSender<Look>,
    mut asks: UnboundedReceiver<()>,
) {
    let mut wait = FIRST;
    loop {
        let asked = tokio::select! {
            () = tokio::time::sleep(wait) => false,
            ask = asks.recv() => match ask {
                Some(()) => true,
                None => return,
            },
        };
        let found = look(&feed, &install).await.map_err(|error| {
            bevy::log::warn!("Cannot look for updates: {error:#}");
            format!("{error:#}")
        });
        // What was asked during this look is answered by it.
        while asks.try_recv().is_ok() {}
        if tx.send(Look { found, asked }).is_err() {
            return;
        }
        wait = EVERY;
    }
}

/// A newer release, downloaded when this copy can put it in place.
async fn look(
    feed: &Feed,
    install: &Install,
) -> Result<Option<Offer>> {
    let client = hamlet_update::client()?;
    let client = &client;
    let Some(found) = feed
        .check(client, install, hamlet_update::current())
        .await?
    else {
        return Ok(None);
    };
    let ready = match (found.asset, hamlet_update::folder()) {
        (Some(asset), Some(folder)) => Ready::File(
            hamlet_update::download(client, &asset, &folder).await?,
        ),
        _ => Ready::Page(found.page),
    };
    Ok(Some(Offer {
        version: found.version,
        ready,
    }))
}

/// Keeps and toasts what the looks found: an offer unless it was
/// declined, and for a look the chief asked for, also that Hamlet is
/// up to date or why it could not look.
fn hear(
    time: Res<Time>,
    looks: Option<ResMut<Looks>>,
    mut updates: ResMut<Updates>,
    mut toasts: ResMut<Toasts>,
) {
    let Some(mut looks) = looks else {
        return;
    };
    let now = time.elapsed_secs();
    while let Ok(Look { found, asked }) = looks.rx.try_recv() {
        if asked {
            updates.checking = false;
        }
        match found {
            Ok(Some(offer)) => {
                if asked {
                    updates.declined = None;
                }
                if updates.declined.as_ref() != Some(&offer.version) {
                    toasts.offer(offer.clone(), now);
                }
                updates.offer = Some(offer);
            }
            Ok(None) if asked => toasts.note(
                StatusKind::Success,
                "Hamlet is up to date".into(),
                format!(
                    "{} is the newest Hamlet.",
                    hamlet_update::current()
                ),
                now,
            ),
            Err(error) if asked => toasts.note(
                StatusKind::Danger,
                "Hamlet could not look for updates".into(),
                error,
                now,
            ),
            Ok(None) | Err(_) => {}
        }
    }
}

/// Looks for an update now, for the Hamlet menu.
pub(crate) fn check(world: &mut World) {
    let Some(looks) = world.get_resource::<Looks>() else {
        return;
    };
    if looks.asks.send(()).is_ok()
        && let Some(mut updates) = world.get_resource_mut::<Updates>()
    {
        updates.checking = true;
    }
}

/// Takes `offer`: restarts into its file, or opens its page.
pub(crate) fn take(world: &mut World, offer: &Offer) {
    match &offer.ready {
        Ready::File(file) => {
            restart(world, offer.version, file.clone());
        }
        Ready::Page(page) => {
            if hamlet_desktop::open::browse(page)
                == hamlet_desktop::open::Launch::Failed
            {
                crate::toasts::unopened(world, page);
            }
        }
    }
}

/// Takes the newest release found, for the Hamlet menu.
pub(crate) fn update(world: &mut World) {
    let offer = world
        .get_resource::<Updates>()
        .and_then(|updates| updates.offer.clone());
    if let Some(offer) = offer {
        take(world, &offer);
    }
}

/// Closes the offer of `version` for as long as the window is open.
pub(crate) fn decline(world: &mut World, version: Version) {
    if let Some(mut updates) = world.get_resource_mut::<Updates>() {
        updates.declined = Some(version);
    }
}

/// The update the window hands over as it closes.
#[derive(Resource)]
struct Restarting {
    version: Version,
    file: PathBuf,
}

/// Closes the window and, once its editor saves are confirmed or the
/// chief closes with copies, hands the update in `file` to the
/// launcher beside it, which stops the server, puts it in place and
/// opens Hamlet again.
pub(crate) fn restart(
    world: &mut World,
    version: Version,
    file: PathBuf,
) {
    world.insert_resource(Restarting { version, file });
    crate::saving::close_then(world, Some(hand));
}

/// Hands the update over, or keeps the window open with the reason
/// it could not.
fn hand(world: &mut World) -> bool {
    let Some(Restarting { version, file }) =
        world.remove_resource::<Restarting>()
    else {
        return true;
    };
    let Err(error) = hand_over(version, &file) else {
        return true;
    };
    let now = world.resource::<Time>().elapsed_secs();
    world.resource_mut::<Toasts>().note(
        StatusKind::Danger,
        "Hamlet could not restart".into(),
        format!("{error:#}"),
        now,
    );
    false
}

/// Notes the attempt and starts `hamlet_launch --update file`.
fn hand_over(version: Version, file: &Path) -> Result<()> {
    let launcher = std::env::current_exe()?.with_file_name(format!(
        "hamlet_launch{}",
        std::env::consts::EXE_SUFFIX
    ));
    let mut command = Command::new(&launcher);
    command
        .arg("--update")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Its own group, so it outlives a terminal the window was opened
    // from.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(
        &mut command,
        0,
    );
    hamlet_update::attempting(version)?;
    command.spawn().with_context(|| {
        format!("Cannot run {}", launcher.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{all_words, panel_app};

    /// The window with toasts and a way to the looks, and the end of
    /// it the looks would send on.
    fn looking() -> (App, UnboundedSender<Look>) {
        let mut app = panel_app();
        app.add_plugins((crate::toasts::plugin, plugin));
        let (tx, rx) = unbounded_channel();
        let (asks, _) = unbounded_channel();
        app.insert_resource(Looks { rx, asks });
        app.world_mut().resource_mut::<Updates>().looking = true;
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            crate::toasts::stack(),
        );
        app.update();
        (app, tx)
    }

    fn offer() -> Offer {
        Offer {
            version: Version::new(0, 2, 0),
            ready: Ready::Page("https://example.com".into()),
        }
    }

    #[test]
    fn a_look_the_chief_asked_for_always_answers() {
        let (mut app, tx) = looking();
        app.world_mut().resource_mut::<Updates>().checking = true;
        let send = |found, asked| {
            tx.send(Look { found, asked }).unwrap();
        };
        // The hourly looks say nothing unless they find something.
        send(Ok(None), false);
        send(Err("offline".into()), false);
        app.update();
        assert!(!all_words(&mut app).contains("Hamlet"));
        assert!(app.world().resource::<Updates>().checking);
        send(Ok(None), true);
        app.update();
        assert!(all_words(&mut app).contains("Hamlet is up to date"));
        assert!(!app.world().resource::<Updates>().checking);
        send(Err("offline".into()), true);
        app.update();
        assert!(
            all_words(&mut app)
                .contains("Hamlet could not look for updates")
        );
    }

    #[test]
    fn asking_again_offers_an_update_whose_toast_was_closed() {
        let (mut app, tx) = looking();
        decline(app.world_mut(), offer().version);
        tx.send(Look {
            found: Ok(Some(offer())),
            asked: false,
        })
        .unwrap();
        app.update();
        // Kept for the menu, but not toasted again.
        assert_eq!(
            app.world().resource::<Updates>().offer,
            Some(offer())
        );
        assert!(!all_words(&mut app).contains("Hamlet 0.2.0 is out"));
        tx.send(Look {
            found: Ok(Some(offer())),
            asked: true,
        })
        .unwrap();
        app.update();
        app.update();
        assert!(all_words(&mut app).contains("Hamlet 0.2.0 is out"));
    }
}

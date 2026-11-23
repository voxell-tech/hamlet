//! Updates looked for while the window is open: shortly after it
//! opens, every hour, when the chief asks from the Hamlet menu and
//! when the chief picks another channel, downloaded, and offered in a
//! toast that stays until the chief restarts or closes it, and in the
//! menu (docs/releases.md#updates).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::app::{App, PreUpdate, Startup};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use hamlet_client::Tokio;
use hamlet_core::panels::PanelSystems;
use hamlet_ui::theme::StatusKind;
use hamlet_update::{Channel, Feed, Install, Version};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};
use tokio::sync::watch;

use crate::remember::Remembered;
use crate::toasts::Toasts;

/// The wait after the window opens before the first look, which
/// leaves the start to itself.
const FIRST: Duration = Duration::from_secs(15);

/// The wait between looks.
const EVERY: Duration = Duration::from_secs(60 * 60);

pub fn plugin(app: &mut App) {
    app.init_resource::<Updates>()
        .init_resource::<UpdateChannel>()
        .add_systems(Startup, begin)
        .add_systems(
            PreUpdate,
            (follow.run_if(resource_changed::<UpdateChannel>), hear)
                .chain()
                .in_set(PanelSystems::Bind),
        );
}

/// The channel the chief picked in the settings; none until a pick,
/// while this copy follows the one it was built for.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct UpdateChannel(pub Option<Channel>);

impl UpdateChannel {
    /// The channel followed.
    pub(crate) fn followed(self) -> Channel {
        self.0.unwrap_or_else(Channel::built)
    }
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

/// The way to the looks: what they found, a look asked for now, and
/// the channel they look in.
#[derive(Resource)]
struct Looks {
    rx: UnboundedReceiver<Look>,
    asks: UnboundedSender<()>,
    channel: watch::Sender<Channel>,
}

/// What a look found, the channel it looked in, and whether the
/// chief asked for it.
struct Look {
    found: Result<Option<Offer>, String>,
    channel: Channel,
    asked: bool,
}

/// Puts back the kept channel, says when the last update did not
/// take, and starts looking.
fn begin(
    mut commands: Commands,
    mut updates: ResMut<Updates>,
    mut channel: ResMut<UpdateChannel>,
    remembered: Option<Res<Remembered>>,
    tokio: Option<Res<Tokio>>,
    time: Res<Time>,
    toasts: Option<ResMut<Toasts>>,
) {
    if let Some(remembered) = remembered {
        channel.0 = remembered.update_channel();
    }
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
    let Some(tokio) = tokio else {
        return;
    };
    let (tx, rx) = unbounded_channel();
    let (asks, asked) = unbounded_channel();
    let (picked, followed) = watch::channel(channel.followed());
    tokio
        .0
        .spawn(keep_looking(Install::here(), tx, asked, followed));
    commands.insert_resource(Looks {
        rx,
        asks,
        channel: picked,
    });
    updates.looking = true;
}

/// Looks for an update after [`FIRST`], then every [`EVERY`], and at
/// once when asked or when the channel changes, and sends what each
/// look found.
async fn keep_looking(
    install: Install,
    tx: UnboundedSender<Look>,
    mut asks: UnboundedReceiver<()>,
    mut channel: watch::Receiver<Channel>,
) {
    let mut wait = FIRST;
    loop {
        let mut asked = tokio::select! {
            () = tokio::time::sleep(wait) => false,
            ask = asks.recv() => match ask {
                Some(()) => true,
                None => return,
            },
            changed = channel.changed() => match changed {
                Ok(()) => false,
                Err(_) => return,
            },
        };
        let looked_in = *channel.borrow_and_update();
        let found =
            look(looked_in, &install).await.map_err(|error| {
                bevy::log::warn!(
                    "Cannot look for updates: {error:#}"
                );
                format!("{error:#}")
            });
        // What was asked during this look is answered by it.
        while asks.try_recv().is_ok() {
            asked = true;
        }
        let look = Look {
            found,
            channel: looked_in,
            asked,
        };
        if tx.send(look).is_err() {
            return;
        }
        wait = EVERY;
    }
}

/// A newer release in `channel`, downloaded when this copy can put it
/// in place.
async fn look(
    channel: Channel,
    install: &Install,
) -> Result<Option<Offer>> {
    let feed =
        Feed::of(channel).context("This copy does not update")?;
    let client = hamlet_update::client()?;
    let client = &client;
    let Some(found) = feed
        .check(client, install, &hamlet_update::current())
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
    channel: Res<UpdateChannel>,
    mut updates: ResMut<Updates>,
    mut toasts: ResMut<Toasts>,
) {
    let Some(mut looks) = looks else {
        return;
    };
    let now = time.elapsed_secs();
    while let Ok(Look {
        found,
        channel: looked_in,
        asked,
    }) = looks.rx.try_recv()
    {
        // A look in the channel left behind. What was asked of it is
        // asked again of the channel followed.
        if looked_in != channel.followed() {
            if asked {
                let _ = looks.asks.send(());
            }
            continue;
        }
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

/// Looks in the channel the chief picked, dropping what was found in
/// the one before, its toasts too. A copy newer than the new
/// channel's newest release waits for one newer still.
fn follow(
    channel: Res<UpdateChannel>,
    looks: Option<Res<Looks>>,
    mut updates: ResMut<Updates>,
    mut toasts: ResMut<Toasts>,
) {
    let Some(looks) = looks else {
        return;
    };
    let followed = channel.followed();
    let moved = looks.channel.send_if_modified(|looked_in| {
        let moved = *looked_in != followed;
        *looked_in = followed;
        moved
    });
    if moved {
        updates.offer = None;
        updates.declined = None;
        toasts.withdraw_offers();
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
            restart(world, offer.version.clone(), file.clone());
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
    hamlet_update::attempting(&version)?;
    command.spawn().with_context(|| {
        format!("Cannot run {}", launcher.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_core::testing::all_words;
    use hamlet_hub::testing::panel_app;

    use super::*;

    /// The window with toasts and a way to the looks, the end of it
    /// the looks would send on, and the end the asks arrive at.
    fn looking() -> (App, UnboundedSender<Look>, UnboundedReceiver<()>)
    {
        let mut app = panel_app();
        app.add_plugins((crate::toasts::plugin, plugin));
        let (tx, rx) = unbounded_channel();
        let (asks, asked) = unbounded_channel();
        let (channel, _) = watch::channel(Channel::built());
        app.insert_resource(Looks { rx, asks, channel });
        app.world_mut().resource_mut::<Updates>().looking = true;
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            crate::toasts::stack(),
        );
        app.update();
        (app, tx, asked)
    }

    fn offer() -> Offer {
        Offer {
            version: Version::new(0, 2, 0),
            ready: Ready::Page("https://example.com".into()),
        }
    }

    /// A look in the channel this copy was built for.
    fn look(
        found: Result<Option<Offer>, String>,
        asked: bool,
    ) -> Look {
        Look {
            found,
            channel: Channel::built(),
            asked,
        }
    }

    #[test]
    fn a_look_the_chief_asked_for_always_answers() {
        let (mut app, tx, _asked) = looking();
        app.world_mut().resource_mut::<Updates>().checking = true;
        let send = |found, asked| {
            tx.send(look(found, asked)).unwrap();
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
        let (mut app, tx, _asked) = looking();
        decline(app.world_mut(), offer().version);
        tx.send(look(Ok(Some(offer())), false)).unwrap();
        app.update();
        // Kept for the menu, but not toasted again.
        assert_eq!(
            app.world().resource::<Updates>().offer,
            Some(offer())
        );
        assert!(!all_words(&mut app).contains("Hamlet 0.2.0 is out"));
        tx.send(look(Ok(Some(offer())), true)).unwrap();
        app.update();
        app.update();
        assert!(all_words(&mut app).contains("Hamlet 0.2.0 is out"));
    }

    #[test]
    fn another_channel_drops_what_the_last_one_found() {
        let (mut app, tx, mut asked) = looking();
        tx.send(look(Ok(Some(offer())), false)).unwrap();
        app.update();
        app.update();
        assert!(all_words(&mut app).contains("Hamlet 0.2.0 is out"));
        let other = Channel::ALL
            .into_iter()
            .find(|channel| *channel != Channel::built())
            .unwrap();
        app.world_mut().resource_mut::<UpdateChannel>().0 =
            Some(other);
        app.world_mut().resource_mut::<Updates>().checking = true;
        // A look still on its way from the channel left behind, which
        // the chief's check does not take as its answer, and asks
        // again of the channel followed.
        tx.send(look(Ok(Some(offer())), true)).unwrap();
        app.update();
        app.update();
        let updates = app.world().resource::<Updates>();
        assert_eq!(updates.offer, None);
        assert!(updates.checking);
        assert!(!all_words(&mut app).contains("Hamlet 0.2.0 is out"));
        assert!(asked.try_recv().is_ok());
        assert!(asked.try_recv().is_err());
    }
}

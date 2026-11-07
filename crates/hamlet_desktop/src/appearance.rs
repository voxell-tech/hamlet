//! Whether the computer is light or dark.

use std::time::Duration;

use bevy_app::{App, Update};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_changed;
use bevy_ecs::system::Res;
use bevy_log::info;
use bevy_window::WindowTheme;
#[cfg(not(target_os = "linux"))]
use {
    bevy_ecs::change_detection::DetectChangesMut,
    bevy_ecs::message::MessageReader,
    bevy_ecs::query::With,
    bevy_ecs::system::{Query, ResMut},
    bevy_time::{Real, Time},
    bevy_window::{PrimaryWindow, Window, WindowThemeChanged},
};

/// How long launch waits for the computer to say.
const PATIENCE: Duration = Duration::from_millis(500);

pub(crate) fn plugin(app: &mut App) {
    // An answer main inserted before the app stands.
    app.init_resource::<SystemAppearance>().add_systems(
        Update,
        tell.run_if(resource_changed::<SystemAppearance>),
    );
    #[cfg(target_os = "linux")]
    app.add_systems(bevy_app::Startup, portal::listen)
        .add_systems(Update, portal::hear);
    #[cfg(not(target_os = "linux"))]
    app.add_systems(Update, (hear, give_up));
}

/// What the computer has said.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SystemAppearance {
    /// Asked, no answer yet.
    #[default]
    Asking,
    Says(WindowTheme),
    /// No answer in time, or none to be had.
    Silent,
}

impl SystemAppearance {
    /// Asks the computer once, before the app is built, so the first
    /// frame is right. Asking where only the window can answer.
    pub async fn ask() -> Self {
        #[cfg(target_os = "linux")]
        return tokio::time::timeout(PATIENCE, portal::read())
            .await
            .unwrap_or(Self::Silent);
        #[cfg(not(target_os = "linux"))]
        Self::Asking
    }
}

fn tell(said: Res<SystemAppearance>) {
    match *said {
        SystemAppearance::Asking => {}
        SystemAppearance::Says(theme) => {
            info!("The computer is {theme:?}");
        }
        SystemAppearance::Silent => {
            info!(
                "The computer does not say whether it is light or \
                 dark"
            );
        }
    }
}

/// Takes the window's theme once winit has given it, then each
/// change. Never writes `window_theme`: a set theme stops the change
/// messages.
#[cfg(not(target_os = "linux"))]
fn hear(
    window: Query<&Window, With<PrimaryWindow>>,
    mut changed: MessageReader<WindowThemeChanged>,
    mut said: ResMut<SystemAppearance>,
) {
    if let Some(change) = changed.read().last() {
        said.set_if_neq(SystemAppearance::Says(change.theme));
    } else if !matches!(*said, SystemAppearance::Says(_))
        && let Ok(window) = window.single()
        && let Some(theme) = window.window_theme
    {
        *said = SystemAppearance::Says(theme);
    }
}

/// Asking becomes Silent after `PATIENCE`. A later answer still comes
/// in through `hear`.
#[cfg(not(target_os = "linux"))]
fn give_up(
    time: Res<Time<Real>>,
    mut said: ResMut<SystemAppearance>,
) {
    if *said == SystemAppearance::Asking && time.elapsed() >= PATIENCE
    {
        *said = SystemAppearance::Silent;
    }
}

/// The desktop portal's `org.freedesktop.appearance` `color-scheme`.
#[cfg(target_os = "linux")]
mod portal {
    use bevy_ecs::change_detection::DetectChangesMut;
    use bevy_ecs::resource::Resource;
    use bevy_ecs::system::{Commands, Res, ResMut};
    use bevy_window::WindowTheme;
    use futures::StreamExt;
    use hamlet_client::Tokio;
    use tokio::sync::watch;
    use zbus::{Connection, OwnedVariant, Proxy, Variant};

    use super::SystemAppearance;

    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    /// The latest the portal said, sent by the listening task.
    #[derive(Resource)]
    pub(super) struct Heard(watch::Receiver<SystemAppearance>);

    pub(super) async fn read() -> SystemAppearance {
        match settings().await {
            Ok(settings) => scheme(&settings).await,
            Err(_) => SystemAppearance::Silent,
        }
    }

    /// Spawns the task that hears each change of the scheme.
    pub(super) fn listen(
        tokio: Option<Res<Tokio>>,
        said: Res<SystemAppearance>,
        mut commands: Commands,
    ) {
        let Some(tokio) = tokio else {
            return;
        };
        let (tx, rx) = watch::channel(*said);
        commands.insert_resource(Heard(rx));
        tokio.0.spawn(async move {
            let Ok(settings) = settings().await else {
                return;
            };
            let Ok(mut changes) = settings
                .receive_signal_with_args(
                    "SettingChanged",
                    &[(0, NAMESPACE), (1, KEY)],
                )
                .await
            else {
                return;
            };
            // Read again for a change made before the subscription.
            // Only an answer counts: a failed read keeps main's.
            if let said @ SystemAppearance::Says(_) =
                scheme(&settings).await
            {
                tx.send_replace(said);
            }
            while let Some(change) = changes.next().await {
                let body = change.body();
                let Ok((_, _, value)) =
                    body.deserialize::<(&str, &str, Variant<'_>)>()
                else {
                    continue;
                };
                if tx.send(appearance(&value)).is_err() {
                    return;
                }
            }
        });
    }

    pub(super) fn hear(
        heard: Option<ResMut<Heard>>,
        mut said: ResMut<SystemAppearance>,
    ) {
        if let Some(mut heard) = heard
            && heard.0.has_changed().unwrap_or(false)
        {
            said.set_if_neq(*heard.0.borrow_and_update());
        }
    }

    async fn settings() -> zbus::Result<Proxy<'static>> {
        Proxy::new_owned(
            Connection::session().await?,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )
        .await
    }

    async fn scheme(settings: &Proxy<'_>) -> SystemAppearance {
        let key = (NAMESPACE, KEY);
        let value = match settings
            .call::<_, _, OwnedVariant>("ReadOne", &key)
            .await
        {
            Ok(value) => value,
            // Portals before version 2 have only Read.
            Err(_) => match settings.call("Read", &key).await {
                Ok(value) => value,
                Err(_) => return SystemAppearance::Silent,
            },
        };
        appearance(&value)
    }

    /// 1 is dark and 2 light. 0, no preference, is light, as GNOME's
    /// default style reports it.
    fn appearance(value: &Variant<'_>) -> SystemAppearance {
        match value {
            // Read wraps the value in one more variant.
            Variant::Variant(inner) => appearance(inner),
            Variant::U32(1) => {
                SystemAppearance::Says(WindowTheme::Dark)
            }
            Variant::U32(0 | 2) => {
                SystemAppearance::Says(WindowTheme::Light)
            }
            _ => SystemAppearance::Silent,
        }
    }
}

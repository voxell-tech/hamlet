//! Drawing a panel with the panels' looks in a dock, into a picture.

mod host;

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::percent;
use bevy_fynix::dock::{DockPlugin, dock};
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{FynixPlugin, mount};
use hamlet_assets::Tuning;
use hamlet_offscreen::{Target, plugins};
use hamlet_sdk::theme::{HamletTheme, ThemeChanged, insert_theme};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};
use hamlet_sdk::{AnyView, Cx};

pub use self::host::Host;

/// The size of a picture, in pixels, unless a harness is given
/// another.
pub const SIZE: UVec2 = UVec2::new(560, 760);

/// The updates before the picture is taken, for the fonts to load and
/// the panel to settle, and those it is given to be written.
const SETTLE: u32 = 90;
const WRITE: u32 = 60;

/// The time one update lasts.
const FRAME: Duration = Duration::from_nanos(16_666_667);

#[derive(Resource)]
struct Output(PathBuf);

/// Draws what `host` shows, where the plugin panel it names shows
/// `drawn`, and writes the picture to `path` as a PNG.
pub fn draw(
    host: Host,
    drawn: Drawn,
    size: UVec2,
    path: &Path,
) -> Result<(), String> {
    // Bevy panics where there is no GPU, as in a sandbox.
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        paint(host, drawn, size, path)
    }))
    .unwrap_or_else(|_| {
        Err("the picture could not be drawn: there is no GPU here, \
             or the sandbox keeps it out of reach"
            .into())
    })
}

fn paint(
    host: Host,
    drawn: Drawn,
    size: UVec2,
    path: &Path,
) -> Result<(), String> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let mut app = App::new();
    app.add_plugins(plugins(size).disable::<LogPlugin>());
    Target::insert(&mut app, size, 1.0);
    app.add_plugins((
        FynixPlugin::<HamletTheme>::default(),
        DockPlugin::<HamletTheme>::default(),
        hamlet_sdk::dock::plugin,
        hamlet_sdk::tree::plugin,
        hamlet_ui::tuning::plugin,
        hamlet_ui::tabs::plugin,
        hamlet_ui::platform::plugin,
        hamlet_ui::icons::plugin,
    ))
    .add_message::<ThemeChanged>()
    .insert_resource(Output(path.to_path_buf()))
    .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
    .add_systems(Startup, camera)
    .add_systems(PostStartup, |world: &mut World| {
        mount::<HamletTheme>(world, view());
    })
    .add_systems(Update, capture);
    insert_theme(app.world_mut(), HamletTheme::designed());
    if let Some(id) = host.panel() {
        let mut trees = PluginTrees::default();
        trees.set(id, drawn);
        app.insert_resource(trees);
    }
    host.arrange(&mut app);
    app.run();
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        _ => Err(format!("{} was not written", path.display())),
    }
}

/// The dock, over the background of the panels, with what the host
/// lays over it.
fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let overlay = cx.world.resource::<Host>().overlay();
        cx.build(
            column((dock::<HamletTheme>(), overlay))
                .width(percent(100.0))
                .height(percent(100.0))
                .fill(cx.theme().background())
                .gap(0.0),
        )
    })
}

fn camera(mut commands: Commands, target: Res<Target>) {
    commands.spawn((
        Camera2d,
        target.drawn(),
        bevy::ui::IsDefaultUiCamera,
    ));
}

/// Takes the picture once the panel has settled, and leaves once it
/// is written.
fn capture(
    mut commands: Commands,
    target: Res<Target>,
    out: Res<Output>,
    mut update: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *update += 1;
    if *update == SETTLE {
        target.shoot(&mut commands, out.0.clone());
    }
    if *update > SETTLE + WRITE {
        exit.write(AppExit::Success);
    }
}

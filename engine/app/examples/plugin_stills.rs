//! Stills of the whole engine with its bundled plugins, drawn into an
//! image with no window and saved as PNGs.
//!
//! ```sh
//! cargo run -p hamlet_engine --example plugin_stills -- --out <dir> \
//!     [--name <still>] [--size 1600x1000] [--scale 2] [--frames <n>] \
//!     [--run <plugin.id/command>]... [--open <plugin.id/panel>]... \
//!     [--bundle <dir>] [--day | --night] [--online]
//! ```
//!
//! The home is `<dir>/home`, made empty first, with a model chosen
//! that nothing serves, so Welcome does not open. Fetches are never
//! sent, unless `--online`: then they are sent as in the app, to the
//! hosts the bundled plugins ask for. Once every plugin is on and the
//! scenes stop growing, each `--run` command runs and each `--open`
//! panel comes to the front; `--frames` frames later, and once the
//! scenes are still again, the still is saved as `<dir>/<name>.png`
//! and its path printed.

#[path = "../src/demo.rs"]
mod demo;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{ExitCondition, WindowResolution};
use bevy::winit::WinitPlugin;
use hamlet_engine_api::{CommandId, PanelId};
use hamlet_engine_core::{Bundle, Home, Log, LogLevel};
use hamlet_engine_plugins::{
    Blocked, Enabled, PluginInfo, PluginKind, Starting, Stopped,
};
use hamlet_engine_render::ScenePart;
use hamlet_engine_shell::{Command, Panel, ShellWorldExt};
use hamlet_engine_wasm::{Compiling, Instance, MANIFEST};

const USAGE: &str = "usage: plugin_stills --out <dir> [--name \
                     <still>] [--size 1600x1000] [--scale 2] \
                     [--frames <n>] [--run <plugin.id/command>]... \
                     [--open <plugin.id/panel>]... [--bundle <dir>] \
                     [--day | --night] [--online]";

/// The settings of a home whose model is served nowhere: chosen, so
/// Welcome stays shut, and never reached.
const CONFIG: &str = "model = \"stills::still\"\n\n[[endpoints]]\nname \
                      = \"stills\"\nbase_url = \
                      \"http://127.0.0.1:9/v1\"\n";

/// The time every plugin has to come on.
const START_PATIENCE: Duration = Duration::from_secs(240);
/// The time the scenes have to stop growing, after which the still
/// is taken all the same.
const SETTLE_PATIENCE: Duration = Duration::from_secs(180);
/// The frames with nothing added or taken away that make the scenes
/// still.
const QUIET: u32 = 120;
/// The frames a plugin may stay off, neither compiled nor waiting its
/// turn, before it counts as failed.
const IDLE: u32 = 300;
/// The frames the PNG has to appear in, and those it is given once it
/// is there.
const WRITE: u32 = 600;
const WRITTEN: u32 = 5;

struct Options {
    out: PathBuf,
    name: String,
    size: UVec2,
    scale: f32,
    frames: u32,
    runs: Vec<CommandId>,
    opens: Vec<PanelId>,
    bundle: Option<PathBuf>,
    online: bool,
}

impl Options {
    fn read() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut options = Options {
            out: PathBuf::new(),
            name: "still".into(),
            size: UVec2::new(1600, 1000),
            scale: 1.0,
            frames: 120,
            runs: Vec::new(),
            opens: Vec::new(),
            bundle: None,
            online: false,
        };
        let mut hour = None;
        while let Some(arg) = args.next() {
            if arg == "--day" || arg == "--night" {
                hour = Some(arg[2..].to_owned());
                continue;
            }
            if arg == "--online" {
                options.online = true;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("{arg} needs a value"))?;
            match arg.as_str() {
                "--out" => options.out = value.into(),
                "--name" => options.name = value,
                "--size" => {
                    options.size = value
                        .split_once('x')
                        .and_then(|(w, h)| {
                            Some(UVec2::new(
                                w.parse().ok()?,
                                h.parse().ok()?,
                            ))
                        })
                        .filter(|size| size.min_element() > 0)
                        .ok_or("--size is <width>x<height>")?;
                }
                "--scale" => {
                    options.scale = value
                        .parse()
                        .ok()
                        .filter(|scale: &f32| *scale > 0.0)
                        .ok_or("--scale is a number above 0")?;
                }
                "--frames" => {
                    options.frames = value
                        .parse()
                        .map_err(|_| "--frames is a whole number")?;
                }
                "--run" => options.runs.push(
                    CommandId::try_from(value.as_str()).map_err(
                        |why| format!("--run {value}: {why}"),
                    )?,
                ),
                "--open" => options.opens.push(
                    PanelId::try_from(value.as_str()).map_err(
                        |why| format!("--open {value}: {why}"),
                    )?,
                ),
                "--bundle" => options.bundle = Some(value.into()),
                _ => return Err(format!("{arg} is not known")),
            }
        }
        if options.out.as_os_str().is_empty() {
            return Err("--out is needed".into());
        }
        if let Some(hour) = hour {
            let sky = format!("hamlet.realm/sky_{hour}");
            let sky = CommandId::try_from(sky.as_str())
                .map_err(|why| why.to_string())?;
            options.runs.insert(0, sky);
        }
        Ok(options)
    }
}

fn main() -> AppExit {
    let options = match Options::read() {
        Ok(options) => options,
        Err(why) => {
            eprintln!("{why}\n{USAGE}");
            return AppExit::error();
        }
    };
    match draw(options) {
        Ok(exit) => exit,
        Err(why) => {
            eprintln!("plugin_stills: {why}");
            AppExit::error()
        }
    }
}

/// Builds the engine as the binary does, with no window, and runs it
/// until the still is saved.
fn draw(options: Options) -> Result<AppExit, String> {
    let bundle = options.bundle.clone().map_or_else(
        || Bundle::find(env!("CARGO_MANIFEST_DIR")),
        Bundle::at,
    );
    let plugins = bundled(&bundle)?;
    let home = options.out.join("home");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home)
        .and_then(|()| {
            std::fs::write(home.join("config.toml"), CONFIG)
        })
        .map_err(|why| format!("{}: {why}", home.display()))?;
    let path = options.out.join(format!("{}.png", options.name));
    let _ = std::fs::remove_file(&path);

    let mut app = App::new();
    if !options.online {
        app.insert_resource(hamlet_engine_net::Offline);
    }
    app.insert_resource(Home::at(&home))
        .insert_resource(bundle)
        .add_plugins(
            DefaultPlugins
                // A window entity with no winit behind it.
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: WindowResolution::new(
                            options.size.x,
                            options.size.y,
                        )
                        .with_scale_factor_override(1.0),
                        ..default()
                    }),
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>()
                .add(ScheduleRunnerPlugin::run_loop(Duration::ZERO)),
        )
        .add_plugins((
            hamlet_engine_core::plugin,
            hamlet_engine_shell::plugin,
            hamlet_engine_plugins::plugin,
            hamlet_engine_grants::plugin,
            hamlet_engine_assets::plugin,
            hamlet_engine_bus::plugin,
            hamlet_engine_net::plugin,
            hamlet_engine_agents::plugin,
            hamlet_engine_desktop::plugin,
            hamlet_engine_render::plugin,
            hamlet_engine_wasm::plugin,
            demo::plugin,
        ));
    let pixels = (options.size.as_vec2() * options.scale).as_uvec2();
    let image = Image::new_target_texture(
        pixels.x,
        pixels.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    let image =
        app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let target = RenderTarget::Image(ImageRenderTarget {
        handle: image,
        scale_factor: options.scale,
    });
    app.insert_resource(Still {
        target,
        path,
        bundled: plugins,
        frames: options.frames,
        runs: options.runs,
        opens: options.opens,
        phase: Phase::Starting,
        began: Instant::now(),
        quiet: 0,
        last: None,
        idle: HashMap::new(),
    })
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ))
    .add_systems(Update, (aim, step).chain());
    Ok(app.run())
}

/// How many plugins the bundle holds.
fn bundled(bundle: &Bundle) -> Result<usize, String> {
    let root = bundle.root();
    let none = || {
        format!(
            "there is no bundle at {}: run scripts/build_plugins.sh",
            root.display()
        )
    };
    let count = std::fs::read_dir(root)
        .map_err(|_| none())?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(MANIFEST).is_file())
        .count();
    if count == 0 {
        return Err(none());
    }
    Ok(count)
}

/// The image drawn into, where its still goes, and what is left to
/// do before it is taken.
#[derive(Resource)]
struct Still {
    target: RenderTarget,
    path: PathBuf,
    /// The plugins in the bundle.
    bundled: usize,
    /// The frames between acting and the still.
    frames: u32,
    runs: Vec<CommandId>,
    opens: Vec<PanelId>,
    phase: Phase,
    /// When the phase began.
    began: Instant,
    /// The frames the scenes have been still for.
    quiet: u32,
    last: Option<Shape>,
    /// The frames each plugin has stayed off.
    idle: HashMap<Entity, u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Until every plugin is on.
    Starting,
    /// Until the scenes are still.
    Settling,
    /// The frames left after acting.
    After(u32),
    /// The frames since the still was asked for, and since its file
    /// appeared.
    Writing(u32, u32),
}

/// What the scenes and the interface hold, to tell when they stop
/// growing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    parts: usize,
    nodes: usize,
    meshes: usize,
    materials: usize,
}

/// Points the interface's camera at the image.
fn aim(
    cameras: Query<(Entity, &RenderTarget), With<Camera2d>>,
    still: Res<Still>,
    mut commands: Commands,
) {
    for (camera, target) in &cameras {
        if !matches!(target, RenderTarget::Image(_)) {
            commands.entity(camera).insert(still.target.clone());
        }
    }
}

fn step(world: &mut World) {
    if let Err(why) = advance(world) {
        eprintln!("plugin_stills: {why}");
        world.write_message(AppExit::error());
    }
}

fn advance(world: &mut World) -> Result<(), String> {
    let on = plugins_on(world)?;
    let shape = shape(world);
    let mut still = world.resource_mut::<Still>();
    if still.last == Some(shape) {
        still.quiet += 1;
    } else {
        still.quiet = 0;
        still.last = Some(shape);
    }
    let settled = still.quiet >= QUIET
        || still.began.elapsed() > SETTLE_PATIENCE;
    let mut then = Then::Wait;
    match still.phase {
        Phase::Starting if on => {
            info!("every plugin is on; waiting for the scenes");
            still.next(Phase::Settling);
        }
        Phase::Starting => {
            if still.began.elapsed() > START_PATIENCE {
                return Err(format!(
                    "the plugins did not all come on in {}s",
                    START_PATIENCE.as_secs()
                ));
            }
        }
        Phase::Settling if settled => {
            let frames = still.frames;
            still.next(Phase::After(frames));
            then = Then::Act;
        }
        Phase::Settling => {}
        Phase::After(left) if left > 0 => {
            still.phase = Phase::After(left - 1);
        }
        Phase::After(_) if settled => {
            still.next(Phase::Writing(0, 0));
            then = Then::Shoot;
        }
        Phase::After(_) => {}
        Phase::Writing(waited, written) => {
            let there = std::fs::metadata(&still.path)
                .is_ok_and(|file| file.len() > 0);
            let written = written + u32::from(there);
            if written > WRITTEN {
                println!("{}", still.path.display());
                then = Then::Leave;
            } else if waited > WRITE {
                return Err(format!(
                    "{} was not written",
                    still.path.display()
                ));
            } else {
                still.phase = Phase::Writing(waited + 1, written);
            }
        }
    }
    match then {
        Then::Wait => {}
        Then::Act => act(world)?,
        Then::Shoot => {
            let still = world.resource::<Still>();
            let (target, path) =
                (still.target.clone(), still.path.clone());
            world
                .spawn(Screenshot(target))
                .observe(save_to_disk(path));
        }
        Then::Leave => {
            world.write_message(AppExit::Success);
        }
    }
    Ok(())
}

/// What is done on the world once a frame's phase is settled.
enum Then {
    Wait,
    Act,
    Shoot,
    Leave,
}

impl Still {
    fn next(&mut self, phase: Phase) {
        self.phase = phase;
        self.began = Instant::now();
        self.quiet = 0;
    }
}

/// Runs each command and opens each panel asked for.
fn act(world: &mut World) -> Result<(), String> {
    let (runs, opens) = {
        let still = world.resource::<Still>();
        (still.runs.clone(), still.opens.clone())
    };
    for id in &runs {
        let known = world
            .query::<&Command>()
            .iter(world)
            .any(|command| command.id == *id);
        if !known {
            return Err(format!("there is no command {id}"));
        }
        info!("running {id}");
        world.run_command(id);
    }
    for id in &opens {
        let known = world
            .query::<&Panel>()
            .iter(world)
            .any(|panel| panel.id == *id);
        if !known {
            return Err(format!("there is no panel {id}"));
        }
        info!("opening {id}");
        world.open_panel(id);
    }
    Ok(())
}

/// Whether every plugin is on, the bundle's all read; an error for
/// one that was stopped, is blocked or stays off.
fn plugins_on(world: &mut World) -> Result<bool, String> {
    let plugins = world
        .query::<(
            Entity,
            &PluginInfo,
            (
                Has<Enabled>,
                Has<Instance>,
                Has<Compiling>,
                Has<Starting>,
            ),
            Option<&Stopped>,
            Option<&Blocked>,
        )>()
        .iter(world)
        .map(|(plugin, info, state, stopped, blocked)| {
            (
                plugin,
                info.clone(),
                state,
                stopped.cloned(),
                blocked.cloned(),
            )
        })
        .collect::<Vec<_>>();
    let mut all = true;
    let mut bundled = 0;
    for (plugin, info, state, stopped, blocked) in plugins {
        let id = info.id.as_str();
        if let Some(Stopped(why)) = stopped {
            return Err(format!("{id} was stopped: {why}"));
        }
        if let Some(Blocked(why)) = blocked {
            return Err(format!("{id} is blocked: {why}"));
        }
        let (enabled, instance, compiling, starting) = state;
        bundled += usize::from(info.kind == PluginKind::Bundled);
        let on =
            enabled && (instance || info.kind == PluginKind::BuiltIn);
        let waiting = compiling || starting;
        let mut still = world.resource_mut::<Still>();
        let idle = still.idle.entry(plugin).or_default();
        *idle = if on || waiting { 0 } else { *idle + 1 };
        if *idle > IDLE {
            return Err(format!(
                "{id} did not start{}",
                errors(world, id)
            ));
        }
        all &= on;
    }
    Ok(all && bundled >= world.resource::<Still>().bundled)
}

/// The errors the log has from `source`, each on a line of its own.
fn errors(world: &World, source: &str) -> String {
    world
        .resource::<Log>()
        .lines()
        .filter(|(_, line)| {
            line.level == LogLevel::Error && line.source == source
        })
        .map(|(_, line)| format!("\n  {}", line.text))
        .collect()
}

fn shape(world: &mut World) -> Shape {
    let parts = world
        .query_filtered::<(), With<ScenePart>>()
        .iter(world)
        .count();
    let nodes =
        world.query_filtered::<(), With<Node>>().iter(world).count();
    Shape {
        parts,
        nodes,
        meshes: world.resource::<Assets<Mesh>>().len(),
        materials: world.resource::<Assets<StandardMaterial>>().len(),
    }
}

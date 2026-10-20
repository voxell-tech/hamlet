//! Keeps the machine awake while an agent works, as the setting kept
//! in the home says.

use std::fs;
use std::process::{Child, Command, Stdio};

use bevy::app::{App, Update};
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use hamlet_core::agents::Busy;
use hamlet_engine_core::{Home, write_whole};
use serde::{Deserialize, Serialize};

/// The file in the home the desktop's settings are kept in.
const FILE: &str = "desktop.toml";

pub(crate) fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    app.insert_resource(KeepAwake::open(&home))
        .init_resource::<Holder>()
        .add_systems(Update, hold);
}

/// The machine is kept awake while an agent works.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeepAwake(pub bool);

/// The desktop's settings as the file has them.
#[derive(Serialize, Deserialize)]
struct Saved {
    keep_awake: bool,
}

impl KeepAwake {
    /// As the home keeps it: on when the file is not there or cannot
    /// be read.
    fn open(home: &Home) -> Self {
        let saved = fs::read_to_string(home.path(FILE))
            .ok()
            .and_then(|text| toml::from_str::<Saved>(&text).ok());
        Self(saved.is_none_or(|saved| saved.keep_awake))
    }

    /// Writes it to the home, whole or not at all.
    pub(crate) fn save(self, home: &Home) -> Result<(), String> {
        let text = toml::to_string(&Saved { keep_awake: self.0 })
            .map_err(|why| why.to_string())?;
        write_whole(&home.path(FILE), text)
            .map_err(|why| format!("{FILE} cannot be written: {why}"))
    }
}

/// The helper process that holds the machine awake.
#[derive(Resource, Default)]
struct Holder(Option<Child>);

impl Drop for Holder {
    fn drop(&mut self) {
        release(&mut self.0);
    }
}

fn release(held: &mut Option<Child>) {
    if let Some(mut child) = held.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// The system's own helper, which lets go by itself if the engine
/// dies. None where there is none.
fn helper() -> Option<Command> {
    let me = std::process::id().to_string();
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("caffeinate");
        command.args(["-i", "-s", "-w", &me]);
        command
    } else if cfg!(target_os = "linux") {
        let mut command = Command::new("systemd-inhibit");
        command.args([
            "--what=idle:sleep",
            "--who=hamlet",
            "--why=An agent is working",
            "tail",
            &format!("--pid={me}"),
            "-f",
            "/dev/null",
        ]);
        command
    } else {
        return None;
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Some(command)
}

/// Holds the machine awake while an agent is busy and lets it go
/// after. `missing` is set once the helper could not be started, so
/// it is not tried every frame.
fn hold(
    keep: Res<KeepAwake>,
    busy: Query<(), With<Busy>>,
    mut holder: ResMut<Holder>,
    mut missing: Local<bool>,
) {
    let want = keep.0 && !busy.is_empty();
    if want == holder.0.is_some() || (want && *missing) {
        return;
    }
    if want {
        holder.0 =
            helper().and_then(|mut helper| helper.spawn().ok());
        *missing = holder.0.is_none();
    } else {
        release(&mut holder.0);
    }
}

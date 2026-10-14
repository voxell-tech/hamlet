//! Keeps the machine awake while an agent works.

use std::process::{Child, Command, Stdio};

use bevy_app::{App, Update};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Query, Res, ResMut};

use crate::agents::Busy;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<KeepAwake>()
        .init_resource::<Holder>()
        .add_systems(Update, hold);
}

/// The machine is kept awake while an agent works.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeepAwake(pub bool);

impl Default for KeepAwake {
    fn default() -> Self {
        KeepAwake(true)
    }
}

/// The helper process that holds the machine awake.
#[derive(Resource, Default)]
pub struct Holder(Option<Child>);

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

/// The system's own helper, which lets go by itself if the app dies.
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
/// after.
pub fn hold(
    keep: Res<KeepAwake>,
    busy: Query<(), With<Busy>>,
    mut holder: ResMut<Holder>,
    mut missing: Local<bool>,
) {
    // A test starts no helper.
    let want = keep.0 && !busy.is_empty() && !cfg!(test);
    if want == holder.0.is_some() || (want && *missing) {
        return;
    }
    if want {
        holder.0 =
            helper().and_then(|mut helper| helper.spawn().ok());
        // A helper this machine lacks is not tried every frame.
        *missing = holder.0.is_none();
    } else {
        release(&mut holder.0);
    }
}

//! Keeps the machine awake while an agent works.

use std::process::{Child, Command, Stdio};

use bevy::app::{App, Update};
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use hamlet_client::realm::Busy;

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
#[derive(Resource)]
pub struct Holder {
    child: Option<Child>,
    helper: fn() -> Option<Command>,
}

impl Default for Holder {
    fn default() -> Self {
        Self::with_helper(helper)
    }
}

impl Holder {
    /// A holder using the given helper process.
    pub fn with_helper(helper: fn() -> Option<Command>) -> Self {
        Self {
            child: None,
            helper,
        }
    }

    /// The presence of a helper holding the machine awake.
    pub fn is_held(&self) -> bool {
        self.child.is_some()
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        release(&mut self.child);
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
    if cfg!(test) {
        return None;
    }
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
    let want = keep.0 && !busy.is_empty();
    if want == holder.is_held() || (want && *missing) {
        return;
    }
    if want {
        holder.child = (holder.helper)()
            .and_then(|mut helper| helper.spawn().ok());
        // A helper this machine lacks is not tried every frame.
        *missing = !holder.is_held();
    } else {
        release(&mut holder.child);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use bevy::prelude::*;
    use hamlet_client::realm::Busy;

    use super::{Holder, KeepAwake, plugin};

    fn sleeper() -> Option<std::process::Command> {
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        Some(command)
    }

    #[test]
    fn the_machine_is_held_only_while_a_client_agent_is_busy() {
        let mut app = App::new();
        app.add_plugins(plugin)
            .insert_resource(Holder::with_helper(sleeper));
        app.update();
        assert!(!app.world().resource::<Holder>().is_held());
        let first = app.world_mut().spawn(Busy).id();
        let second = app.world_mut().spawn(Busy).id();
        app.update();
        assert!(app.world().resource::<Holder>().is_held());
        app.world_mut().entity_mut(first).remove::<Busy>();
        app.update();
        assert!(app.world().resource::<Holder>().is_held());
        app.world_mut().entity_mut(second).remove::<Busy>();
        app.update();
        assert!(!app.world().resource::<Holder>().is_held());
        app.world_mut().entity_mut(first).insert(Busy);
        app.update();
        assert!(app.world().resource::<Holder>().is_held());
        app.world_mut().resource_mut::<KeepAwake>().0 = false;
        app.update();
        assert!(!app.world().resource::<Holder>().is_held());
    }
}

//! Compiling a plugin's component off the frame: the compile fills
//! the cache in the home, and the plugin is brought up, or reloaded,
//! from it once it is done.

use std::path::Path;

use bevy::prelude::*;
use hamlet_base::Tokio;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{Manifest, PluginInfo, Status};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::cache;
use crate::engine::Host;
use crate::fault::Fault;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::reload::{Modified, keep};
use crate::start::{self, Setting, Source};
use crate::stop::{HeldState, state_of};

/// A compile that ended: the plugin, what it was for and whether it
/// worked.
type Ended = (Entity, Then, Result<(), Fault>);

/// Brings up, or reloads, each plugin whose component was compiled
/// off the frame.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Compiler>().add_systems(
        Update,
        compiled
            .in_set(crate::PluginSet::Collect)
            .run_if(any_with_component::<Compiling>),
    );
}

/// What a plugin is compiled for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Then {
    /// It is brought up.
    Start,
    /// Its new version is built.
    Reload,
}

/// The compiles under way.
#[derive(Resource)]
pub struct Compiler {
    tx: UnboundedSender<Ended>,
    rx: UnboundedReceiver<Ended>,
}

impl Default for Compiler {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self { tx, rx }
    }
}

/// On a plugin whose component is being compiled off the frame.
#[derive(Component)]
pub struct Compiling;

/// The compiler and the runtime it runs on, where the app has one.
#[derive(Clone, Copy)]
pub struct Background<'a> {
    pub tokio: &'a Tokio,
    pub compiler: &'a Compiler,
}

impl Background<'_> {
    /// Starts compiling `file` for `plugin` off the frame, unless the
    /// cache already holds it. Whether it started.
    pub fn compile(
        &self,
        host: &Host,
        limits: &Limits,
        plugin: Entity,
        file: &Path,
        then: Then,
        commands: &mut Commands,
    ) -> bool {
        if cache::cached(&host.0, file, limits.wasm_bytes) {
            return false;
        }
        let (engine, file) = (host.0.clone(), file.to_path_buf());
        let (cap, tx) = (limits.wasm_bytes, self.compiler.tx.clone());
        self.tokio.0.spawn_blocking(move || {
            let made =
                cache::component(&engine, &file, cap).map(drop);
            let _ = tx.send((plugin, then, made));
        });
        commands.entity(plugin).insert(Compiling);
        true
    }
}

/// Brings up from the cache each plugin whose compile ended, and has
/// each one being reloaded looked at again. One that is no longer
/// starting is left as it is, and one whose compile failed says why.
fn compiled(
    mut compiler: ResMut<Compiler>,
    plugins: Query<(
        &PluginInfo,
        &Manifest,
        &Status,
        &Source,
        &Granted,
        Option<&Instance>,
        Option<&HeldState>,
    )>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    mut modified: MessageWriter<Modified>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    let setting = Setting {
        host: &host,
        limits: &limits,
        agents: &agents,
        papers: &papers,
        background: None,
    };
    while let Ok((plugin, then, made)) = compiler.rx.try_recv() {
        let Ok((
            info,
            manifest,
            status,
            source,
            granted,
            instance,
            held,
        )) = plugins.get(plugin)
        else {
            continue;
        };
        commands.entity(plugin).remove::<Compiling>();
        match (then, made) {
            (Then::Reload, Ok(())) => {
                modified.write(Modified(plugin));
            }
            (Then::Reload, Err(fault)) => {
                warn!("plugin {}: reload failed: {fault}", info.id);
                let folder =
                    source.0.parent().unwrap_or(Path::new(""));
                commands
                    .entity(plugin)
                    .insert(keep::trouble(folder, fault.to_string()));
            }
            (Then::Start, _) if *status != Status::Starting => {}
            (Then::Start, Err(fault)) => start::failed(
                plugin,
                info,
                source,
                &fault,
                &mut commands,
            ),
            (Then::Start, Ok(())) => start::bring_up(
                plugin,
                info,
                &manifest.exports,
                source,
                granted,
                &setting,
                state_of(instance, held),
                &mut commands,
            ),
        }
    }
}

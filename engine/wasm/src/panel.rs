//! Serving WASM plugins: what is in each one's inbox handed to it,
//! frame by frame, and its panels and settings
//! pages marked as its, to be built node by node (`retained.rs`).

use std::collections::VecDeque;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Has, With};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::time::Time;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label};
use hamlet_engine_api::Failure;
use hamlet_engine_bus::{Serve, Served};
use hamlet_engine_core::{ActiveLimits, Guards, LogLine};
use hamlet_engine_plugins::{
    Asked, ContributedBy, Contributions, PluginInfo, Stopped,
};
use hamlet_engine_shell::style::{caption, description};
use hamlet_engine_shell::{
    AnyView, Cx, EngineTheme, Overlay, Panel, PanelSection,
    SettingsFooter, SettingsPage,
};

use crate::WasmCommandsExt;
use crate::budget::{Spending, tally};
use crate::folder::Package;
use crate::host::WasmTuning;
use crate::instance::{Caps, Fault, Instance, NodeInput, Spent};
use crate::mail::deliver;
use crate::retained::NodeStats;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Serve>()
        .add_message::<Served>()
        .add_observer(adopt_panel)
        .add_observer(adopt_page)
        .add_observer(adopt_footer)
        .add_observer(adopt_overlay)
        .add_observer(adopt_section)
        .add_systems(
            Update,
            (deliver, tally, serve, show_stopped).chain(),
        );
}

/// On a WASM plugin's panel, settings page, overlay or section: its
/// name within the plugin, and why the plugin was stopped, if it was.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Surface {
    pub name: String,
    pub stopped: Option<String>,
}

/// On a running WASM plugin: what it is to be given, oldest first.
#[derive(Component, Default)]
pub(crate) struct Inbox(pub(crate) VecDeque<Work>);

pub(crate) enum Work {
    /// Its command of this name.
    Run(String),
    /// A message from the engine's services, at its depth in a chain.
    Mail(hamlet_engine_api::Message, u32),
    /// A call of one of its services.
    Serve(Serve),
    /// Its panel or settings page `name` to build under the holder
    /// `root`.
    Mount { root: Entity, name: String },
    /// Its panel or settings page of this name, whose nodes are gone.
    Unmount(String),
    /// What the person did to the node it named with this key.
    Node(u32, NodeInput),
}

/// On a plugin whose views still move, given a frame each frame.
#[derive(Component)]
pub(crate) struct Moving;

/// That a plugin was stopped, and why.
pub(crate) fn stopped(why: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                caption("STOPPED", Tone::Critical),
                label("The plugin was stopped."),
                description(why),
            ))
            .gap(gap),
        )
    })
}

/// Marks a WASM plugin's new panel as its.
fn adopt_panel(
    added: On<Add, Panel>,
    panels: Query<(&Panel, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((panel, by)) = panels.get(added.entity)
        && packages.contains(by.0)
    {
        commands.adopt_surface(added.entity, &panel.id);
    }
}

/// Marks a WASM plugin's new settings page as its.
fn adopt_page(
    added: On<Add, SettingsPage>,
    pages: Query<(&SettingsPage, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((page, by)) = pages.get(added.entity)
        && packages.contains(by.0)
    {
        commands.adopt_surface(added.entity, &page.id);
    }
}

/// Marks a WASM plugin's new settings footer as its.
fn adopt_footer(
    added: On<Add, SettingsFooter>,
    footers: Query<(&SettingsFooter, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((footer, by)) = footers.get(added.entity)
        && packages.contains(by.0)
    {
        commands.adopt_surface(added.entity, &footer.id);
    }
}

/// Marks a WASM plugin's new overlay as its.
fn adopt_overlay(
    added: On<Add, Overlay>,
    overlays: Query<(&Overlay, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((overlay, by)) = overlays.get(added.entity)
        && packages.contains(by.0)
    {
        commands.adopt_surface(added.entity, &overlay.id);
    }
}

/// Marks a WASM plugin's new panel section as its.
fn adopt_section(
    added: On<Add, PanelSection>,
    sections: Query<(&PanelSection, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((section, by)) = sections.get(added.entity)
        && packages.contains(by.0)
    {
        commands.adopt_surface(added.entity, &section.id);
    }
}

/// Gives the running plugins all that is in their inboxes, and a
/// frame to those whose views still move. Only a plugin past the CPU
/// time a second the person set leaves work for the next second. A
/// plugin at fault is stopped.
pub(crate) fn serve(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &ActiveLimits,
        &mut Instance,
        &mut Inbox,
        &mut Spending,
        Has<Moving>,
    )>,
    guards: Res<Guards>,
    tuning: Res<WasmTuning>,
    theme: Option<Res<EngineTheme>>,
    time: Res<Time>,
    mut stats: ResMut<NodeStats>,
    mut log: MessageWriter<LogLine>,
    mut asked: MessageWriter<Asked>,
    mut served_calls: MessageWriter<Served>,
    mut commands: Commands,
) {
    let motions = theme.map_or([0.0; 2], |theme| {
        [theme.motion.interact, theme.motion.expand]
    });
    let micros = u32::try_from(time.delta().as_micros()).unwrap_or(0);
    for (
        plugin,
        info,
        limits,
        mut instance,
        mut inbox,
        mut spending,
        was_moving,
    ) in &mut plugins
    {
        let caps = Caps::of(&limits.0, &guards, &tuning);
        let mut fault = None;
        let mut moving = was_moving;
        let mut moved = false;
        while !inbox.0.is_empty() && spending.may_call(limits) {
            let Some(work) = inbox.0.pop_front() else {
                break;
            };
            // A call of its views, which says whether they still
            // move.
            let mut viewed = None;
            let ((done, spent), depth) = match work {
                Work::Run(name) => (instance.run(&caps, &name), 0),
                Work::Mail(message, depth) => {
                    let (messages, depth) =
                        inbox.batch((message, depth));
                    (instance.receive(&caps, &messages), depth)
                }
                Work::Serve(call) => {
                    let (done, spent) = instance.serve_call(
                        &caps,
                        &call,
                        &mut served_calls,
                    );
                    ((done, spent), call.depth)
                }
                Work::Mount { root, name } => {
                    let (done, spent, number) =
                        instance.mount(&caps, &name, motions);
                    viewed = Some((done, Some((number, root))));
                    ((Ok(()), spent), 0)
                }
                Work::Unmount(name) => {
                    let (done, spent) =
                        instance.unmount(&caps, &name);
                    viewed = Some((done, None));
                    ((Ok(()), spent), 0)
                }
                Work::Node(key, input) => {
                    let mut inputs = vec![(key, input)];
                    while let Some(Work::Node(..)) = inbox.0.front() {
                        if let Some(Work::Node(key, input)) =
                            inbox.0.pop_front()
                        {
                            inputs.push((key, input));
                        }
                    }
                    let (done, spent) = instance.input(&caps, inputs);
                    viewed = Some((done, None));
                    ((Ok(()), spent), 0)
                }
            };
            // Any call may change the plugin's nodes, and what it
            // shows outside its panels.
            let changes = instance.take_node_changes();
            commands.apply_surface_changes(
                plugin,
                instance.take_surface_changes(),
            );
            let done = match viewed {
                Some((done, root)) => {
                    commands
                        .apply_node_changes(plugin, root, changes);
                    stats.count(&spent);
                    moved = true;
                    done.map(|now| moving = now)
                }
                None => {
                    if !changes.is_empty() {
                        commands.apply_node_changes(
                            plugin, None, changes,
                        );
                        // Its next frame says whether they still
                        // move.
                        moving = true;
                    }
                    done
                }
            };
            instance.pass_requests(plugin, depth, &mut asked);
            spending.charge(spent);
            if let Err(why) = done {
                fault = Some(why);
                break;
            }
        }
        // What moves is moved on once a frame, unless it was just
        // given an input.
        if was_moving
            && !moved
            && fault.is_none()
            && spending.may_call(limits)
        {
            let (done, spent) = instance.frame(&caps, micros);
            let changes = instance.take_node_changes();
            commands.apply_node_changes(plugin, None, changes);
            commands.apply_surface_changes(
                plugin,
                instance.take_surface_changes(),
            );
            stats.count(&spent);
            spending.charge(spent);
            match done {
                Ok(now) => moving = now,
                Err(why) => fault = Some(why),
            }
        }
        if moving != was_moving {
            if moving {
                commands.entity(plugin).insert(Moving);
            } else {
                commands.entity(plugin).remove::<Moving>();
            }
        }
        instance.pass_logs(info, &mut log);
        if let Some(fault) = fault {
            commands.stop_plugin(&mut log, plugin, info, &fault);
        }
    }
}

impl Instance {
    /// Serves `call` and says what became of it. A plugin past the
    /// call time the person set leaves the caller out of budget.
    fn serve_call(
        &mut self,
        caps: &Caps,
        call: &Serve,
        served: &mut MessageWriter<Served>,
    ) -> (Result<(), Fault>, Spent) {
        let (result, spent) = self.serve(
            caps,
            &call.service,
            call.caller.as_str(),
            &call.argument,
        );
        let (done, result) = match result {
            Ok(Ok(bytes)) => (Ok(()), Ok(bytes)),
            Ok(Err(why)) => (Ok(()), Err(Failure::Failed(why))),
            Err(fault @ Fault::Time) => {
                let why = Failure::OutOfBudget(fault.to_string());
                (Err(fault), Err(why))
            }
            Err(fault) => {
                let why = Failure::Failed(fault.to_string());
                (Err(fault), Err(why))
            }
        };
        served.write(Served {
            call: call.call,
            result,
        });
        (done, spent)
    }
}

/// Shows on a stopped plugin's panels and settings pages why it was
/// stopped.
fn show_stopped(
    plugins: Query<(&Stopped, &Contributions), Changed<Stopped>>,
    mut surfaces: Query<&mut Surface>,
) {
    for (stopped, contributions) in &plugins {
        for surface in contributions.iter() {
            if let Ok(mut shown) = surfaces.get_mut(surface) {
                shown.stopped = Some(stopped.0.clone());
            }
        }
    }
}

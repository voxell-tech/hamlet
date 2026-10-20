//! Serving WASM plugins: what is in each one's inbox handed to it,
//! frame by frame, within its budgets, and its panels and settings
//! pages marked as its, to be built node by node (`retained.rs`).

use std::collections::VecDeque;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Has, With};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::time::Time;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label};
use hamlet_engine_api::Failure;
use hamlet_engine_bus::{Serve, Served};
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_plugins::{
    Asked, ContributedBy, Contributions, PluginInfo, Stopped,
};
use hamlet_engine_shell::style::{caption, note};
use hamlet_engine_shell::{
    AnyView, Cx, EngineTheme, Panel, SettingsPage,
};

use crate::WasmCommandsExt;
use crate::budget::{Spending, tally};
use crate::folder::Package;
use crate::host::WasmTuning;
use crate::instance::{Caps, Fault, Instance, NodeInput, Spent};
use crate::mail::{batch, deliver};
use crate::retained::NodeStats;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Serve>()
        .add_message::<Served>()
        .add_observer(adopt_panel)
        .add_observer(adopt_page)
        .add_systems(
            Update,
            (deliver, tally, serve, show_stopped).chain(),
        );
}

/// On a WASM plugin's panel or settings page: its name within the
/// plugin, and why the plugin was stopped, if it was.
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
                note(why),
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

/// Gives the running plugins what is in their inboxes, and a frame
/// to those whose views still move, within the frame's share and
/// each plugin's budgets of the second. The plugin served longest ago
/// goes first, and what is left waits for the next frame. A plugin
/// at fault is stopped.
pub(crate) fn serve(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &mut Instance,
        &mut Inbox,
        &mut Spending,
        Has<Moving>,
    )>,
    budgets: Res<Budgets>,
    tuning: Res<WasmTuning>,
    theme: Option<Res<EngineTheme>>,
    time: Res<Time>,
    mut stats: ResMut<NodeStats>,
    mut frame: Local<u64>,
    mut log: MessageWriter<LogLine>,
    mut asked: MessageWriter<Asked>,
    mut served_calls: MessageWriter<Served>,
    mut commands: Commands,
) {
    *frame += 1;
    let caps = Caps::of(&budgets, &tuning);
    let motions = theme.map_or([0.0; 2], |theme| {
        [theme.motion.interact, theme.motion.expand]
    });
    let micros = u32::try_from(time.delta().as_micros()).unwrap_or(0);
    let mut left =
        Duration::from_secs_f32(budgets.frame_share / 1000.0);
    let mut order = plugins
        .iter()
        .map(|(plugin, .., spending, _)| (spending.served, plugin))
        .collect::<Vec<_>>();
    order.sort();
    for (_, plugin) in order {
        let Ok((
            _,
            info,
            mut instance,
            mut inbox,
            mut spending,
            was_moving,
        )) = plugins.get_mut(plugin)
        else {
            continue;
        };
        let mut fault = None;
        let mut served = false;
        let mut moving = was_moving;
        let mut moved = false;
        while let Some(work) = inbox.0.front() {
            let event = matches!(work, Work::Node(..));
            if left.is_zero() || !spending.may_call(&budgets, event) {
                break;
            }
            let Some(work) = inbox.0.pop_front() else {
                break;
            };
            // A service's caller pays for its call.
            let mut paid = false;
            // A call of its views, which says whether they still
            // move.
            let mut viewed = None;
            let ((done, spent), depth) = match work {
                Work::Run(name) => (instance.run(&caps, &name), 0),
                Work::Mail(message, depth) => {
                    let (messages, depth) = batch(
                        (message, depth),
                        &mut inbox,
                        caps.bytes,
                    );
                    (instance.receive(&caps, &messages), depth)
                }
                Work::Serve(call) => {
                    paid = true;
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
            // Any call may change the plugin's nodes.
            let changes = instance.take_node_changes();
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
            spending.charge(
                if paid { Spent::default() } else { spent },
                event,
            );
            left = left.saturating_sub(spent.time);
            served = true;
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
            && !left.is_zero()
            && spending.may_call(&budgets, false)
        {
            let (done, spent) = instance.frame(&caps, micros);
            let changes = instance.take_node_changes();
            commands.apply_node_changes(plugin, None, changes);
            stats.count(&spent);
            spending.charge(spent, false);
            left = left.saturating_sub(spent.time);
            served = true;
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
        instance.pass_logs(info, &mut spending, &budgets, &mut log);
        if served {
            spending.served = *frame;
        }
        if let Some(fault) = fault {
            commands.stop_plugin(&mut log, plugin, info, &fault);
        }
    }
}

impl Instance {
    /// Serves `call` and says what became of it. A plugin out of its
    /// fuel or time for the call leaves the caller out of budget.
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
            Err(fault @ (Fault::Fuel | Fault::Time)) => {
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
            fuel: spent.fuel,
            time: spent.time,
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

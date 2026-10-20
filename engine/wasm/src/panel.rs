//! A WASM plugin's panels and settings pages: drawn from the widget
//! tree the plugin gives, with what the person does sent back to it.
//! A panel is drawn when it comes, when the plugin asks, and after
//! the plugin is given an event or runs a command.

use std::collections::VecDeque;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::message::{Message, MessageReader, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Has, With};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label};
use bevy_fynix::{ViewExt, component, keyed};
use hamlet_engine_api::{
    Action, Failure, PanelId, WidgetEvent, WidgetId, WidgetTree,
};
use hamlet_engine_bus::{Serve, Served};
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_plugins::{
    Asked, ContributedBy, Contributions, PluginInfo, Stopped,
};
use hamlet_engine_shell::{
    AnyView, Panel, SettingsPage, panel, tree_view,
};

use crate::budget::{Spending, pass_logs, tally};
use crate::folder::Package;
use crate::host::WasmTuning;
use crate::instance::{Caps, Fault, Instance, Spent};
use crate::mail::{batch, deliver, pass_requests};
use crate::reload::Reloading;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Input>()
        .add_message::<Serve>()
        .add_message::<Served>()
        .add_observer(adopt_panel)
        .add_observer(adopt_page)
        .add_systems(
            Update,
            (route, deliver, tally, serve, show_stopped).chain(),
        );
}

/// On a WASM plugin's panel or settings page: its name within the
/// plugin, and what it shows.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Drawn {
    pub name: String,
    pub shows: Shows,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Shows {
    /// Not drawn yet.
    Waiting,
    Tree(WidgetTree),
    /// The plugin was stopped: why.
    Stopped(String),
}

/// On a panel or settings page to be drawn again.
#[derive(Component)]
pub(crate) struct Redraw;

/// On a running WASM plugin: what it is to be given, oldest first.
#[derive(Component, Default)]
pub(crate) struct Inbox(pub(crate) VecDeque<Work>);

pub(crate) enum Work {
    Event(WidgetEvent),
    /// Its command of this name.
    Run(String),
    /// A message from the engine's services, at its depth in a chain.
    Mail(hamlet_engine_api::Message, u32),
    /// A call of one of its services.
    Serve(Serve),
}

/// What the person did to a widget of the panel or settings page
/// `surface`.
#[derive(Message, Clone, Debug)]
pub(crate) struct Input {
    pub(crate) surface: Entity,
    pub(crate) widget: WidgetId,
    pub(crate) action: Action,
}

/// The content of a WASM plugin's panel or settings page.
pub(crate) fn view(surface: Entity) -> AnyView {
    let shows = component::<Drawn, _>(surface, |drawn| {
        drawn.map(|drawn| drawn.shows.clone())
    });
    panel(keyed(shows, move |shows| match shows {
        Some(Shows::Tree(tree)) => tree_view(
            tree,
            move |world: &mut World, widget, action| {
                world.write_message(Input {
                    surface,
                    widget,
                    action,
                });
            },
        ),
        Some(Shows::Stopped(why)) => column((
            label("The plugin was stopped."),
            label(why.clone()).tone(Tone::Dim),
        ))
        .gap(8.0)
        .boxed(),
        _ => column(()).boxed(),
    }))
}

/// Draws a WASM plugin's new panel.
fn adopt_panel(
    added: On<Add, Panel>,
    panels: Query<(&Panel, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((panel, by)) = panels.get(added.entity)
        && packages.contains(by.0)
    {
        adopt(&mut commands, added.entity, &panel.id);
    }
}

/// Draws a WASM plugin's new settings page.
fn adopt_page(
    added: On<Add, SettingsPage>,
    pages: Query<(&SettingsPage, &ContributedBy)>,
    packages: Query<(), With<Package>>,
    mut commands: Commands,
) {
    if let Ok((page, by)) = pages.get(added.entity)
        && packages.contains(by.0)
    {
        adopt(&mut commands, added.entity, &page.id);
    }
}

fn adopt(commands: &mut Commands, surface: Entity, id: &PanelId) {
    let name = id
        .as_str()
        .split_once('/')
        .map_or_else(String::new, |(_, name)| name.to_owned());
    commands.entity(surface).insert((
        Drawn {
            name,
            shows: Shows::Waiting,
        },
        Redraw,
    ));
}

/// Hands what the person did to the plugin whose panel it was. An
/// inbox holds a second's events at most; the rest are left out.
fn route(
    mut inputs: MessageReader<Input>,
    surfaces: Query<(&Drawn, &ContributedBy)>,
    mut inboxes: Query<(&mut Inbox, &mut Spending)>,
    budgets: Res<Budgets>,
) {
    for input in inputs.read() {
        let Ok((drawn, by)) = surfaces.get(input.surface) else {
            continue;
        };
        let Ok((mut inbox, mut spending)) = inboxes.get_mut(by.0)
        else {
            continue;
        };
        if inbox.0.len() >= budgets.events_per_second as usize {
            spending.dropped_events += 1;
            continue;
        }
        inbox.0.push_back(Work::Event(WidgetEvent {
            panel: drawn.name.clone(),
            widget: input.widget.clone(),
            action: input.action.clone(),
        }));
    }
}

/// Gives the running plugins what is in their inboxes, then draws
/// their panels that are to be drawn again, within the frame's share
/// and each plugin's budgets of the second. The plugin served longest
/// ago goes first, and what is left waits for the next frame, but
/// one panel is drawn each frame whatever the share. A plugin at
/// fault is stopped.
fn serve(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &mut Instance,
        &mut Inbox,
        &mut Spending,
        Option<&Contributions>,
    )>,
    mut surfaces: Query<(&mut Drawn, Has<Redraw>)>,
    budgets: Res<Budgets>,
    tuning: Res<WasmTuning>,
    mut frame: Local<u64>,
    mut log: MessageWriter<LogLine>,
    mut asked: MessageWriter<Asked>,
    mut served_calls: MessageWriter<Served>,
    mut commands: Commands,
) {
    *frame += 1;
    let caps = Caps::of(&budgets, &tuning);
    let limits = budgets.tree_limits();
    let mut left =
        Duration::from_secs_f32(budgets.frame_share / 1000.0);
    let mut drew = false;
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
            contributions,
        )) = plugins.get_mut(plugin)
        else {
            continue;
        };
        let mut fault = None;
        let mut served = false;
        // After an event or a command, each of its panels is drawn
        // again.
        let mut all = false;
        while let Some(work) = inbox.0.front() {
            let event = matches!(work, Work::Event(_));
            if left.is_zero() || !spending.may_call(&budgets, event) {
                break;
            }
            let Some(work) = inbox.0.pop_front() else {
                break;
            };
            // A service's caller pays for its call.
            let mut paid = false;
            let ((done, spent), depth) = match work {
                Work::Event(event) => {
                    (instance.event(&caps, &event), 0)
                }
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
                    let (done, spent) = serve_call(
                        &mut instance,
                        &caps,
                        &call,
                        &mut served_calls,
                    );
                    ((done, spent), call.depth)
                }
            };
            pass_requests(plugin, &mut instance, depth, &mut asked);
            spending.charge(
                if paid { Spent::default() } else { spent },
                event,
            );
            left = left.saturating_sub(spent.time);
            (served, all) = (true, true);
            if let Err(why) = done {
                fault = Some(why);
                break;
            }
        }
        let redraws = instance.take_redraws();
        for surface in
            contributions.into_iter().flat_map(|c| c.iter())
        {
            if fault.is_some() {
                break;
            }
            let Ok((mut drawn, redraw)) = surfaces.get_mut(surface)
            else {
                continue;
            };
            if !(all || redraw || redraws.contains(&drawn.name)) {
                continue;
            }
            let may = (!left.is_zero() || !drew)
                && spending.may_call(&budgets, false);
            if !may {
                commands.entity(surface).insert(Redraw);
                continue;
            }
            commands.entity(surface).remove::<Redraw>();
            let (tree, spent) = instance.draw(&caps, &drawn.name);
            pass_requests(plugin, &mut instance, 0, &mut asked);
            spending.charge(spent, false);
            left = left.saturating_sub(spent.time);
            (served, drew) = (true, true);
            let tree = tree.and_then(|tree| {
                tree.check(&limits)
                    .map(|()| tree)
                    .map_err(|why| Fault::Unreadable(why.to_string()))
            });
            match tree {
                Ok(tree) => {
                    let shows = Shows::Tree(tree);
                    if drawn.shows != shows {
                        drawn.shows = shows;
                    }
                }
                Err(why) => fault = Some(why),
            }
        }
        pass_logs(
            info,
            &mut instance,
            &mut spending,
            &budgets,
            &mut log,
        );
        if served {
            spending.served = *frame;
        }
        if let Some(fault) = fault {
            stop(&mut commands, &mut log, plugin, info, &fault);
        }
    }
}

/// Serves `call` and says what became of it. A plugin out of its fuel
/// or time for the call leaves the caller out of budget.
fn serve_call(
    instance: &mut Instance,
    caps: &Caps,
    call: &Serve,
    served: &mut MessageWriter<Served>,
) -> (Result<(), Fault>, Spent) {
    let (result, spent) = instance.serve(
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

/// Stops a plugin at fault and tells the person why.
pub(crate) fn stop(
    commands: &mut Commands,
    log: &mut MessageWriter<LogLine>,
    plugin: Entity,
    info: &PluginInfo,
    fault: &Fault,
) {
    commands
        .entity(plugin)
        .remove::<(Instance, Inbox, Spending, Reloading)>()
        .insert(Stopped(fault.to_string()));
    log.write(LogLine::error(
        info.id.as_str(),
        format!("{} was stopped: {fault}", info.name),
    ));
}

/// Shows on a stopped plugin's panels why it was stopped.
fn show_stopped(
    plugins: Query<(&Stopped, &Contributions), Changed<Stopped>>,
    mut surfaces: Query<&mut Drawn>,
) {
    for (stopped, contributions) in &plugins {
        for surface in contributions.iter() {
            if let Ok(mut drawn) = surfaces.get_mut(surface) {
                drawn.shows = Shows::Stopped(stopped.0.clone());
            }
        }
    }
}

//! The Hubs: each a rail of the realm's agents and the page beside
//! it, one of them following the world's selection.

mod details;
pub(crate) mod erasure;
mod gone;
mod memory;
mod papers;
mod rail;

pub(crate) use self::rail::guides;
mod profile;
mod thread;
mod visits;

use std::collections::{BTreeSet, HashMap};

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::Local;
use bevy::ecs::world::World;
use bevy::ui::{percent, px};
use bevy_fynix::dock::{DockTree, DockWindowKind};
use bevy_fynix::shortcut::Scope;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{HamletId, Left, Seen};
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};
use hamlet_ui::styles::{Fills, ICON, filling};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::{Highlight, Updated};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::tabs;
use hamlet_ui::{View, with_cx};

pub use self::memory::Opened;
pub use self::papers::{Papers, keep_papers};
pub use self::rail::Tab as RailTab;
pub(crate) use self::rail::under_name;
pub use self::thread::{Briefing, Sheet};
use crate::convo::{Convo, seat};
use crate::desk::Desks;
use crate::dock::{
    HUB, PanelAppExt, highlight_panel, number, show_tab,
};
use crate::overview;

pub fn plugin(app: &mut App) {
    app.add_numbered_panel(HUB, window)
        .add_plugins((
            rail::plugin,
            visits::plugin,
            thread::plugin,
            erasure::plugin,
        ))
        .init_resource::<CollapsedRails>()
        .init_resource::<crate::forms::OpenForm>()
        .init_resource::<profile::Form>()
        .add_systems(Update, tabs::mark_on::<Sheet>)
        .init_resource::<Papers>()
        .init_resource::<Briefing>()
        .init_resource::<Opened>()
        .init_resource::<papers::Requests>()
        .add_observer(papers::response)
        .add_observer(papers::edited)
        .add_observer(papers::papers_on_job)
        .add_systems(
            Update,
            (
                keep_hubs
                    .run_if(resource_changed::<DockTree>)
                    .after(crate::desk::capture),
                note_collapsed,
                thread::leave_prompt,
                // Cheap when nothing changed, and it follows the
                // following Hub wherever that moves.
                keep_papers,
            ),
        );
}

/// A Hub, by the id of its dock window.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Hub(pub String);

/// On the one Hub that follows the world's selection.
#[derive(Component)]
pub struct Following;

/// What a Hub's page shows.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Showing {
    /// Every hamlet, and the Hand.
    #[default]
    Realm,
    /// The chat with the agent the Hub is seated at.
    Thread,
    /// The whole of that agent's prompt.
    Prompt,
    Profile(Entity),
    Hamlet(Entity),
    /// An agent that is gone.
    Gone(Entity),
    Chief(Entity),
    /// The library's shelves.
    Library,
}

/// The panel ids of the Hubs whose rails are collapsed, as the view
/// keeps them.
#[derive(Resource, Default, PartialEq, Clone)]
pub(crate) struct CollapsedRails(pub(crate) BTreeSet<String>);

/// The dock window of the Hub `id`.
pub fn window(id: &str) -> DockWindowKind<HamletTheme> {
    let id = id.to_owned();
    DockWindowKind::new("Hub", move || {
        // Its tab glows while anything waits for the chief.
        highlight_panel(
            panel(id.clone()),
            Highlight {
                attention: true,
                flash: true,
            },
        )
    })
    .icon(Glyph::Tree.image(ICON))
}

fn is_hub(id: &str) -> bool {
    id == HUB || number(HUB, id).is_some()
}

/// The first Hub is 1, the others their number.
fn rank(id: &str) -> u32 {
    number(HUB, id).unwrap_or(1)
}

/// The Hub of the panel `id`.
fn hub_of(world: &World, id: &str) -> Option<Entity> {
    let mut hubs = world.try_query::<(Entity, &Hub)>()?;
    hubs.iter(world)
        .find(|(_, hub)| hub.0 == id)
        .map(|(entity, _)| entity)
}

/// The Hub that follows the world's selection.
pub(crate) fn following(world: &World) -> Option<Entity> {
    let mut hubs =
        world.try_query_filtered::<Entity, With<Following>>()?;
    hubs.iter(world).next()
}

pub(crate) fn is_following(world: &World, hub: Entity) -> bool {
    world.get::<Following>(hub).is_some()
}

/// The Hub under the mouse's pointer.
pub(crate) fn under_pointer(world: &World) -> Option<Entity> {
    let node = crate::trail::under_pointer(world, |node| {
        world.get::<rail::Frame>(node).is_some()
    })?;
    Some(world.get::<rail::Frame>(node)?.0)
}

/// Brings the following Hub's tab forward, opening the first Hub
/// when the dock holds none.
pub(crate) fn bring_following(world: &mut World) -> Option<Entity> {
    match following(world) {
        Some(hub) => {
            let id = world.get::<Hub>(hub)?.0.clone();
            show_tab(world, &id);
        }
        None => {
            show_tab(world, HUB);
            let _ = world.run_system_cached(keep_hubs);
        }
    }
    following(world)
}

/// Makes `hub` the one of the shown desk that follows the world's
/// selection.
pub(crate) fn follow(world: &mut World, hub: Entity) {
    if let Some(id) = world.get::<Hub>(hub).map(|hub| hub.0.clone())
        && let Some(mut desks) = world.get_resource_mut::<Desks>()
    {
        desks.follow(&id);
    }
    if is_following(world, hub) {
        return;
    }
    if let Some(was) = following(world) {
        world.entity_mut(was).remove::<Following>();
    }
    world.entity_mut(hub).insert(Following);
    // It shows what is selected now.
    if let Some(mut selected) = world.get_resource_mut::<Selected>() {
        selected.set_changed();
    }
}

/// Keeps a Hub for each Hub panel the dock holds, the one the shown
/// desk chose following, else its first.
fn keep_hubs(world: &mut World) {
    let Some(tree) = world.get_resource::<DockTree>() else {
        return;
    };
    let docked = tree.root.map_or_else(Vec::new, |root| {
        tree.leaves_under(root)
            .into_iter()
            .flat_map(|(_, leaf)| {
                leaf.tabs().map(|(id, _)| id.to_owned())
            })
            .filter(|id| is_hub(id))
            .collect::<Vec<_>>()
    });
    let had = world
        .query::<(Entity, &Hub)>()
        .iter(world)
        .map(|(entity, hub)| (entity, hub.0.clone()))
        .collect::<Vec<_>>();
    for (entity, id) in &had {
        if !docked.contains(id) {
            world.despawn(*entity);
        }
    }
    let collapsed = world
        .get_resource::<CollapsedRails>()
        .cloned()
        .unwrap_or_default();
    for id in &docked {
        if had.iter().any(|(_, had)| had == id) {
            continue;
        }
        let rail = match collapsed.0.contains(id) {
            true => RailState::Collapsed,
            false => RailState::Expanded,
        };
        spawn(world, id.clone(), rail);
    }
    let chosen = world
        .get_resource::<Desks>()
        .and_then(Desks::following)
        .filter(|id| docked.iter().any(|docked| docked == id))
        .map(str::to_owned);
    let first = docked.iter().min_by_key(|id| rank(id)).cloned();
    if let Some(hub) =
        chosen.or(first).and_then(|id| hub_of(world, &id))
    {
        follow(world, hub);
    }
}

/// Whether a Hub's rail is collapsed to a strip.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RailState {
    Expanded,
    Collapsed,
}

/// Spawns the Hub of the panel `id`, on the realm's page.
pub(crate) fn spawn(
    world: &mut World,
    id: String,
    rail: RailState,
) -> Entity {
    let hub = world
        .spawn((
            Hub(id),
            Showing::default(),
            Sheet::default(),
            visits::Visits::default(),
        ))
        .id();
    rail::equip(world, hub, rail);
    hub
}

/// The first Hub, made following the selection when there is none.
pub fn first(world: &mut World) -> Entity {
    if let Some(hub) = hub_of(world, HUB) {
        return hub;
    }
    let hub = spawn(world, HUB.into(), RailState::Expanded);
    world.entity_mut(hub).insert(Following);
    hub
}

/// Notes whose rails are collapsed, for the view to keep. Hubs of
/// other desks stay as they were.
fn note_collapsed(world: &mut World) {
    let shown = world
        .query::<&Hub>()
        .iter(world)
        .map(|hub| hub.0.clone())
        .collect::<BTreeSet<_>>();
    let mut now = world.resource::<CollapsedRails>().clone();
    now.0.retain(|id| !shown.contains(id));
    now.0.extend(rail::collapsed(world));
    if *world.resource::<CollapsedRails>() != now {
        *world.resource_mut::<CollapsedRails>() = now;
    }
}

/// What `hub` shows.
pub(crate) fn showing(world: &World, hub: Entity) -> Option<Showing> {
    world.get::<Showing>(hub).copied()
}

/// Turns `hub`'s page to `showing`, and its rail to the tab.
pub(crate) fn show(world: &mut World, hub: Entity, showing: Showing) {
    if let Some(mut now) = world.get_mut::<Showing>(hub)
        && *now != showing
    {
        *now = showing;
    }
    let tab = match showing {
        Showing::Hamlet(_) => rail::Tab::Hamlets,
        Showing::Profile(_) => rail::Tab::City,
        _ => return,
    };
    rail::turn(world, hub, tab);
}

/// Turns the following Hub to `showing` and brings it forward.
pub(crate) fn show_following(world: &mut World, showing: Showing) {
    if let Some(hub) = bring_following(world) {
        show(world, hub, showing);
    }
}

/// Selects `thing` in the world and brings it into view, when `hub`
/// is the one following the selection.
pub(crate) fn look_at(world: &mut World, hub: Entity, thing: Thing) {
    if is_following(world, hub)
        && world.contains_resource::<Selected>()
    {
        select(world, Some(thing));
        bring_into_view(world, thing);
    }
}

/// Turns `hub` to the thread with `agent`, as the following Hub does
/// with its house chosen in the world.
pub(crate) fn talk_in(world: &mut World, hub: Entity, agent: Entity) {
    if is_following(world, hub) {
        crate::convo::talk_to(world, agent);
    } else if seat(world, agent) == crate::convo::Seating::Seated {
        show(world, hub, Showing::Thread);
    }
}

/// Turns the following Hub to `profile`'s page, its house chosen in
/// the world.
pub(crate) fn open_profile(world: &mut World, profile: Entity) {
    open(world, Showing::Profile(profile), Thing::Profile(profile));
}

/// Turns the following Hub to `hamlet`'s page, its common chosen in
/// the world.
pub(crate) fn open_hamlet(world: &mut World, hamlet: Entity) {
    open(world, Showing::Hamlet(hamlet), Thing::Common(hamlet));
}

fn open(world: &mut World, showing: Showing, thing: Thing) {
    let Some(hub) = bring_following(world) else {
        return;
    };
    show(world, hub, showing);
    look_at(world, hub, thing);
}

/// `hub`'s page.
fn page(hub: Entity) -> impl View {
    keyed(
        bevy_fynix::component::<Showing, _>(hub, |showing| {
            showing.copied().unwrap_or_default()
        }),
        move |showing| match showing {
            Showing::Realm => overview::overview(hub).boxed(),
            Showing::Profile(profile) => {
                profile::page(hub, *profile).boxed()
            }
            Showing::Thread => thread::living(hub).boxed(),
            Showing::Prompt => thread::prompt_page(hub).boxed(),
            Showing::Hamlet(hamlet) => {
                overview::hamlet(hub, *hamlet).boxed()
            }
            Showing::Gone(agent) => gone::ruin(hub, *agent).boxed(),
            Showing::Chief(chief) => crate::chief::panel(*chief),
            Showing::Library => gone::library(hub).boxed(),
        },
    )
    .within(filling())
}

/// The Hub panel `id`, once its Hub is kept.
fn panel(id: String) -> impl View {
    keyed(
        derived(move |world: &World| hub_of(world, &id))
            .when(every_frame()),
        |hub| match hub {
            Some(hub) => screen(*hub).boxed(),
            None => frame().boxed(),
        },
    )
    .within(filling())
}

/// `hub`'s contents: the rail beside the page.
fn screen(hub: Entity) -> impl View {
    with_cx(move |cx| {
        let gap = cx.theme().space.section;
        row((
            rail::view(hub),
            rail::divider(hub),
            column((page(hub),)).grow(1.0).min_width(px(0.0)),
        ))
        .gap(gap)
        .fills_height()
        .width(percent(100.0))
        .tagged(rail::Frame(hub))
        .tagged(Scope(visits::KEYS))
    })
}

/// Turns the following Hub to what is selected in the world.
pub(crate) fn follow_selection(world: &mut World) {
    let selected = world.resource_ref::<Selected>();
    if !selected.is_changed() {
        return;
    }
    let Some(hub) = following(world) else {
        return;
    };
    let showing = match selected.0 {
        Some(Thing::Keeper | Thing::Cottage) => {
            let Some(lead) = hamlet_client::realm::first_lead(world)
            else {
                return;
            };
            seat_if_new(world, lead);
            Showing::Thread
        }
        Some(Thing::Agent(agent) | Thing::House(agent)) => {
            seat_if_new(world, agent);
            Showing::Thread
        }
        // The Hand's tower is the Hand's.
        Some(Thing::Hand) => {
            let Some(hand) = hamlet_client::realm::hand(world) else {
                return;
            };
            seat_if_new(world, hand);
            Showing::Thread
        }
        Some(Thing::Common(hamlet))
            if world.get_entity(hamlet).is_ok() =>
        {
            Showing::Hamlet(hamlet)
        }
        // One that came back is shown as itself again.
        Some(Thing::Ruin(agent))
            if world.get::<Left>(agent).is_some() =>
        {
            Showing::Gone(agent)
        }
        Some(Thing::Chief(chief) | Thing::ChiefPlace(chief))
            if world.get_entity(chief).is_ok() =>
        {
            Showing::Chief(chief)
        }
        Some(Thing::Library) => Showing::Library,
        Some(Thing::Profile(profile))
            if world.get_entity(profile).is_ok() =>
        {
            Showing::Profile(profile)
        }
        // Nothing, or what has no page of its own.
        _ => return,
    };
    show(world, hub, showing);
    if let Some(thing) = world.resource::<Selected>().0 {
        rail::follow(world, hub, thing);
    }
}

/// Opens the following Hub on the Hand's chat in a realm with no
/// hamlet, else on the realm.
pub(crate) fn open_first(
    world: &mut World,
    mut decided: Local<bool>,
) {
    if *decided || !world.contains_resource::<Seen>() {
        return;
    }
    let Some(hand) = hamlet_client::realm::hand(world) else {
        return;
    };
    let Some(hub) = following(world) else {
        return;
    };
    *decided = true;
    let any = world
        .try_query_filtered::<(), With<HamletId>>()
        .is_some_and(|mut hamlets| {
            hamlets.iter(world).next().is_some()
        });
    if !any {
        seat_if_new(world, hand);
        show(world, hub, Showing::Thread);
    }
}

fn seat_if_new(world: &mut World, agent: Entity) {
    if world.resource::<Convo>().agent != agent {
        seat(world, agent);
    }
}

/// Tells the flashing panels that a Hub shows something else.
pub(crate) fn tell_page(
    world: &mut World,
    mut seen: Local<HashMap<Entity, (Showing, Option<Entity>)>>,
) {
    let Some(mut hubs) = world.try_query::<(Entity, &Showing)>()
    else {
        return;
    };
    let agent = world.resource::<Convo>().agent;
    let now = hubs
        .iter(world)
        .map(|(hub, page)| {
            let agent = (*page == Showing::Thread).then_some(agent);
            (hub, (*page, agent))
        })
        .collect::<HashMap<_, _>>();
    // A Hub's first page is not a change.
    let changed = now.iter().any(|(hub, page)| {
        seen.get(hub).is_some_and(|was| was != page)
    });
    *seen = now;
    if changed {
        world.write_message(Updated);
    }
}

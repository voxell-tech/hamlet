//! The Hub: a rail of the realm's agents and the page beside it.

mod details;
pub(crate) mod erasure;
mod gone;
mod memory;
mod papers;
mod rail;

pub(crate) use self::rail::guides;
mod profile;
pub(crate) mod thread;
mod visits;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::Local;
use bevy::ecs::world::World;
use bevy::ui::{percent, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::shortcut::Scope;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::realm::{HamletId, Left, Seen};
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};
use hamlet_ui::styles::{Fills, ICON, filling};
use hamlet_ui::widgets::effects::{Highlight, Updated};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{View, with_cx};

pub use self::memory::Opened;
pub use self::papers::{Papers, keep_papers};
pub use self::rail::Tab as RailTab;
pub(crate) use self::rail::under_name;
pub use self::thread::{Briefing, Sheet};
use crate::convo::{Convo, seat};
use crate::dock::{HUB, PanelAppExt, highlight_panel, show_tab};
use crate::overview;

pub fn plugin(app: &mut App) {
    app.add_panel(
        HUB,
        DockWindowKind::new("Hub", || {
            // Its tab glows while anything waits for the chief.
            highlight_panel(
                screen(),
                Highlight {
                    attention: true,
                    flash: true,
                },
            )
        })
        .icon(Glyph::Tree.image(ICON)),
    )
    .add_plugins((
        rail::plugin,
        visits::plugin,
        thread::plugin,
        erasure::plugin,
    ))
    .init_resource::<Showing>()
    .init_resource::<crate::forms::OpenForm>()
    .init_resource::<profile::Form>()
    .init_resource::<Sheet>()
    .add_systems(Update, hamlet_ui::widgets::tabs::mark::<Sheet>)
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
            thread::leave_prompt.run_if(resource_changed::<Convo>),
            keep_papers.run_if(
                resource_changed::<Convo>
                    .or_eager(resource_changed::<Showing>),
            ),
        ),
    );
}

/// What the Hub's page shows.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
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

/// Turns the Hub's page to `showing`, and the rail to its tab.
pub(crate) fn show(world: &mut World, showing: Showing) {
    if let Some(mut now) = world.get_resource_mut::<Showing>()
        && *now != showing
    {
        *now = showing;
    }
    let tab = match showing {
        Showing::Hamlet(_) => rail::Tab::Hamlets,
        Showing::Profile(_) => rail::Tab::City,
        _ => return,
    };
    if let Some(mut now) = world.get_resource_mut::<rail::Tab>()
        && *now != tab
    {
        *now = tab;
    }
}

/// Turns the Hub to `profile`'s page, its house chosen in the world.
pub(crate) fn open_profile(world: &mut World, profile: Entity) {
    open(world, Showing::Profile(profile), Thing::Profile(profile));
}

/// Turns the Hub to `hamlet`'s page, its common chosen in the world.
pub(crate) fn open_hamlet(world: &mut World, hamlet: Entity) {
    open(world, Showing::Hamlet(hamlet), Thing::Common(hamlet));
}

fn open(world: &mut World, showing: Showing, thing: Thing) {
    show(world, showing);
    show_tab(world, HUB);
    if world.contains_resource::<Selected>() {
        select(world, Some(thing));
        bring_into_view(world, thing);
    }
}

/// The Hub's page.
fn page() -> impl View {
    keyed(resource::<Showing, _>(|showing| *showing), |showing| {
        match showing {
            Showing::Realm => overview::overview().boxed(),
            Showing::Profile(profile) => {
                profile::page(*profile).boxed()
            }
            Showing::Thread => thread::living().boxed(),
            Showing::Prompt => thread::prompt_page().boxed(),
            Showing::Hamlet(hamlet) => {
                overview::hamlet(*hamlet).boxed()
            }
            Showing::Gone(agent) => gone::ruin(*agent).boxed(),
            Showing::Chief(chief) => crate::chief::panel(*chief),
            Showing::Library => gone::library().boxed(),
        }
    })
    .within(filling())
}

/// The Hub's contents: the rail beside the page.
pub fn screen() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.section;
        row((
            rail::view(),
            rail::divider(),
            column((page(),)).grow(1.0).min_width(px(0.0)),
        ))
        .gap(gap)
        .fills_height()
        .width(percent(100.0))
        .tagged(rail::Frame)
        .tagged(Scope(visits::KEYS))
    })
}

/// Turns the Hub to what is selected in the world.
pub(crate) fn follow_selection(world: &mut World) {
    let selected = world.resource_ref::<Selected>();
    if !selected.is_changed() {
        return;
    }
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
    show(world, showing);
    if let Some(thing) = world.resource::<Selected>().0 {
        rail::follow(world, thing);
    }
}

/// Opens on the Hand's chat in a realm with no hamlet, else the
/// realm.
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
    *decided = true;
    let any = world
        .try_query_filtered::<(), With<HamletId>>()
        .is_some_and(|mut hamlets| {
            hamlets.iter(world).next().is_some()
        });
    if !any {
        seat_if_new(world, hand);
        show(world, Showing::Thread);
    }
}

fn seat_if_new(world: &mut World, agent: Entity) {
    if world.resource::<Convo>().agent != agent {
        seat(world, agent);
    }
}

/// Tells the flashing panels that the Hub shows something else.
pub(crate) fn tell_page(
    world: &mut World,
    mut seen: Local<Option<(Showing, Option<Entity>)>>,
) {
    let page = world.get_resource::<Showing>().copied();
    let Some(page) = page else {
        return;
    };
    let agent = (page == Showing::Thread)
        .then(|| world.resource::<Convo>().agent);
    // The first page seen is not a change.
    if seen
        .replace((page, agent))
        .is_some_and(|was| was != (page, agent))
    {
        world.write_message(Updated);
    }
}

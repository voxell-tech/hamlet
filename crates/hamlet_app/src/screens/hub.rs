//! The Hub: a rail of the realm's agents and the page beside it,
//! which is a thread, a scholar, a hamlet, the library or the home.

mod conversations;
mod details;
mod gone;
mod memory;
mod papers;
mod rail;
mod scholar;
mod thread;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Local, Res};
use bevy::ecs::world::World;
use bevy::ui::{percent, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::realm::Left;
use hamlet_realm::pick::{Selected, Thing};
use hamlet_ui::styles::{Fills, ICON, filling};
use hamlet_ui::widgets::effects::{Highlight, Updated};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{View, with_cx};

pub(crate) use self::gone::{of_agent, skills_screen};
pub use self::memory::Opened;
pub use self::papers::{Papers, keep_papers};
pub use self::rail::Rail;
pub(crate) use self::thread::show_prompt;
pub use self::thread::{Briefing, Sheet};
use crate::convo::{Convo, seat};
use crate::dock::{HUB, PanelAppExt, highlight_panel, panel};
use crate::overview;

pub fn plugin(app: &mut App) {
    app.add_panel(
        HUB,
        DockWindowKind::new("HUB", || {
            highlight_panel(
                screen(),
                Highlight {
                    attention: false,
                    flash: true,
                },
            )
        })
        .icon(Glyph::Lens.image(ICON)),
    )
    // Closed until asked for from a tab bar's plus.
    .add_panel(
        "skills",
        DockWindowKind::new("SKILLS", || panel(skills_screen()))
            .icon(Glyph::Book.image(ICON)),
    )
    .add_plugins(rail::plugin)
    .init_resource::<Showing>()
    .init_resource::<scholar::Form>()
    .init_resource::<Sheet>()
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
    Home,
    /// The chat with the agent the Hub is seated at.
    Thread,
    /// That agent's past sessions.
    Sessions,
    /// The whole of that agent's prompt.
    Prompt,
    Scholar(Entity),
    Hamlet(Entity),
    /// An agent that is gone.
    Gone(Entity),
    Chief(Entity),
    /// The library's shelves.
    Library,
}

/// Whether the Hub shows a page that lists the agent's sessions.
pub(crate) fn reads_sessions(showing: Res<Showing>) -> bool {
    matches!(*showing, Showing::Thread | Showing::Sessions)
}

/// Turns the Hub's page to `showing`.
pub(crate) fn show(world: &mut World, showing: Showing) {
    if let Some(mut now) = world.get_resource_mut::<Showing>()
        && *now != showing
    {
        *now = showing;
    }
}

/// The Hub's page.
fn page() -> impl View {
    keyed(resource::<Showing, _>(|showing| *showing), |showing| {
        match showing {
            Showing::Home => overview::overview().boxed(),
            Showing::Scholar(scholar) => {
                scholar::page(*scholar).boxed()
            }
            Showing::Thread => thread::living().boxed(),
            Showing::Sessions => thread::sessions().boxed(),
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
            frame().width(px(1.0)).fill(cx.theme().palette.gold_6),
            column((page(),)).grow(1.0).min_width(px(0.0)),
        ))
        .gap(gap)
        .fills_height()
        .width(percent(100.0))
        .tagged(rail::Frame)
    })
}

/// A click on a robot or a house selects its agent, and the Hub turns
/// to the thing selected. The panel is not brought to the front: it
/// flashes, and its tab with it, when it is behind another.
pub(crate) fn follow_selection(world: &mut World) {
    let selected = world.resource_ref::<Selected>();
    if !selected.is_changed() {
        return;
    }
    let showing = match selected.0 {
        Some(Thing::Keeper | Thing::Cottage) => {
            let Some(mayor) =
                hamlet_client::realm::first_mayor(world)
            else {
                return;
            };
            seat_if_new(world, mayor);
            Showing::Thread
        }
        Some(Thing::Agent(agent) | Thing::House(agent)) => {
            seat_if_new(world, agent);
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
        // Nothing, or what has no page of its own.
        _ => return,
    };
    show(world, showing);
}

fn seat_if_new(world: &mut World, agent: Entity) {
    if world.resource::<Convo>().agent != agent {
        seat(world, agent);
    }
}

/// Tells the panels that flash that the Hub shows something else:
/// another page, or on a thread, another agent.
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
    // The first look is not a change.
    if seen
        .replace((page, agent))
        .is_some_and(|was| was != (page, agent))
    {
        world.write_message(Updated);
    }
}

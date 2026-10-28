//! The Inspector: whoever is selected, or every hamlet when nobody
//! is.

mod agent;
mod conversations;
mod gone;
mod papers;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::Local;
use bevy::ecs::world::World;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::Left;
use hamlet_realm::pick::{Selected, Thing};

pub(crate) use self::agent::show_prompt;
pub use self::agent::{Briefing, Section};
pub(crate) use self::gone::{of_agent, skills_screen};
pub use self::papers::{Papers, keep_papers};
use crate::convo::{Convo, seat};
use crate::dock::{PanelAppExt, highlight_panel, panel};
use crate::effects::{Highlight, Updated};
use crate::icons::Glyph;
use crate::styles::{ICON, filling};
use crate::{View, overview, tabs};

pub fn plugin(app: &mut App) {
    app.add_panel(
        "agent",
        DockWindowKind::new("INSPECTOR", || {
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
    // Closed until asked for from the View menu or a tab bar's plus.
    .add_panel(
        "skills",
        DockWindowKind::new("SKILLS", || panel(skills_screen()))
            .icon(Glyph::Book.image(ICON)),
    )
    .init_resource::<Section>()
    .init_resource::<Papers>()
    .init_resource::<Briefing>()
    .init_resource::<papers::Requests>()
    .add_observer(papers::response)
    .add_observer(papers::edited)
    .add_observer(papers::papers_on_job)
    .add_systems(
        Update,
        (
            tabs::mark::<Section>,
            agent::leave_prompt.run_if(resource_changed::<Convo>),
            keep_papers.run_if(
                resource_changed::<Convo>
                    .or_eager(resource_changed::<Section>),
            ),
        ),
    );
}

/// The subject of the panel: every hamlet when nothing is selected, a
/// hamlet whose common is, an agent that is gone, a chief, or the
/// agent the chat is with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Showing {
    Overview,
    Hamlet(Entity),
    Gone(Entity),
    Chief(Entity),
    /// The library's shelves.
    Library,
    Agent,
}

fn showing(world: &World) -> Showing {
    let selected = world.get_resource::<Selected>().and_then(|s| s.0);
    match selected {
        Some(Thing::Library) => Showing::Library,
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
        None
        | Some(
            Thing::Common(_)
            | Thing::Ruin(_)
            | Thing::Chief(_)
            | Thing::ChiefPlace(_)
            | Thing::Clockwork(_)
            | Thing::Workbench(_),
        ) => Showing::Overview,
        Some(_) => Showing::Agent,
    }
}

/// The Inspector's contents.
pub fn screen() -> impl View {
    keyed(derived(showing).when(every_frame()), |showing| {
        match showing {
            Showing::Overview => overview::overview().boxed(),
            Showing::Hamlet(hamlet) => {
                overview::hamlet(*hamlet).boxed()
            }
            Showing::Gone(agent) => gone::ruin(*agent).boxed(),
            Showing::Chief(chief) => crate::chief::panel(*chief),
            Showing::Library => gone::library().boxed(),
            Showing::Agent => agent::living().boxed(),
        }
    })
    .within(filling())
}

/// A click on a robot or a house selects its agent. The panel is not
/// brought to the front: it flashes, and its tab with it, when it is
/// behind another.
pub(crate) fn follow_selection(world: &mut World) {
    let selected = world.resource_ref::<Selected>();
    if !selected.is_changed() {
        return;
    }
    let agent = match selected.0 {
        Some(Thing::Keeper | Thing::Cottage) => {
            match hamlet_client::realm::mayor(world) {
                Some(mayor) => mayor,
                None => return,
            }
        }
        Some(Thing::Agent(agent) | Thing::House(agent)) => agent,
        // A hamlet, one that is gone, or nothing at all.
        _ => return,
    };
    if world.resource::<Convo>().agent != agent {
        seat(world, agent);
    }
}

/// Tells the panels that flash that the Inspector shows something
/// else: another page, or on an agent's, another agent.
pub(crate) fn tell_page(
    world: &mut World,
    mut seen: Local<Option<(Showing, Option<Entity>)>>,
) {
    let page = showing(world);
    let agent = (page == Showing::Agent)
        .then(|| world.resource::<Convo>().agent);
    // The first look is not a change.
    if seen
        .replace((page, agent))
        .is_some_and(|was| was != (page, agent))
    {
        world.write_message(Updated);
    }
}

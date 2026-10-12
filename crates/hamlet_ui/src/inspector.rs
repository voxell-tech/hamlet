//! The Inspector: whoever is selected, or every hamlet when nobody
//! is.

mod agent;
mod gone;
mod papers;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::world::World;
use bevy_fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_core::agents::Left;
use hamlet_world::pick::{Selected, Thing};

pub use self::agent::{Briefing, Section};
pub(crate) use self::agent::{now, show_prompt};
pub(crate) use self::gone::{of_agent, skills_screen};
pub use self::papers::{Papers, keep_papers};
pub(crate) use self::papers::{house, house_of};
use crate::dock::show_tab;
use crate::looks::filling;
use crate::me::{Me, talk_to};
use crate::{Ui, overview, tabs};

/// The subject of the panel: every hamlet when nothing is selected, a
/// hamlet whose common is, an agent that is gone, a chief, or the
/// agent the chat is with.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Showing {
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
        None => Showing::Overview,
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
        Some(
            Thing::Common(_)
            | Thing::Ruin(_)
            | Thing::Chief(_)
            | Thing::ChiefPlace(_),
        ) => Showing::Overview,
        Some(_) => Showing::Agent,
    }
}

/// The Inspector's contents.
pub fn screen() -> Ui {
    keyed(derived(showing).when(every_frame()), |showing| {
        match showing {
            Showing::Overview => overview::overview(),
            Showing::Hamlet(hamlet) => overview::hamlet(*hamlet),
            Showing::Gone(agent) => gone::ruin(*agent),
            Showing::Chief(chief) => crate::chief::panel(*chief),
            Showing::Library => gone::library(),
            Showing::Agent => agent::living(),
        }
    })
    .within(filling())
    .boxed()
}

/// A click on a robot or a house selects its agent.
pub(crate) fn follow_selection(world: &mut World) {
    let selected = world.resource_ref::<Selected>();
    if !selected.is_changed() {
        return;
    }
    let agent = match selected.0 {
        Some(Thing::Keeper | Thing::Cottage) => {
            match hamlet_core::agents::mayor(world) {
                Some(mayor) => mayor,
                None => return,
            }
        }
        Some(
            Thing::Resident(agent) | Thing::ResidentHouse(agent),
        ) => agent,
        // A hamlet, one that is gone, or nothing at all.
        _ => return show_tab(world, "agent"),
    };
    if world.resource::<Me>().agent != agent {
        talk_to(world, agent);
    }
    show_tab(world, "agent");
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Section>()
        .init_resource::<Papers>()
        .init_resource::<Briefing>()
        .add_observer(papers::papers_on_job)
        .add_systems(
            Update,
            (
                tabs::mark::<Section>,
                agent::leave_prompt.run_if(resource_changed::<Me>),
                keep_papers.run_if(
                    resource_changed::<Me>
                        .or_eager(resource_changed::<Section>),
                ),
            ),
        );
}

//! The notes, the character and the skills an agent keeps in its
//! house, read for its pages.

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Query, Res, ResMut};
use hamlet_core::agents;
use hamlet_core::core::HomeDir;
use hamlet_core::session::{self, JobFinished, Seat};

use crate::convo::{AgentSelected, Me};

/// The most of the notes or the character the panel shows.
const NOTES_SHOWN: usize = 6000;

/// Reads the papers again when the chat is with another agent, when
/// the selection changes and when a turn has ended.
pub fn plugin(app: &mut App) {
    app.init_resource::<Papers>()
        .init_resource::<AgentSelected>()
        .add_observer(papers_on_job)
        .add_systems(
            Update,
            keep_papers.run_if(
                resource_changed::<Me>
                    .or_eager(resource_changed::<AgentSelected>),
            ),
        );
}

/// The notes, the character and the skills the selected agent keeps
/// in its house, as its pages show them. Empty while no agent is
/// selected.
#[derive(Resource, Clone, Default, PartialEq, Debug)]
pub struct Papers {
    pub notes: String,
    pub character: String,
    /// Each skill of its own by its name, with what it is for.
    pub skills: Vec<(String, String)>,
}

fn read_papers(
    home: Option<&HomeDir>,
    me: &Me,
    selected: AgentSelected,
    seats: &Query<&Seat>,
    papers: &mut ResMut<Papers>,
) {
    if !selected.0 {
        if **papers != Papers::default() {
            **papers = Papers::default();
        }
        return;
    }
    let Some(home) = home else {
        return;
    };
    let house =
        session::house_at(&home.0, seats.get(me.session).ok());
    let kept = agents::papers::Papers::read(&house);
    let shown =
        |text: String| text.chars().take(NOTES_SHOWN).collect();
    let now = Papers {
        notes: shown(kept.notes),
        character: shown(kept.character),
        skills: kept.skills,
    };
    if **papers != now {
        **papers = now;
    }
}

/// Reads the selected agent's papers again.
pub fn keep_papers(
    home: Option<Res<HomeDir>>,
    me: Res<Me>,
    selected: Res<AgentSelected>,
    seats: Query<&Seat>,
    mut papers: ResMut<Papers>,
) {
    read_papers(home.as_deref(), &me, *selected, &seats, &mut papers);
}

/// Reads them again when a turn has ended, which is when an agent
/// has written them.
fn papers_on_job(
    _: On<JobFinished>,
    home: Option<Res<HomeDir>>,
    me: Res<Me>,
    selected: Res<AgentSelected>,
    seats: Query<&Seat>,
    mut papers: ResMut<Papers>,
) {
    read_papers(home.as_deref(), &me, *selected, &seats, &mut papers);
}

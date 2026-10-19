//! The realm as a plugin reads it: the hamlets, the agents and where
//! each stands, the thing selected and the hour. An agent is known by
//! its name and a hamlet by its id, the name of its folder.

use bevy::ecs::resource::Resource;
use bevy::math::Vec3;

/// A hamlet, the name the window shows for it, and the place of its
/// common while its town is drawn.
#[derive(Clone, PartialEq, Debug)]
pub struct SurveyedHamlet {
    pub id: String,
    pub name: String,
    pub at: Option<Vec3>,
}

/// A living agent, the id of its hamlet, the place of its robot while
/// it is drawn, and whether it is at work.
#[derive(Clone, PartialEq, Debug)]
pub struct SurveyedAgent {
    pub name: String,
    pub hamlet: String,
    pub at: Option<Vec3>,
    pub busy: bool,
}

/// The thing selected in the world: its kind, in the words of a
/// command's subject (`agent`, `mayor`, `house`, `ruin`, `hamlet`,
/// `chief`, `hearth` or `library`), and the name of the agent or the
/// chief, or the id of the hamlet.
#[derive(Clone, PartialEq, Debug)]
pub struct Spotted {
    pub kind: String,
    pub name: String,
}

/// The realm as it is now, with places rounded to a tenth of a metre
/// so that it changes only when something moved that far. The realm
/// keeps it.
#[derive(Resource, Clone, PartialEq, Default, Debug)]
pub struct Survey {
    pub hamlets: Vec<SurveyedHamlet>,
    pub agents: Vec<SurveyedAgent>,
    pub selected: Option<Spotted>,
    /// The hour the sky shows, 0 to 24.
    pub hour: f32,
}

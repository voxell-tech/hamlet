//! The scene as the keys and the stills set it.

use bevy::prelude::*;
use hamlet_realm::Stage;
use hamlet_realm::keeper::KeeperState;
use hamlet_vfx::inlay::InlayState;

/// The camera's subject.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Default,
    Wide,
    Keeper,
    Stall,
    Lineup,
}

impl View {
    pub fn next(self) -> Self {
        match self {
            Self::Default => Self::Wide,
            Self::Wide => Self::Keeper,
            Self::Keeper => Self::Stall,
            Self::Stall | Self::Lineup => Self::Default,
        }
    }
}

/// The sky, the groove, the Keeper and the camera's view of the
/// scene.
#[derive(Resource, Clone, PartialEq)]
pub struct Demo {
    pub night: bool,
    pub inlay: InlayState,
    pub keeper_state: KeeperState,
    pub view: View,
    /// Mild tilt-shift blur in the far views.
    pub dof: bool,
    /// Five Keepers stand in a row, one per state.
    pub lineup: bool,
    /// The text on the paper tag at the signal post.
    pub tag: Option<String>,
}

impl Default for Demo {
    fn default() -> Self {
        Demo {
            night: false,
            inlay: InlayState::Unlit,
            keeper_state: KeeperState::Rest,
            view: View::Default,
            dof: true,
            lineup: false,
            tag: None,
        }
    }
}

impl Demo {
    /// True while anything moves, so frames must keep coming.
    pub fn is_animating(&self) -> bool {
        self.inlay != InlayState::Unlit
            || self.keeper_state.is_animated()
            || self.lineup
    }
}

/// Has the world's sky follow the demo's.
pub fn to_stage(demo: Res<Demo>, mut stage: ResMut<Stage>) {
    if stage.night != demo.night {
        stage.night = demo.night;
    }
}

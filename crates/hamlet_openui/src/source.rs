//! The text of a panel, and the program it is checked into.

use bevy::prelude::*;
use openui_lang::Document;

use crate::library::PanelLibrary;
use crate::limits::PanelLimits;
use crate::stale::Stale;
use crate::state::{Raised, State};
use crate::wire::Wires;

/// On a panel's entity: the program that draws it. Putting it again
/// draws the panel from the new text.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Source(pub String);

/// On a panel's entity: its [`Source`] parsed and checked.
#[derive(Component, Clone, Debug)]
pub struct Compiled(pub Document);

/// What a panel drawn from a program carries, to take off it when it
/// is drawn some other way.
pub type Program = (Source, Compiled, Stale);

/// Parses the program of a panel that was given one, and has it
/// drawn.
pub(crate) fn compile(
    insert: On<Insert, Source>,
    sources: Query<&Source>,
    library: Res<PanelLibrary>,
    limits: Res<PanelLimits>,
    mut commands: Commands,
) {
    let Ok(Source(text)) = sources.get(insert.entity) else {
        return;
    };
    let document = openui_lang::parse_with_limits(
        &library.0,
        text,
        limits.runtime(),
    );
    let Ok(mut panel) = commands.get_entity(insert.entity) else {
        return;
    };
    panel.insert((Compiled(document), Stale));
    panel.insert_if_new((
        State::default(),
        Wires::default(),
        Raised::default(),
    ));
}

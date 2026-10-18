//! What the user does in a panel: typing in a bound field, toggling a
//! bound checkbox and pressing a button.

use bevy::prelude::*;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::tree::event::{Act, Interaction};
use openui_lang::Value;

use crate::action;
use crate::source::Compiled;
use crate::stale::Stale;
use crate::state::{Raised, State};
use crate::wire::Wires;

/// Writes what was done to the state of the panel and has the panel
/// drawn again when anything changed.
pub(crate) fn interact(
    event: On<Interaction>,
    mut panels: Query<(
        Entity,
        &Panel,
        &Compiled,
        &Wires,
        &mut State,
        &mut Raised,
    )>,
    mut commands: Commands,
) {
    let Some((entity, _, compiled, wires, mut state, mut raised)) =
        panels
            .iter_mut()
            .find(|(_, panel, ..)| panel.id == event.panel)
    else {
        return;
    };
    let Some(wire) = wires.0.get(&event.node) else {
        return;
    };
    let written = match (&event.act, &wire.binding) {
        (Act::Typed(text), Some(variable)) => {
            Some((variable, Value::from(text.as_str())))
        }
        (Act::Toggled(on), Some(variable)) => {
            Some((variable, Value::Bool(*on)))
        }
        _ => None,
    };
    let mut changed = false;
    if let Some((variable, value)) = written
        && state.0.state(variable) != Some(&value)
    {
        state.0.set_state(variable.as_str(), value);
        changed = true;
    }
    let mut problems = Vec::new();
    if !matches!(event.act, Act::Typed(_))
        && let Some(plan) = &wire.action
    {
        problems = action::run(plan, &compiled.0, &mut state.0);
        changed = true;
    }
    if raised.0 != problems {
        raised.0 = problems;
        changed = true;
    }
    if changed {
        commands.entity(entity).insert(Stale);
    }
}

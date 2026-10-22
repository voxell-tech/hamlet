//! The system prompt an agent would be given now, for whoever wants
//! to read it.

use anyhow::{Result, anyhow};
use bevy_ecs::prelude::*;
use hamlet_rules::hamlet::{Hamlet, MAYOR};
use hamlet_rules::library::Library;
use hamlet_rules::realm::Rules;

use super::worker::Brief;
use super::{Seat, house_of};
use crate::agent::Told;
use crate::core::{HomeDir, Sandboxing};
use crate::tools::Tools;

/// The system prompt the agent at `seat` would begin a turn with
/// now. A turn that answers a chief's instruction adds a line on
/// objecting to it.
pub fn prompt_of(world: &World, seat: &Seat) -> Result<String> {
    let house = house_of(world, seat)?;
    let root = &world.resource::<HomeDir>().0.root;
    let hamlet = Hamlet::at(root, &seat.hamlet);
    let resident = hamlet
        .member(&seat.name)
        .ok_or_else(|| anyhow!("no agent named `{}`", seat.name))?;
    let (resident, mayor) = hamlet.seat(resident.id, &seat.name)?;
    let me = match mayor {
        true => MAYOR,
        false => resident.name.as_str(),
    };
    let brief = Brief::read(world);
    let model = hamlet.model_of(me).unwrap_or(brief.model);
    let project = hamlet.work().project();
    let library = Library::of(root);
    let mut tools =
        Tools::new(house.clone(), world.resource::<Sandboxing>().0);
    let faults = tools.faults();
    Ok(Told {
        house: &house,
        environment: &brief.environment,
        model: &model,
        faults: &faults,
        project: project.as_ref(),
        library: Some(&library),
        place: Some((&hamlet, &resident)),
        mayor,
        rules: Rules::read(root),
        budget: &hamlet.budget_words(me, mayor),
    }
    .prompt())
}

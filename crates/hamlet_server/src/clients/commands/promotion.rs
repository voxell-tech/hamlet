//! A scholar of the city made of what a hire has become (H38): a
//! blank hire's own scholar let into the city, or a new scholar
//! forked from a city scholar's hire.

use hamlet_store::ids::Actor;

use super::{Answer, Line};
use crate::core::Core;
use crate::knowledge::promotion;
use crate::society::addresses::Reach;
use crate::society::city;

/// `/make-scholar <address> [name]`.
pub(super) fn make_scholar(
    core: &mut Core<'_, '_>,
    line: &Line,
) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return Ok(Err(core.deeds.usage(&line.name)));
    };
    let hire =
        match core.resolve(text, line.here.as_ref(), Reach::Kept)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
    if let Err(refusal) = core.leads(hire)? {
        return Ok(Err(refusal));
    }
    let layout = core.realm.layout().clone();
    let source = promotion::source(
        core.realm.store().connection(),
        &layout,
        &core.writing,
        hire,
        &line.tail(1),
    )?;
    let chief = core.realm.chief();
    let made = core.attempt(Actor::Chief(chief), |cx, _| {
        city::make_of_hire(
            cx,
            hire,
            &source.name,
            chief,
            source.commit,
            "",
        )
    })?;
    let db = core.realm.store().connection();
    Ok(match made {
        Ok(id) => Ok(format!(
            "{} lives in the city, made of what `{text}` learned.",
            city::get(db, id)?.label()
        )),
        Err(refusal) => Err(refusal),
    })
}

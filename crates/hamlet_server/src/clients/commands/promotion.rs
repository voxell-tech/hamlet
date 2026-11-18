//! A scholar of the city made of what a hire learned (H38).

use hamlet_store::ids::Actor;

use super::{Answer, Line};
use crate::core::Core;
use crate::knowledge::promotion;
use crate::realm::change::done;
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
        done!(core.resolve(text, line.here.as_ref(), Reach::Kept));
    done!(core.leads(hire));
    let layout = core.realm.layout().clone();
    let chief = core.realm.chief();
    let make = promotion::make_scholar(
        core.realm.store().connection(),
        &layout,
        &core.writing,
        hire,
        &line.tail(1),
        chief,
        String::new(),
    )?;
    let made = core.attempt(Actor::Chief(chief), |cx, _| make(cx))?;
    let db = core.realm.store().connection();
    Ok(match made {
        Ok(made) => Ok(format!(
            "{} lives in the city, made of what `{text}` learned.",
            city::get(db, made.scholar)?.label()
        )),
        Err(refusal) => Err(refusal),
    })
}

//! The Hand judging a newcomer for the chiefs (H45): an errand that
//! answers whether a profile an agent made moves into the city, its
//! verdict kept in the record for a chief to undo.

use anyhow::Result;
use hamlet_store::ids::{Actor, ProfileId};

use crate::conversations::errands::{self, Ended, Errand};
use crate::conversations::tasks::{self, Origin};
use crate::realm::change::Change;
use crate::realm::settings::EffectiveSettings;
use crate::society::{addresses, city, hand};

/// The errand the Hand's verdicts on newcomers are.
pub(crate) const WELCOME: &str = "welcome";

/// The Hand's part before every verdict.
const ROLE: &str =
    "You are the Hand, deciding for the chiefs which newcomers move \
     into the city. The city is where the realm's profiles live \
     between their work: any hamlet can add an agent of a profile \
     there, and what it learns stays with it. Let a newcomer in \
     when it is a profile the realm will want again, distinct from \
     those the city has. Turn it away when it repeats one of them, \
     is too narrow to keep, or is unclear. A newcomer turned away \
     still works for the hamlet that made it. The chiefs can undo \
     what you decide.";

/// The owner of the errand that judges the newcomer `id`.
fn owner(id: ProfileId) -> String {
    format!("newcomer:{id}")
}

/// Puts to the Hand whether the newcomer `id`, made from `character`,
/// moves into the city.
pub(crate) fn judge(
    cx: &mut Change<'_>,
    env: &EffectiveSettings,
    id: ProfileId,
    character: &str,
) -> Result<()> {
    let newcomer = city::get(cx.db, id)?;
    let maker = addresses::actor_name(cx.db, Some(newcomer.made_by))?;
    let living = city::all(cx.db)?
        .into_iter()
        .filter(|profile| {
            profile.is_living()
                && profile.id != id
                && profile.arrival == city::Arrival::Housed
        })
        .map(|profile| format!("- {}", profile.label()))
        .collect::<Vec<_>>();
    let city = if living.is_empty() {
        "(nobody yet)".to_owned()
    } else {
        living.join("\n")
    };
    let instructions = format!("{ROLE}\n\n# The city\n{city}");
    let question = format!(
        "{maker} made the profile `{}`, whose character \
         is:\n\n{}\n\nShould it move into the city? Answer ACCEPT \
         or TURN AWAY on the first line, then say why in one line.",
        newcomer.label(),
        character.trim()
    );
    let the_hand = hand::id(cx.db)?;
    let task = tasks::mint(
        cx,
        Origin::Errand,
        the_hand,
        None,
        Some(&format!("judge the newcomer {}", newcomer.label())),
    )?;
    errands::run(
        cx,
        env,
        Errand {
            kind: WELCOME,
            owner: &owner(id),
            agent: the_hand,
            instructions: &instructions,
            question: &question,
            task: Some(task),
        },
    )?;
    Ok(())
}

/// Carries out the Hand's verdict on the newcomer its errand `owner`
/// judged. A verdict that is neither is no verdict: the newcomer
/// waits for a chief.
pub(crate) fn judged(
    cx: &mut Change<'_>,
    owner: &str,
    ended: Ended,
) -> Result<()> {
    let Some(id) = owner
        .strip_prefix("newcomer:")
        .and_then(|id| id.parse::<i64>().ok())
    else {
        return Ok(());
    };
    let id = ProfileId(id);
    let Ended::Answered(answer) = ended else {
        return Ok(());
    };
    let mut lines = answer.lines();
    let verdict =
        lines.next().unwrap_or_default().trim().to_uppercase();
    let reason =
        lines.collect::<Vec<_>>().join(" ").trim().to_owned();
    // The record names the Hand as who decided.
    let was = cx.actor;
    cx.actor = Actor::Agent(hand::id(cx.db)?);
    // A newcomer let in or turned away since stays as it is.
    let _ = if verdict.starts_with("ACCEPT") {
        city::welcome(cx, id, &reason)?
    } else if verdict.starts_with("TURN AWAY") {
        city::turn_away(cx, id, &reason)?
    } else {
        Ok(())
    };
    cx.actor = was;
    Ok(())
}

//! Chief-controlled founding, placement and closure of hamlets.

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;

use super::{At, Removed, read, write};
use crate::Outcome;
use crate::harness::Harnesses;
use crate::papers::Capability;
use crate::protocol::Refusal;
use crate::record::{self, AgentRef, Audience, FoundRecord};
use crate::service::Service;
use crate::society::{self, Deed};

pub(crate) async fn deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    deed: &str,
    rest: &str,
) -> Result<Option<Deed>> {
    if ![
        "found",
        "hamlets",
        "name-hamlet",
        "move-hamlet",
        "remove-hamlet",
        "restore-hamlet",
        "erase-hamlet",
    ]
    .contains(&deed)
    {
        return Ok(None);
    }
    let bad = |text: String| Ok(Some(Err(Refusal::Invalid(text))));
    let needs = match deed {
        "found" => Some(Capability::FoundHamlet),
        "move-hamlet" => Some(Capability::MoveHamlet),
        "remove-hamlet" => Some(Capability::RemoveHamlet),
        "restore-hamlet" => Some(Capability::RestoreHamlet),
        "erase-hamlet" => Some(Capability::EraseHamlet),
        _ => None,
    };
    if let Some(capability) = needs
        && let Err(words) = service.houses.allows(capability)
    {
        return bad(words);
    }
    let mut realm = read(service.home.store().connection())?;
    if deed == "hamlets" {
        return Ok(Some(Ok(realm
            .live()
            .map(|h| {
                format!(
                    "- {} (`{}`)",
                    h.title.as_ref().unwrap_or(&h.id),
                    h.id
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))));
    }
    let words = rest.split_whitespace().collect::<Vec<_>>();
    let id = words.first().copied().unwrap_or_default();
    let cap = service.limits_of("home")?.hamlets.most as usize;
    if deed == "found" {
        if words.len() < 3 {
            return bad("Use /found <hamlet> <mayor> <purpose>; \
                        give the new mayor a purpose."
                .into());
        }
        if realm.hamlets.iter().any(|h| h.id == id) {
            return bad(format!(
                "the hamlet name `{id}` was used before; choose \
                 another"
            ));
        }
        if realm.live().count() >= cap {
            return bad("no more hamlets may be founded under the \
                        chief's limits"
                .into());
        }
        let operation = FoundRecord {
            hamlet: id.into(),
            name: words[1].into(),
            purpose: words[2..].join(" "),
            chief: service.chief.clone(),
            at: crate::clock::now(),
        };
        let tx = service.home.store().transaction()?;
        let result = crate::operation::change(&tx, &operation)?;
        match result {
            Outcome::Refused(refusal) => {
                return bad(refusal.text(Audience::Chief));
            }
            Outcome::Done(_) => {
                let realm = read(&tx)?;
                write(&tx, &realm)?;
                tx.commit()?;
            }
        }
        service.houses.initialize(id, 0).await?;
        society::reconfigure(service, harnesses).await?;
        let notice = format!(
            "The hamlet `{id}` was founded under the mayor `{}`. \
             /hamlet {id} talks to it.",
            words[1]
        );
        crate::chats::tell_chiefs(service, harnesses, id, &notice)
            .await?;
        return Ok(Some(Ok(format!(
            "The hamlet `{id}` is founded under the mayor `{}`, and \
             it is yours. /hamlet {id} talks to it.",
            words[1]
        ))));
    }
    let Some(index) =
        realm.hamlets.iter().position(|h| h.id == id && !h.erased)
    else {
        return bad(format!("no hamlet named `{id}`"));
    };
    let r = service
        .home
        .store()
        .record(id)?
        .context("the placed hamlet has no record")?;
    if !r.chiefs.is_empty() && !r.chiefs.contains(&service.chief) {
        return bad("Only a chief of that hamlet may do that.".into());
    }
    let closed = realm.hamlets[index].removed.is_some();
    if closed != ["restore-hamlet", "erase-hamlet"].contains(&deed) {
        return bad(if closed {
            format!("the hamlet `{id}` was removed; restore it first")
        } else {
            format!(
                "the hamlet `{id}` is still living; remove it first"
            )
        });
    }
    let text = match deed {
        "name-hamlet" => {
            let title = words[1..].join(" ");
            if title.chars().count() > 32 {
                return bad("A hamlet's name may have at most 32 \
                            letters."
                    .into());
            }
            realm.hamlets[index].title =
                (!title.is_empty()).then_some(title.clone());
            format!(
                "The hamlet `{id}` is shown as {}.",
                if title.is_empty() { id } else { &title }
            )
        }
        "move-hamlet" => {
            let at = if words.len() == 3 {
                words[1]
                    .parse::<f64>()
                    .ok()
                    .zip(words[2].parse::<f64>().ok())
                    .filter(|(x, z)| x.is_finite() && z.is_finite())
            } else {
                None
            };
            let Some((x, z)) = at else {
                return bad("Use /move-hamlet <hamlet> <x> <z> \
                            with finite coordinates."
                    .into());
            };
            realm.hamlets[index].at = Some(At { x, z });
            format!("The town of `{id}` is set down at {x}, {z}.")
        }
        "remove-hamlet" => {
            if id == "home" {
                return bad(
                    "the first hamlet cannot be removed".into()
                );
            }
            realm.hamlets[index].removed = Some(Removed {
                at: crate::clock::now(),
                by: service.chief.clone(),
                reason: words[1..].join(" "),
            });
            for agent in &r.agents {
                crate::chats::stop_agent(
                    service,
                    harnesses,
                    &AgentRef {
                        hamlet: id.into(),
                        number: agent.number,
                    },
                )
                .await?;
            }
            format!(
                "The hamlet `{id}` was removed and its houses \
                 archived."
            )
        }
        "restore-hamlet" => {
            if realm.live().count() >= cap {
                return bad("no more hamlets may be restored under \
                            the chief's limits"
                    .into());
            }
            for agent in &r.agents {
                if service
                    .home
                    .store()
                    .connection()
                    .query_row(
                        "SELECT 1 FROM record_name WHERE name = ?1",
                        [&agent.name],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some()
                {
                    return bad(format!(
                        "the name `{}` already belongs to a living \
                         agent",
                        agent.name
                    ));
                }
            }
            realm.hamlets[index].removed = None;
            realm.removals.retain(|ask| ask.hamlet != id);
            format!("The hamlet `{id}` is restored.")
        }
        "erase-hamlet" => {
            realm.hamlets[index].erased = true;
            format!(
                "The hamlet `{id}` was deleted for good. It cannot \
                 be restored."
            )
        }
        _ => unreachable!("realm deed checked above"),
    };
    let tx = service.home.store().transaction()?;
    write(&tx, &realm)?;
    if realm.lives(id) {
        record::claim_names(&tx, id)?;
    } else {
        record::release_names(&tx, id)?;
    }
    tx.commit()?;
    if realm.hamlets[index].erased {
        service.houses.erase_hamlet(id)?;
    }
    society::reconfigure(service, harnesses).await?;
    Ok(Some(Ok(text)))
}

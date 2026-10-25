//! Chief-controlled founding, placement and closure of hamlets.

use anyhow::{Context, Result};
use hamlet_protocol::client::{FoundFrom, Settler};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{At, Removed, read, write};
use crate::harness::Harnesses;
use crate::operation::Request;
use crate::papers::Capability;
use crate::protocol::{Refusable, Refusal};
use crate::record::{
    self, Actor, Admit, AgentRef, Audience, Change, FoundRecord,
    RecordOperation, RecordResult, tidy_name,
};
use crate::service::Service;
use crate::society::{self, Deed};
use crate::{OperationId, Outcome, RuntimeInstance};

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
        && let Err(refusal) = service.houses.allows(capability)
    {
        return Ok(Some(Err(refusal)));
    }
    let mut realm = read(service.db())?;
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
            return Ok(Some(Err(Refusal::Usage(
                "Use /found <hamlet> <mayor> <purpose>; give the \
                 new mayor a purpose."
                    .into(),
            ))));
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
                return Ok(Some(Err(Refusal::Record(Box::new(
                    refusal,
                )))));
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
        return Ok(Some(Err(Refusal::NoHamlet(id.into()))));
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
                return Ok(Some(Err(Refusal::Usage(
                    "Use /move-hamlet <hamlet> <x> <z> with finite \
                     coordinates."
                        .into(),
                ))));
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
                    .db()
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
    if realm.is_living(id) {
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

/// Why a hamlet was not founded from a template.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error,
)]
pub(crate) enum Unfounded {
    #[error("the hamlet name `{0}` was used before; choose another")]
    Used(String),
    #[error(
        "no more hamlets may be founded under the chief's limits"
    )]
    Full,
    #[error("the template brings an agent named `{0}` twice")]
    Twice(String),
    #[error(
        "`{0}` cannot name a skill: use letters, digits, hyphens \
         and underscores, and name each skill of an agent once"
    )]
    Skill(String),
    /// Boxed, as [`Refusal::Record`] is.
    #[error("{}", .0.text(Audience::Chief))]
    Record(Box<record::Refusal>),
}

/// The hamlet a template founded, and the number each of its agents
/// took, the mayor first.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Founded {
    numbers: Vec<u64>,
}

/// The chief's `found-from`: the template's hamlet founded with its
/// mayor and agents, their characters in the record, then their notes
/// and skills in their houses. The record is written whole or not at
/// all, as one marked operation of the request.
pub(crate) async fn found_from(
    service: &mut Service,
    harnesses: &mut Harnesses,
    found: &FoundFrom,
) -> Result<Refusable<String>> {
    if let Err(refusal) =
        service.houses.allows(Capability::FoundHamlet)
    {
        return Ok(Err(refusal));
    }
    let template = &found.template;
    let hamlet = template.name.as_str();
    let id = OperationId {
        runtime: RuntimeInstance::new("chief")?,
        key: found.id.to_string(),
    };
    let request = Request::new("found-from", found)?;
    let cap = service.limits_of("home")?.hamlets.most as usize;
    let (chief, at) = (service.chief.clone(), crate::clock::now());
    let mut fresh = false;
    let outcome = service.home.store().once(&id, &request, |db| {
        fresh = true;
        db.execute_batch("SAVEPOINT founding")?;
        let founded = settle(db, found, cap, &chief, at);
        if matches!(founded, Ok(Ok(_))) {
            db.execute_batch("RELEASE founding")?;
        } else {
            db.execute_batch(
                "ROLLBACK TO founding; RELEASE founding",
            )?;
        }
        founded
    })?;
    let numbers = match outcome {
        None => return Ok(Err(Refusal::Reused)),
        Some(Err(unfounded)) => {
            return Ok(Err(Refusal::Unfounded(unfounded)));
        }
        Some(Ok(founded)) => founded.numbers,
    };
    let everyone =
        std::iter::once(&template.mayor).chain(&template.agents);
    if fresh {
        for (settler, number) in everyone.zip(numbers) {
            service.houses.initialize(hamlet, number).await?;
            move_in(service, hamlet, number, settler).await?;
        }
        society::reconfigure(service, harnesses).await?;
        let notice = format!(
            "The hamlet `{hamlet}` was founded from a template \
             under the mayor `{}`. /hamlet {hamlet} talks to it.",
            template.mayor.name
        );
        crate::chats::tell_chiefs(
            service, harnesses, hamlet, &notice,
        )
        .await?;
    }
    Ok(Ok(format!(
        "The hamlet `{hamlet}` is founded under the mayor `{}`, \
         with {} agents besides, and it is yours. /hamlet {hamlet} \
         talks to it.",
        template.mayor.name,
        template.agents.len()
    )))
}

/// Founds the template's hamlet in the record and the realm, in the
/// transaction `db`, which its caller rolls back when it refuses.
fn settle(
    db: &Connection,
    found: &FoundFrom,
    cap: usize,
    chief: &str,
    at: i64,
) -> Result<Result<Founded, Unfounded>> {
    let template = &found.template;
    let hamlet = template.name.as_str();
    let realm = read(db)?;
    if realm.hamlets.iter().any(|h| h.id == hamlet) {
        return Ok(Err(Unfounded::Used(hamlet.into())));
    }
    if realm.live().count() >= cap {
        return Ok(Err(Unfounded::Full));
    }
    let everyone =
        || std::iter::once(&template.mayor).chain(&template.agents);
    let mut names = Vec::new();
    for settler in everyone() {
        let name = tidy_name(&settler.name);
        if names.contains(&name) {
            return Ok(Err(Unfounded::Twice(name)));
        }
        names.push(name);
        let mut skills = Vec::new();
        for skill in &settler.skills {
            if !society::library::is_valid(&skill.name)
                || skills.contains(&&skill.name)
            {
                return Ok(Err(Unfounded::Skill(skill.name.clone())));
            }
            skills.push(&skill.name);
        }
    }
    let refused =
        |refusal| Ok(Err(Unfounded::Record(Box::new(refusal))));
    let operation = FoundRecord {
        hamlet: hamlet.into(),
        name: template.mayor.name.clone(),
        purpose: template.purpose.clone(),
        chief: chief.into(),
        at,
    };
    let mayor = match crate::operation::change(db, &operation)? {
        Outcome::Refused(refusal) => return refused(refusal),
        Outcome::Done(mayor) => mayor,
    };
    let change = |change| RecordOperation {
        hamlet: hamlet.into(),
        actor: Actor::Chief(chief.into()),
        at,
        reason: "came with the hamlet, founded from a template"
            .into(),
        task: None,
        change,
    };
    let mut numbers = vec![mayor.number];
    if !template.mayor.character.trim().is_empty() {
        let character = change(Change::SetCharacter {
            number: mayor.number,
            text: template.mayor.character.clone(),
        });
        if let Outcome::Refused(refusal) =
            crate::operation::change(db, &character)?
        {
            return refused(refusal);
        }
    }
    for settler in &template.agents {
        let admit = change(Change::Admit(Admit {
            name: settler.name.clone(),
            purpose: settler.purpose.clone(),
            model: None,
            character: settler.character.clone(),
            roads: Vec::new(),
        }));
        match crate::operation::change(db, &admit)? {
            Outcome::Done(RecordResult::Admitted {
                agent, ..
            }) => {
                numbers.push(agent.number);
            }
            Outcome::Done(other) => {
                anyhow::bail!("an agent admitted as {other:?}")
            }
            Outcome::Refused(refusal) => return refused(refusal),
        }
    }
    let mut realm = read(db)?;
    if let Some(place) =
        realm.hamlets.iter_mut().find(|h| h.id == hamlet)
    {
        place.title = (!template.title.trim().is_empty())
            .then(|| template.title.trim().to_owned());
    }
    write(db, &realm)?;
    Ok(Ok(Founded { numbers }))
}

/// Writes what `settler` knows into the house of agent `number` of
/// `hamlet`: its notes and its skills.
async fn move_in(
    service: &Service,
    hamlet: &str,
    number: u64,
    settler: &Settler,
) -> Result<()> {
    if !settler.notes.trim().is_empty() {
        service
            .houses
            .write_notes(hamlet, number, &settler.notes)
            .await?;
    }
    let house = service.houses.house(hamlet, number)?;
    for skill in &settler.skills {
        // One line, so the front matter stays whole.
        let description =
            skill.description.split_whitespace().collect::<Vec<_>>();
        let paper = format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}\n",
            skill.name,
            description.join(" "),
            skill.body.trim()
        );
        house
            .put(
                &format!("skills/{}/SKILL.md", skill.name),
                &crate::papers::tree::Entry {
                    bytes: paper.into_bytes(),
                    mode: 0o644,
                },
            )
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client::{
        RequestId, SettlerSkill, Template,
    };
    use hamlet_testing::Scratch;

    use super::*;
    use crate::{Home, Overrides};

    fn settler(name: &str, skill: &str) -> Settler {
        Settler {
            character: format!("{name} is calm."),
            name: name.into(),
            notes: format!("# Notes of {name}\n"),
            purpose: format!("{name} works."),
            skills: vec![SettlerSkill {
                body: "Step by step.".into(),
                description: "Does it.".into(),
                name: skill.into(),
            }],
        }
    }

    fn template(name: &str, agents: &[&str]) -> FoundFrom {
        FoundFrom {
            id: RequestId::new(),
            template: Template {
                agents: agents
                    .iter()
                    .map(|agent| settler(agent, "dig"))
                    .collect(),
                mayor: settler(&format!("{name}-lead"), "plan"),
                name: name.into(),
                purpose: "Do the work.".into(),
                title: format!("The {name}"),
            },
        }
    }

    #[tokio::test]
    async fn a_hamlet_from_a_template_is_founded_whole_and_only_once()
    {
        let scratch = Scratch::new("realm-template");
        let mut service = Service::new(
            Home::open(&scratch).unwrap(),
            Overrides::none(true),
        )
        .unwrap();
        let layout = service.home.layout().clone();
        let (mut harnesses, _) = Harnesses::start(
            &layout,
            service.home.store(),
            crate::testing::config(Overrides::none(true)),
        )
        .unwrap();

        // An agent's name is taken in the realm, or a skill's would
        // leave its folder: nothing is founded.
        let taken = template("quarry", &["founder"]);
        assert!(matches!(
            found_from(&mut service, &mut harnesses, &taken)
                .await
                .unwrap(),
            Err(Refusal::Unfounded(Unfounded::Record(_)))
        ));
        let mut climbing = template("quarry", &["miner"]);
        climbing.template.agents[0].skills[0].name = "../out".into();
        assert!(matches!(
            found_from(&mut service, &mut harnesses, &climbing)
                .await
                .unwrap(),
            Err(Refusal::Unfounded(Unfounded::Skill(_)))
        ));
        let twice = template("quarry", &["miner", "miner"]);
        assert!(matches!(
            found_from(&mut service, &mut harnesses, &twice)
                .await
                .unwrap(),
            Err(Refusal::Unfounded(Unfounded::Twice(_)))
        ));

        // Another stands whole, under the name it is shown by.
        let mill = template("mill", &["miller", "carter"]);
        assert!(
            found_from(&mut service, &mut harnesses, &mill)
                .await
                .unwrap()
                .is_ok()
        );
        assert!(
            found_from(&mut service, &mut harnesses, &mill)
                .await
                .unwrap()
                .is_ok()
        );
        let realm = read(service.db()).unwrap();
        assert!(realm.hamlets.iter().all(|h| h.id != "quarry"));
        let place =
            realm.hamlets.iter().find(|h| h.id == "mill").unwrap();
        assert_eq!(place.title.as_deref(), Some("The mill"));
        let record =
            service.home.store().record("mill").unwrap().unwrap();
        assert_eq!(record.agents.len(), 3);
        for agent in &record.agents {
            assert_eq!(
                agent.character,
                format!("{} is calm.", agent.name)
            );
            let notes = service
                .houses
                .read_notes("mill", agent.number)
                .await
                .unwrap();
            assert_eq!(notes, format!("# Notes of {}\n", agent.name));
            let skill = if agent.number == record.mayor {
                "plan"
            } else {
                "dig"
            };
            let paper = service
                .houses
                .house("mill", agent.number)
                .unwrap()
                .read_file(&format!("skills/{skill}/SKILL.md"), 4096)
                .await
                .unwrap();
            assert!(String::from_utf8(paper).unwrap().starts_with(
                &format!(
                    "---\nname: {skill}\ndescription: Does it.\n---"
                )
            ));
        }
        // Founded again under another request, it is refused.
        let again = template("mill", &["miller", "carter"]);
        assert!(matches!(
            found_from(&mut service, &mut harnesses, &again)
                .await
                .unwrap(),
            Err(Refusal::Unfounded(Unfounded::Used(_)))
        ));
    }
}

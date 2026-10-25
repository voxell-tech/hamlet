//! A package bought at the bazaar's server, settled into the home: an
//! agent moved into a hamlet or brought up to a new version, a hamlet
//! founded or brought up, or tools added; and the record of what was
//! bought.

use std::path::Path;

use anyhow::{Result, bail};
use hamlet_protocol::client::{Bought, Settle, Settler, Template};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::package::{
    self, Agent, CHARACTER, Contents, NOTES, Package, SKILLS, Tools,
};
use crate::harness::Harnesses;
use crate::operation::{Marked, Request};
use crate::papers::Capability;
use crate::papers::tree::Entry;
use crate::protocol::{Refusable, Refusal};
use crate::record::{
    self, Actor, Admit, Audience, Change, RecordOperation,
    RecordResult,
};
use crate::service::Service;
use crate::{OperationId, Outcome, RuntimeInstance, society};

/// The kind of the chief documents that keep what was bought, by
/// listing.
const BOUGHT: &str = "bazaar-bought";

/// The reason recorded for an agent bought.
const REASON: &str = "bought at the Agent Bazaar";

/// What is added to an agent's name, in turn, when its own is taken.
const OTHER_NAMES: [&str; 9] =
    ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];

/// Why a package bought was not settled into the home.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error,
)]
pub(crate) enum Unsettled {
    #[error(
        "the package's bytes are not those its settle announced; \
         download it again"
    )]
    Mismatch,
    #[error("the package cannot be read: {0}")]
    Unreadable(String),
    #[error("choose the hamlet the agent moves into")]
    NoHamlet,
    #[error("a package of a hamlet or tools moves into no hamlet")]
    Into,
    #[error("an agent named `{0}` already lives in the realm")]
    Named(String),
    #[error("the realm has a hamlet named `{0}` already")]
    HamletTaken(String),
    #[error("the home has a tool named `{0}` already")]
    ToolTaken(String),
    #[error("{0}")]
    Unfounded(crate::realm::Unfounded),
    /// Boxed, as [`Refusal::Record`] is.
    #[error("{}", .0.text(Audience::Chief))]
    Record(Box<record::Refusal>),
}

/// What a settle decided in the record, kept with its marker: its
/// words, and the houses its agents move into.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Settled {
    words: String,
    houses: Vec<Housing>,
    /// The hamlet founded, whose chiefs are told.
    founded: Option<String>,
}

/// The house of agent `number` of `hamlet`, where the package's agent
/// `agent` lives, the mayor first.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Housing {
    hamlet: String,
    number: u64,
    agent: usize,
    /// New, rather than brought up to the version.
    fresh: bool,
}

/// The chief's `settle` of `bytes`: the package checked, decided in
/// the record with what was bought as one marked operation of the
/// request, then its files written into the houses and the home.
pub(crate) async fn settle(
    service: &mut Service,
    harnesses: &mut Harnesses,
    settle: &Settle,
    bytes: &[u8],
) -> Result<Refusable<String>> {
    let refused = |unsettled| Ok(Err(Refusal::Unsettled(unsettled)));
    if bytes.len() as u64 != settle.size
        || format!("{:x}", Sha256::digest(bytes)) != settle.sha256
    {
        return refused(Unsettled::Mismatch);
    }
    let package = match Package::read(bytes) {
        Ok(package) => package,
        Err(error) => {
            return refused(Unsettled::Unreadable(error.to_string()));
        }
    };
    if matches!(package.contents, Contents::Hamlet { .. })
        && let Err(refusal) =
            service.houses.allows(Capability::FoundHamlet)
    {
        return Ok(Err(refusal));
    }
    let (id, request) = marker(settle)?;
    let cap = service.limits_of("home")?.hamlets.most as usize;
    let (chief, at) = (service.chief.clone(), crate::clock::now());
    let tools = tools_folder(service);
    let mut fresh = false;
    let outcome = service.home.store().once(&id, &request, |db| {
        fresh = true;
        db.execute_batch("SAVEPOINT settling")?;
        let decided = decide(
            db,
            &Deal {
                settle,
                package: &package,
                tools: &tools,
                cap,
                chief: &chief,
                at,
            },
        );
        if matches!(decided, Ok(Ok(_))) {
            db.execute_batch("RELEASE settling")?;
        } else {
            db.execute_batch(
                "ROLLBACK TO settling; RELEASE settling",
            )?;
        }
        decided
    })?;
    let settled = match outcome {
        None => return Ok(Err(Refusal::Reused)),
        Some(Err(unsettled)) => return refused(unsettled),
        Some(Ok(settled)) => settled,
    };
    if fresh {
        write(service, &package, &settled).await?;
        if let Contents::Tools { tools: bought } = &package.contents {
            install_tools(&tools, bought)?;
        }
        society::reconfigure(service, harnesses).await?;
        if let Some(hamlet) = &settled.founded {
            let notice = format!(
                "The hamlet `{hamlet}` was bought at the Agent \
                 Bazaar and founded. /hamlet {hamlet} talks to it."
            );
            crate::chats::tell_chiefs(
                service, harnesses, hamlet, &notice,
            )
            .await?;
        }
    }
    Ok(Ok(settled.words))
}

/// The marker of `settle`, and the request it keeps.
pub(super) fn marker(
    settle: &Settle,
) -> Result<(OperationId, Request)> {
    Ok((
        OperationId {
            runtime: RuntimeInstance::new("chief")?,
            key: settle.id.to_string(),
        },
        Request::new("settle", settle)?,
    ))
}

/// The words a settle already decided under its marker; a refusal
/// when the marker is another request's; `None` when it is not
/// settled yet.
pub(super) fn settled(
    service: &Service,
    settle: &Settle,
) -> Result<Option<Refusable<String>>> {
    let (id, request) = marker(settle)?;
    let db = service.db();
    let kept = crate::operation::marked::<
        std::result::Result<Settled, Unsettled>,
    >(db, &id, &request)?;
    let other = kept.is_none()
        && crate::operation::is_marked(
            db,
            id.runtime.as_str(),
            &id.key,
        )?;
    Ok(match kept {
        Some(Marked::Outcome(outcome)) => {
            Some(outcome.map(|s| s.words).map_err(Refusal::Unsettled))
        }
        Some(Marked::Conflict) => Some(Err(Refusal::Reused)),
        _ if other => Some(Err(Refusal::Reused)),
        Some(Marked::Begun) | None => None,
    })
}

/// The home's folder of the tools added by hand.
fn tools_folder(service: &Service) -> std::path::PathBuf {
    service.home.layout().root().join("tools")
}

/// What a settle decides from.
struct Deal<'a> {
    settle: &'a Settle,
    package: &'a Package,
    tools: &'a Path,
    /// The most hamlets the chief may have.
    cap: usize,
    chief: &'a str,
    at: i64,
}

/// Decides `deal` in the record, in the transaction `db`, which its
/// caller rolls back when it refuses: agents admitted or their
/// characters brought up, a hamlet founded, tools' names checked,
/// and what was bought kept.
fn decide(
    db: &Connection,
    deal: &Deal,
) -> Result<Result<Settled, Unsettled>> {
    let Deal {
        settle, package, ..
    } = deal;
    let before = bought(db, &settle.listing)?
        .filter(|before| before.kind == package.kind());
    let version = &settle.version;
    let (settled, names, hamlet) =
        match (&package.contents, &settle.into) {
            (Contents::Agent { .. }, None) => {
                return Ok(Err(Unsettled::NoHamlet));
            }
            (
                Contents::Hamlet { .. } | Contents::Tools { .. },
                Some(_),
            ) => {
                return Ok(Err(Unsettled::Into));
            }
            (Contents::Agent { agent }, Some(into)) => {
                let still = before.as_ref().and_then(|before| {
                    Some((
                        before.hamlet.clone()?,
                        before.names.first()?,
                    ))
                });
                let living = match still {
                    Some((hamlet, name)) => {
                        record::read(db, &hamlet)?
                            .and_then(|record| {
                                record
                                    .agents
                                    .into_iter()
                                    .find(|a| &a.name == name)
                            })
                            .map(|agent| (hamlet, agent))
                    }
                    None => None,
                };
                let (hamlet, housing, words) = match living {
                    Some((hamlet, living)) => {
                        if let Err(refusal) = bring_up(
                            db, deal, &hamlet, &living, agent,
                        )? {
                            return Ok(Err(refusal));
                        }
                        let words = format!(
                            "{} is up to version {version}.",
                            living.name
                        );
                        (
                            hamlet.clone(),
                            Housing {
                                hamlet,
                                number: living.number,
                                agent: 0,
                                fresh: false,
                            },
                            words,
                        )
                    }
                    None => {
                        let Some(name) = free_name(db, &agent.name)?
                        else {
                            return Ok(Err(Unsettled::Named(
                                agent.name.clone(),
                            )));
                        };
                        let number = match admit(
                            db, deal, into, &name, agent,
                        )? {
                            Ok(number) => number,
                            Err(refusal) => return Ok(Err(refusal)),
                        };
                        let words =
                            format!("{name} moved into {into}.");
                        (
                            into.clone(),
                            Housing {
                                hamlet: into.clone(),
                                number,
                                agent: 0,
                                fresh: true,
                            },
                            words,
                        )
                    }
                };
                let name = record::read(db, &hamlet)?
                    .and_then(|record| {
                        record
                            .agents
                            .into_iter()
                            .find(|a| a.number == housing.number)
                    })
                    .map(|agent| agent.name)
                    .unwrap_or_default();
                (
                    Settled {
                        words,
                        houses: vec![housing],
                        founded: None,
                    },
                    vec![name],
                    Some(hamlet),
                )
            }
            (Contents::Hamlet { hamlet: packed }, None) => {
                let name = packed.name.as_str();
                let stands = crate::realm::read(db)?
                    .live()
                    .any(|place| place.id == name);
                let ours = before.as_ref().is_some_and(|b| {
                    b.names.first().map(String::as_str) == Some(name)
                });
                let settled = match (stands, ours) {
                    (true, true) => {
                        let mut houses = Vec::new();
                        let record = record::read(db, name)?;
                        for (index, agent) in
                            packed.everyone().enumerate()
                        {
                            let living =
                                record.as_ref().and_then(|r| {
                                    r.agents.iter().find(|a| {
                                        a.name == agent.name
                                    })
                                });
                            let housing = match living {
                                Some(living) => {
                                    if let Err(refusal) = bring_up(
                                        db, deal, name, living, agent,
                                    )? {
                                        return Ok(Err(refusal));
                                    }
                                    Housing {
                                        hamlet: name.into(),
                                        number: living.number,
                                        agent: index,
                                        fresh: false,
                                    }
                                }
                                None => {
                                    let number = match admit(
                                        db,
                                        deal,
                                        name,
                                        &agent.name,
                                        agent,
                                    )? {
                                        Ok(number) => number,
                                        Err(refusal) => {
                                            return Ok(Err(refusal));
                                        }
                                    };
                                    Housing {
                                        hamlet: name.into(),
                                        number,
                                        agent: index,
                                        fresh: true,
                                    }
                                }
                            };
                            houses.push(housing);
                        }
                        Settled {
                            words: format!(
                                "{} is up to version {version}.",
                                packed.title
                            ),
                            houses,
                            founded: None,
                        }
                    }
                    (true, false) => {
                        return Ok(Err(Unsettled::HamletTaken(
                            name.into(),
                        )));
                    }
                    (false, _) => {
                        let template = template(packed);
                        let founded = crate::realm::found_in(
                            db, &template, deal.cap, deal.chief,
                            deal.at,
                        )?;
                        let numbers = match founded {
                            Ok(founded) => founded.numbers,
                            Err(unfounded) => {
                                return Ok(Err(
                                    Unsettled::Unfounded(unfounded),
                                ));
                            }
                        };
                        Settled {
                            words: format!(
                                "{} stands in your realm, with {} \
                                 and all they know.",
                                packed.title, packed.mayor.name
                            ),
                            houses: numbers
                                .into_iter()
                                .enumerate()
                                .map(|(agent, number)| Housing {
                                    hamlet: name.into(),
                                    number,
                                    agent,
                                    fresh: true,
                                })
                                .collect(),
                            founded: Some(name.into()),
                        }
                    }
                };
                (settled, vec![name.to_owned()], None)
            }
            (Contents::Tools { tools }, None) => {
                let replacing = before
                    .as_ref()
                    .map(|before| before.names.clone())
                    .unwrap_or_default();
                for tool in &tools.tools {
                    if deal.tools.join(&tool.name).exists()
                        && !replacing.contains(&tool.name)
                    {
                        return Ok(Err(Unsettled::ToolTaken(
                            tool.name.clone(),
                        )));
                    }
                }
                let names = tools
                    .tools
                    .iter()
                    .map(|tool| tool.name.clone())
                    .collect::<Vec<_>>();
                let words = match &before {
                    Some(_) => format!(
                        "{} is up to version {version}.",
                        names.join(", ")
                    ),
                    None => format!(
                        "Every agent can use {} now.",
                        names.join(", ")
                    ),
                };
                (
                    Settled {
                        words,
                        houses: Vec::new(),
                        founded: None,
                    },
                    names,
                    None,
                )
            }
        };
    keep(
        db,
        &Bought {
            hamlet,
            kind: package.kind(),
            listing: settle.listing.clone(),
            names,
            title: settle.title.clone(),
            version: settle.version.clone(),
        },
    )?;
    Ok(Ok(settled))
}

/// The first of `name`, `name-ii`, `name-iii`... no living agent of
/// the realm has.
fn free_name(db: &Connection, name: &str) -> Result<Option<String>> {
    let candidates = std::iter::once(name.to_owned())
        .chain(OTHER_NAMES.iter().map(|n| format!("{name}-{n}")));
    for candidate in candidates {
        let taken = db
            .query_row(
                "SELECT 1 FROM record_name WHERE name = ?1",
                [&candidate],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !taken {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// A change of the record by the chief, for what was bought.
fn by_chief(
    deal: &Deal,
    hamlet: &str,
    change: Change,
) -> RecordOperation {
    RecordOperation {
        hamlet: hamlet.into(),
        actor: Actor::Chief(deal.chief.into()),
        at: deal.at,
        reason: REASON.into(),
        task: None,
        change,
    }
}

/// Admits `agent` into `hamlet` under `name`, with its purpose and
/// character. Its number, or why the record refused it.
fn admit(
    db: &Connection,
    deal: &Deal,
    hamlet: &str,
    name: &str,
    agent: &Agent,
) -> Result<Result<u64, Unsettled>> {
    let admit = by_chief(
        deal,
        hamlet,
        Change::Admit(Admit {
            name: name.into(),
            purpose: purpose(agent),
            model: None,
            character: agent.text(CHARACTER).unwrap_or_default(),
            roads: Vec::new(),
        }),
    );
    Ok(match crate::operation::change(db, &admit)? {
        Outcome::Done(RecordResult::Admitted { agent, .. }) => {
            Ok(agent.number)
        }
        Outcome::Done(other) => {
            bail!("an agent admitted as {other:?}")
        }
        Outcome::Refused(refusal) => {
            Err(Unsettled::Record(Box::new(refusal)))
        }
    })
}

/// Brings the character of `living`, of `hamlet`, up to `agent`'s.
fn bring_up(
    db: &Connection,
    deal: &Deal,
    hamlet: &str,
    living: &record::Agent,
    agent: &Agent,
) -> Result<Result<(), Unsettled>> {
    let Some(character) = agent.text(CHARACTER) else {
        return Ok(Ok(()));
    };
    if character == living.character {
        return Ok(Ok(()));
    }
    let change = by_chief(
        deal,
        hamlet,
        Change::SetCharacter {
            number: living.number,
            text: character,
        },
    );
    Ok(match crate::operation::change(db, &change)? {
        Outcome::Done(_) => Ok(()),
        Outcome::Refused(refusal) => {
            Err(Unsettled::Record(Box::new(refusal)))
        }
    })
}

fn purpose(agent: &Agent) -> String {
    match agent.purpose.trim() {
        "" => "came from the Agent Bazaar".into(),
        purpose => purpose.into(),
    }
}

/// The template a hamlet bought is founded from. Its agents' notes
/// and skills are written apart, as a skill may hold any file.
fn template(packed: &package::Hamlet) -> Template {
    let settler = |agent: &Agent| Settler {
        character: agent.text(CHARACTER).unwrap_or_default(),
        name: agent.name.clone(),
        notes: String::new(),
        purpose: purpose(agent),
        skills: Vec::new(),
    };
    Template {
        agents: packed.agents.iter().map(settler).collect(),
        mayor: settler(&packed.mayor),
        name: packed.name.clone(),
        purpose: match packed.purpose.trim() {
            "" => "came from the Agent Bazaar".into(),
            purpose => purpose.into(),
        },
        title: match packed.title.trim() {
            "" => packed.name.clone(),
            title => title.into(),
        },
    }
}

/// Writes what each agent of `package` knows into the house
/// `settled` gave it.
async fn write(
    service: &Service,
    package: &Package,
    settled: &Settled,
) -> Result<()> {
    let agents = match &package.contents {
        Contents::Agent { agent } => vec![agent],
        Contents::Hamlet { hamlet } => hamlet.everyone().collect(),
        Contents::Tools { .. } => Vec::new(),
    };
    for housing in &settled.houses {
        let Some(agent) = agents.get(housing.agent) else {
            continue;
        };
        if housing.fresh {
            service
                .houses
                .initialize(&housing.hamlet, housing.number)
                .await?;
        }
        move_in(service, housing, agent).await?;
    }
    Ok(())
}

/// Writes `agent`'s notes and skills into the house of `housing`. The
/// skills it comes with replace those of the same name; the notes of
/// an agent brought up are its own memory and are kept, unless it
/// has none yet.
async fn move_in(
    service: &Service,
    housing: &Housing,
    agent: &Agent,
) -> Result<()> {
    let house =
        service.houses.house(&housing.hamlet, housing.number)?;
    if let Some(notes) = agent.text(NOTES) {
        let own = if housing.fresh {
            false
        } else {
            service
                .houses
                .read_notes(&housing.hamlet, housing.number)
                .await
                .is_ok_and(|own| {
                    !matches!(own.trim(), "" | "# Notes")
                })
        };
        if !own {
            service
                .houses
                .write_notes(&housing.hamlet, housing.number, &notes)
                .await?;
        }
    }
    let mut skills = Vec::<&str>::new();
    for file in &agent.files {
        if let Some((skill, _)) = file
            .path
            .strip_prefix(SKILLS)
            .and_then(|rest| rest.split_once('/'))
            && !skills.contains(&skill)
        {
            skills.push(skill);
        }
    }
    for skill in &skills {
        let folder = format!("skills/{skill}");
        if house.kind(&folder).await?.is_some() {
            house.remove(&folder, true).await?;
        }
    }
    for file in &agent.files {
        if let Some(rest) = file.path.strip_prefix(SKILLS) {
            house
                .put(&format!("skills/{rest}"), &entry(file))
                .await?;
        }
    }
    Ok(())
}

fn entry(file: &package::File) -> Entry {
    Entry {
        bytes: file.data.clone(),
        mode: if file.executable { 0o755 } else { 0o644 },
    }
}

/// Adds `bought` to the home's tools in `folder`, each replacing the
/// one of its name. Each is written beside its place first, so a tool
/// half written is never loaded.
fn install_tools(folder: &Path, bought: &Tools) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    for tool in &bought.tools {
        let dir = folder.join(&tool.name);
        let fresh = folder.join(format!(".{}.new", tool.name));
        if fresh.exists() {
            std::fs::remove_dir_all(&fresh)?;
        }
        for file in &tool.files {
            if !package::safe(&file.path) {
                bail!(
                    "`{}` is not a path a package may hold",
                    file.path
                );
            }
            let to = fresh.join(&file.path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&to, &file.data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = entry(file).mode;
                std::fs::set_permissions(
                    &to,
                    std::fs::Permissions::from_mode(mode),
                )?;
            }
        }
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::rename(&fresh, &dir)?;
    }
    Ok(())
}

/// What was bought as the listing `listing`, if it was.
fn bought(db: &Connection, listing: &str) -> Result<Option<Bought>> {
    crate::service::document(db, BOUGHT, listing)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn keep(db: &Connection, bought: &Bought) -> Result<()> {
    crate::service::save(
        db,
        BOUGHT,
        &bought.listing,
        &serde_json::to_value(bought)?,
    )
}

/// Everything bought and settled into the home, by listing.
pub(crate) fn all_bought(db: &Connection) -> Result<Vec<Bought>> {
    let mut statement = db.prepare(
        "SELECT value FROM chief_document WHERE kind = ?1 ORDER BY \
         key",
    )?;
    statement
        .query_map(params![BOUGHT], |row| row.get::<_, String>(0))?
        .map(|value| Ok(serde_json::from_str(&value?)?))
        .collect()
}

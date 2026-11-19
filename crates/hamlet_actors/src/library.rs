//! The `library` actor, `library/local`: the only writer of
//! `library.git`, of its checkout `library/` in the realm folder, and
//! of the store beside them.
//!
//! `main` holds each shelved skill as `<skill>/...` at its root, and
//! `offers/<offer>` the files of each offer. Every agent reads
//! `library/`; none writes it.

mod offers;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use hamlet_actor::{
    Actor, BindParam, ColumnValue, Ctx, Handler, Registry,
};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_git::{Change, MAIN};
use hamlet_host::{Caller, HostRef};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Agent, Party, Realm};
use hamlet_messages::library::{
    DropOfferParts, NoSkill, Offer, ReadOffer, ReadShelves,
    ShelfList, Shelves, ShelvesChanged, Skill, Unshelve, Verdict,
};
use hamlet_messages::realm::Done;
use hamlet_messages::workspace::AgentNumber;

use crate::repo::git::{By, Git, Held, Reads};
use crate::repo::store::{self, Store, Writes};
use crate::repo::{
    self, Admits, Before, Call, Ended, Owner, Reply, Repo, Takes,
    Writer,
};

const SCHEMA: &str = include_str!("library/schema.sql");

/// The library's repository, under the realm folder.
const REPOSITORY: &str = "library.git";

/// The checkout of `main` every agent reads, under the realm folder.
const CHECKOUT: &str = "library";

/// The file whose front matter tells of a skill.
const SKILL_FILE: &str = "SKILL.md";

macro_rules! starter {
    ($name:literal, [$($file:literal),+]) => {
        ($name, &[$((
            $file,
            include_str!(concat!(
                "../../hamlet_server/assets/starters/", $name, "/", $file
            )),
        )),+])
    };
}

/// A starter skill: its name and its files.
type Starter =
    (&'static str, &'static [(&'static str, &'static str)]);

const STARTERS: &[Starter] = &[
    starter!("web-search", ["SKILL.md", "scripts/search.py"]),
    starter!("read-webpage", ["SKILL.md", "scripts/read.py"]),
    starter!("weather", ["SKILL.md", "scripts/weather.py"]),
    starter!("call-api", ["SKILL.md", "scripts/api.py"]),
    starter!("system-info", ["SKILL.md", "scripts/info.py"]),
    starter!("write-skill", ["SKILL.md"]),
    starter!("manage-memory", ["SKILL.md"]),
    starter!("write-typst", ["SKILL.md"]),
];

struct Events(());

impl store::Table for Events {
    const NAME: &'static str = "shelf_event";
}

struct Listeners(());

impl store::Table for Listeners {
    const NAME: &'static str = "listener";
}

/// The factory of the `library` kind.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Library>(
        address::Library::KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(address::Library::KIND)
        .expect("the registry holds the kind it registered")
}

pub(crate) struct Library {
    repo: Repo,
}

#[async_trait::async_trait]
impl Actor for Library {
    type Input = ();
    type Actions = (
        Shelves,
        ReadShelves,
        Offer,
        ReadOffer,
        Verdict,
        Unshelve,
        DropOfferParts,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let path =
            HostRef::of(ctx.inner())?.folder().join(REPOSITORY);
        let repo =
            Repo::start(ctx, SCHEMA, path, starters, check_out)
                .await?;
        Ok(Self { repo })
    }
}

fn starters(git: &Git<'_>) -> Result<()> {
    let files = STARTERS
        .iter()
        .flat_map(|(skill, files)| {
            files.iter().map(move |(file, text)| Change::Write {
                path: format!("{skill}/{file}"),
                bytes: text.as_bytes().to_vec(),
            })
        })
        .collect::<Vec<_>>();
    git.commit(MAIN, None, &files, "The starter skills", By::Realm)
        .map(drop)
}

fn check_out(held: &Held) -> Result<()> {
    held.check_out(main(held.reads()?)?, Path::new(CHECKOUT), &|_| {
        false
    })
}

fn main(git: &Reads) -> Result<hamlet_git::CommitId> {
    git.tip(MAIN)?.context("the library has a main")
}

impl<M> Handler<M> for Library
where
    M: Message<To = address::Library>,
    M::From: Send + Sync + 'static,
    Library: Takes<M>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        message: M,
    ) -> Result<M::Output> {
        repo::admit(self, ctx, message).await
    }
}

impl Owner for Library {
    type Party = address::Library;

    fn repo(&self) -> &Repo {
        &self.repo
    }
}

impl Admits<Realm> for Library {
    type Known = ();

    fn known(_: &Store<'_>, _: &Realm) -> Result<Option<()>> {
        Ok(Some(()))
    }
}

impl Admits<Agent> for Library {
    type Known = Agent;

    fn known(_: &Store<'_>, agent: &Agent) -> Result<Option<Agent>> {
        Ok(Some(*agent))
    }
}

fn is_skill_name(skill: &str) -> bool {
    !skill.is_empty()
        && skill.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
        })
}

/// The value of `key` in the front matter that opens `text`, between
/// its `---` lines; `None` when it is not there or empty.
pub(crate) fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let head = text.strip_prefix("---")?.split("\n---").next()?;
    head.lines()
        .find_map(|line| {
            line.trim().strip_prefix(key)?.strip_prefix(':')
        })
        .map(|value| value.trim().trim_matches('"'))
        .filter(|value| !value.is_empty())
}

/// The shelves as `main` holds them. The version is the number of
/// commits on `main`, so it counts up with every skill shelved or
/// taken off.
fn shelf_list(git: &Reads) -> Result<ShelfList> {
    let main = main(git)?;
    let version =
        u32::try_from(git.history(main, usize::MAX)?.len())?;
    let mut skills = Vec::new();
    for entry in git.entries(main)? {
        let Some((name, SKILL_FILE)) = entry.path.split_once('/')
        else {
            continue;
        };
        let bytes = git.blob(entry.blob)?;
        let text = String::from_utf8_lossy(&bytes);
        let text =
            |key| field(&text, key).unwrap_or_default().to_owned();
        skills.push(Skill {
            name: name.to_owned(),
            description: text("description"),
            usage: text("usage"),
        });
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ShelfList { version, skills })
}

fn skill_paths(
    git: &Reads,
    at: hamlet_git::CommitId,
    skill: &str,
) -> Result<Vec<String>> {
    Ok(git
        .entries(at)?
        .into_iter()
        .filter(|entry| entry.path.split('/').next() == Some(skill))
        .map(|entry| entry.path)
        .collect())
}

/// Publishes a change of `main` to `library/` and to every listener.
fn published<M>(w: &mut Writer<'_, Library, M>) -> Result<()>
where
    M: Message,
    Library: Admits<M::From>,
{
    w.check_out(main(w.git()?)?, Path::new(CHECKOUT), &|_| false)?;
    let list = shelf_list(w.git()?)?;
    let listeners = w
        .table(Listeners(()))
        .rows()
        .select(
            "SELECT agent FROM listener ORDER BY agent",
            Vec::new(),
        )?
        .iter()
        .map(|row| match row.as_slice() {
            [ColumnValue::Integer(agent)] => {
                Ok(Agent(AgentNumber(u32::try_from(*agent)?)))
            }
            _ => anyhow::bail!(
                "a listener row of another shape: {row:?}"
            ),
        })
        .collect::<Result<Vec<_>>>()?;
    for agent in listeners {
        w.tell(&agent, &ShelvesChanged { list: list.clone() })?;
    }
    Ok(())
}

/// A row of `shelf_event`.
struct Event<'a> {
    kind: &'static str,
    skill: &'a str,
    by: By,
    why: &'a str,
    commit: Option<hamlet_git::CommitId>,
}

fn record(
    table: &Writes<'_, Events>,
    at: i64,
    event: Event<'_>,
) -> Result<()> {
    table.execute(
        "INSERT INTO shelf_event (kind, skill, by, why, commit_id, \
         at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        vec![
            BindParam::Text(event.kind.to_owned()),
            BindParam::Text(event.skill.to_owned()),
            BindParam::Text(event.by.name()),
            BindParam::Text(event.why.to_owned()),
            event.commit.map_or(BindParam::Null, |commit| {
                BindParam::Text(commit.to_string())
            }),
            BindParam::Integer(at),
        ],
    )?;
    Ok(())
}

impl Takes<Shelves> for Library {
    async fn take(
        call: Call<Self, Shelves>,
    ) -> Result<Reply<Shelves>> {
        call.once(shelves).await
    }
}

fn shelves(
    unit: Before<'_, Library, Shelves>,
) -> Result<Ended<Shelves>> {
    let list = shelf_list(unit.git()?)?;
    let agent = BindParam::Integer(i64::from(unit.from().0.0));
    let listening = !unit
        .rows(Listeners(()))
        .select(
            "SELECT agent FROM listener WHERE agent = ?1",
            vec![agent.clone()],
        )?
        .is_empty();
    if listening {
        return unit.kept(list);
    }
    let w = unit.write();
    w.table(Listeners(())).execute(
        "INSERT INTO listener (agent) VALUES (?1)",
        vec![agent],
    )?;
    w.kept(list)
}

impl Takes<ReadShelves> for Library {
    async fn take(
        call: Call<Self, ReadShelves>,
    ) -> Result<Reply<ReadShelves>> {
        call.read(|look| shelf_list(look.git()?)).await
    }
}

impl Takes<Unshelve> for Library {
    async fn take(
        call: Call<Self, Unshelve>,
    ) -> Result<Reply<Unshelve>> {
        call.once(unshelve).await
    }
}

/// Removes `<skill>/` from `main`. Past a kill between the commit and
/// the marker, the repeat finds no such skill and is refused.
fn unshelve(
    unit: Before<'_, Library, Unshelve>,
) -> Result<Ended<Unshelve>> {
    let Unshelve {
        skill, reason, by, ..
    } = unit.message().clone();
    let main = main(unit.git()?)?;
    let paths = if is_skill_name(&skill) {
        skill_paths(unit.git()?, main, &skill)?
    } else {
        Vec::new()
    };
    if paths.is_empty() {
        return unit.refuse(NoSkill::NoSuchSkill { skill });
    }
    let removed = paths
        .into_iter()
        .map(|path| Change::Remove { path })
        .collect::<Vec<_>>();
    let message = match &reason {
        Some(reason) => {
            format!("Took {skill} off the shelves: {reason}")
        }
        None => format!("Took {skill} off the shelves"),
    };
    let by = By::from(by);
    let mut w = unit.write();
    let commit = w.writes()?.commit(
        MAIN,
        Some(main),
        &removed,
        &message,
        by,
    )?;
    let event = Event {
        kind: "unshelved",
        skill: &skill,
        by,
        why: reason.as_deref().unwrap_or_default(),
        commit: Some(commit),
    };
    record(&w.table(Events(())), w.at(), event)?;
    published(&mut w)?;
    w.kept(Done::Done)
}

#[cfg(test)]
impl repo::testing::Hostless {
    pub(crate) fn library(home: &Path) -> Self {
        Self::new(home, home.join(REPOSITORY), SCHEMA, starters)
            .expect("a scratch folder takes a repository")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::time::Duration;

    use hamlet_actor::ActionSet;
    use hamlet_actor_error::ActorError;
    use hamlet_host::{Host, HostOptions};
    use hamlet_messages::profile::By as Who;
    use hamlet_protocol::client::RequestId;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::repo::testing::Hostless;

    fn args<M: Message>(message: &M) -> (&'static str, Vec<u8>) {
        (
            M::NAME,
            hamlet_actor::action::encode_positional(message).unwrap(),
        )
    }

    fn every_action() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            args(&Shelves { version_seen: None }),
            args(&ReadShelves),
            args(&Offer {
                call: hamlet_messages::workspace::CallId("c".into()),
                skill: "tidy".into(),
                reason: String::new(),
                part: 0,
                last: false,
                files: Vec::new(),
            }),
            args(&ReadOffer {
                offer: hamlet_messages::library::OfferId(1),
            }),
            args(&Verdict {
                id: RequestId::new(),
                offer: hamlet_messages::library::OfferId(1),
                accepted: true,
                words: String::new(),
            }),
            args(&Unshelve {
                id: RequestId::new(),
                skill: "weather".into(),
                reason: None,
                by: Who::Chief { chief: 3 },
            }),
            args(&DropOfferParts {
                agent: AgentNumber(7),
            }),
        ]
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn every_action_refuses_a_caller_it_does_not_admit() {
        let actions = every_action();
        let listed = actions
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();
        let taken = <<Library as Actor>::Actions as ActionSet<
            Library,
        >>::entries()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<BTreeSet<_>>();
        assert_eq!(listed, taken, "one call for each action");

        let dir = Scratch::new("library-every-action-caller");
        let host = Host::open_with(
            &dir,
            HashMap::from([(
                address::Library::KIND.to_owned(),
                factory(),
            )]),
            HostOptions::default(),
        )
        .unwrap();
        let library = address::Library.address().unwrap();
        host.get_or_start(&library).await.unwrap();
        for (name, args) in actions {
            let error = host
                .call(&library, name, args, Duration::from_secs(20))
                .await
                .expect_err(name);
            let error = ActorError::extract(&error);
            assert_eq!(
                format!("{}.{}", error.group(), error.code()),
                "message.wrong_caller",
                "{name}"
            );
        }
        host.shutdown().await.unwrap();
    }

    #[test]
    fn a_front_matter_field_is_read_between_its_lines() {
        let text = "---\nname: tidy\ndescription: \"Tidy \
                    up.\"\nusage:\n---\nusage: not this\n";
        assert_eq!(field(text, "name"), Some("tidy"));
        assert_eq!(field(text, "description"), Some("Tidy up."));
        assert_eq!(field(text, "usage"), None);
        assert_eq!(field("name: tidy\n", "name"), None);
    }

    #[test]
    fn an_unshelve_cut_off_before_its_marker_is_refused_by_its_repeat()
     {
        let dir = Scratch::new("library-unshelve-cut");
        let mut library = Hostless::library(&dir);
        let realm = Caller::Actor(Realm.address().unwrap());
        let asked = Unshelve {
            id: RequestId::new(),
            skill: "weather".into(),
            reason: None,
            by: Who::Chief { chief: 3 },
        };
        let cut = library.once::<Library, Unshelve, _>(
            realm.clone(),
            asked.clone(),
            |unit| {
                unshelve(unit)?;
                anyhow::bail!("killed before the marker")
            },
        );
        assert!(cut.is_err());

        let (repeat, told) = library
            .once::<Library, Unshelve, _>(realm, asked, unshelve)
            .unwrap();
        assert_eq!(
            repeat,
            Done::Refused {
                why: NoSkill::NoSuchSkill {
                    skill: "weather".into()
                }
            }
        );
        assert!(told.is_empty());
    }
}

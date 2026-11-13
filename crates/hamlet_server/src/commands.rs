//! The chief's commands: typed lines that begin with `/` (H24, H26),
//! said to an agent, whose hamlet is the one they mean. Each command
//! is a function of its own, named in its table beside how it is
//! written, and runs as a system of its own.

mod oversight;
mod shelves;
mod society;
mod versions;

use anyhow::{Result, anyhow};
use bevy_app::App;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{In, SystemId};
use bevy_ecs::world::World;
use hamlet_git::Repository;
use hamlet_protocol::client::{self as protocol, Choice};
use hamlet_store::ids::{Actor, AgentId, HamletId, ScholarId};

use crate::change::Done;
use crate::core::Core;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind};
use crate::society::hires::{self, Hiring};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, hamlets};

/// A command a chief can type: its name, how it is written, what it
/// does, and the function that does it.
pub(crate) struct Command {
    name: &'static str,
    usage: &'static str,
    does: &'static str,
    run: Run,
}

/// What runs a command, as its own system.
type Run = fn(&mut Core<'_, '_>, &Line) -> Answer;

/// The word `/create-hamlet` and `/hire` take in place of a scholar
/// for a blank agent, which is not a scholar of the city.
const BLANK: &str = "blank";

/// Whether `word` is a scholar's address, with or without a version.
fn is_scholar(word: &str) -> bool {
    word.starts_with("scholar#")
}

/// The commands about the chat itself: the same requests a client's
/// buttons make.
const SESSIONS: &[Command] = &[
    Command {
        name: "hand",
        usage: "/hand",
        does: "Talk to the Hand",
        run: choose_in_client,
    },
    Command {
        name: "hamlet",
        usage: "/hamlet <hamlet>",
        does: "Talk to a hamlet's mayor",
        run: choose_in_client,
    },
    Command {
        name: "agent",
        usage: "/agent <address>",
        does: "Talk to an agent",
        run: choose_in_client,
    },
    Command {
        name: "new",
        usage: "/new",
        does: "Start a new conversation",
        run: |core, line| Ok(Ok(core.new_session(line.agent)?)),
    },
    Command {
        name: "resume",
        usage: "/resume [n]",
        does: "Go back to an earlier conversation",
        run: resume,
    },
    Command {
        name: "retry",
        usage: "/retry",
        does: "Carry on a failed or stopped turn",
        run: |core, line| core.retry_turn(line.agent),
    },
    Command {
        name: "stop",
        usage: "/stop [address|all]",
        does: "Stop an agent's work",
        run: stop,
    },
];

/// The commands that oversee the agents' work.
const OVERSIGHT: &[Command] = &[
    Command {
        name: "to",
        usage: "/to <address> <instruction>",
        does: "Instruct another agent from here",
        run: |core, line| {
            core.instruct(line.here.as_ref(), &line.rest)
        },
    },
    Command {
        name: "reflect",
        usage: "/reflect",
        does: "Ask the agent to keep what this session taught it",
        run: |core, line| core.reflect(line.agent),
    },
    Command {
        name: "worker",
        usage: "/worker <task>",
        does: "Start a worker on a task in this hamlet",
        run: |core, line| {
            core.chiefs_worker(line.here.as_ref(), &line.rest)
        },
    },
    Command {
        name: "queue",
        usage: "/queue",
        does: "List what waits for you",
        run: |core, _| core.queue(),
    },
    Command {
        name: "allow",
        usage: "/allow [#wait] [reason]",
        does: "Allow what waits, this once",
        run: |core, line| core.answer_line(line, &Choice::Allow),
    },
    Command {
        name: "always",
        usage: "/always [#wait] [reason]",
        does: "Allow it, and the like from now on",
        run: |core, line| core.answer_line(line, &Choice::Always),
    },
    Command {
        name: "deny",
        usage: "/deny [#wait] [reason]",
        does: "Refuse what waits",
        run: |core, line| core.answer_line(line, &Choice::Deny),
    },
    Command {
        name: "go",
        usage: "/go [#wait] [reason]",
        does: "Carry out an instruction an agent objected to",
        run: |core, line| core.answer_line(line, &Choice::Go),
    },
    Command {
        name: "withdraw",
        usage: "/withdraw [#wait] [reason]",
        does: "Withdraw an instruction an agent objected to",
        run: |core, line| core.answer_line(line, &Choice::Withdraw),
    },
    Command {
        name: "reply",
        usage: "/reply [#wait] <text>",
        does: "Answer a question that waits",
        run: |core, line| core.answer_line(line, &Choice::Reply),
    },
    Command {
        name: "dismiss",
        usage: "/dismiss [#wait]",
        does: "Put away a fault or a question",
        run: |core, line| core.answer_line(line, &Choice::Dismiss),
    },
    Command {
        name: "grants",
        usage: "/grants",
        does: "List what agents may use for good",
        run: |core, _| core.grants(),
    },
    Command {
        name: "revoke",
        usage: "/revoke <address> <network|unconfined>",
        does: "Take back a lasting grant",
        run: |core, line| core.revoke(line.here.as_ref(), &line.rest),
    },
];

/// The commands about a scholar's versions.
const VERSIONS: &[Command] = &[
    Command {
        name: "tag",
        usage: "/tag <scholar#id>[@<commit>] <version> [note]",
        does: "Name a version of a scholar",
        run: |core, line| core.tag(&line.words()),
    },
    Command {
        name: "versions",
        usage: "/versions <scholar#id>",
        does: "List a scholar's versions and latest commits",
        run: |core, line| core.list_versions(&line.words()),
    },
    Command {
        name: "pin",
        usage: "/pin <address> <version|main> [reason]",
        does: "Hold a hire at a version, or let it follow main",
        run: |core, line| core.pin(line.here.as_ref(), &line.words()),
    },
    Command {
        name: "revert",
        usage: "/revert <scholar#id> <commit|version> [reason]",
        does: "Undo a commit of a scholar's main",
        run: |core, line| core.revert(&line.words()),
    },
    Command {
        name: "fork",
        usage: "/fork <scholar#id>[@<version>] <new name>",
        does: "Make a new scholar from a version of another",
        run: |core, line| core.fork(&line.words()),
    },
    Command {
        name: "purge",
        usage: "/purge <scholar#id> <path>",
        does: "Take a file out of a scholar's whole history",
        run: |core, line| core.purge(&line.words()),
    },
];

/// The commands about the library's shelves and dreams.
const SHELVES: &[Command] = &[
    Command {
        name: "unshelve",
        usage: "/unshelve <skill> [reason]",
        does: "Take a skill off the library's shelves",
        run: |core, line| core.unshelve(&line.words()),
    },
    Command {
        name: "dream",
        usage: "/dream <scholar#id|address>",
        does: "Have a hire bring its learning home, or a scholar \
               tidy",
        run: |core, line| {
            core.dream_command(line.here.as_ref(), &line.words())
        },
    },
];

/// A command line said to an agent, as its system's input: the
/// agent, its hamlet, which it means, the command's name and what
/// follows it.
pub(crate) struct Line {
    pub agent: AgentId,
    pub here: Option<HamletId>,
    pub name: String,
    pub rest: String,
}

impl Line {
    /// What follows the name, word by word.
    pub fn words(&self) -> Vec<&str> {
        self.rest.split_whitespace().collect()
    }

    /// The words from the `from`th on, as one text.
    pub fn tail(&self, from: usize) -> String {
        self.words().get(from..).unwrap_or_default().join(" ")
    }
}

/// What a command answers: words for the chief, or a refusal.
pub(crate) type Answer = Done<String>;

/// Every command, as the deeds a client lists in this order, with the
/// system that runs it. A command is added with its usage and its
/// function in one row, so the list and what runs cannot drift.
#[derive(Resource, Default)]
pub(crate) struct Deeds(
    Vec<(&'static Command, SystemId<In<Line>, Answer>)>,
);

impl Deeds {
    /// The commands a client may offer.
    pub fn listed(&self) -> Vec<protocol::Deed> {
        self.0
            .iter()
            .map(|(command, _)| protocol::Deed {
                description: command.does.into(),
                name: command.name.into(),
                // A deed's usage is what follows its name.
                usage: command
                    .usage
                    .strip_prefix('/')
                    .and_then(|rest| rest.strip_prefix(command.name))
                    .unwrap_or(command.usage)
                    .trim()
                    .into(),
            })
            .collect()
    }

    /// How command `name` is written, as a refusal.
    pub fn usage(&self, name: &str) -> Refusal {
        let usage = self
            .0
            .iter()
            .find(|(command, _)| command.name == name)
            .map_or("", |(command, _)| command.usage);
        Refusal::Usage {
            words: format!("Use {usage}"),
        }
    }

    fn system(
        &self,
        name: &str,
    ) -> Option<SystemId<In<Line>, Answer>> {
        self.0
            .iter()
            .find(|(command, _)| command.name == name)
            .map(|(_, system)| *system)
    }
}

/// Adds commands to an app.
pub(crate) trait AddCommands {
    /// Adds the commands of `table`, each run by its own system.
    fn add_commands(
        &mut self,
        table: &'static [Command],
    ) -> &mut Self;
}

impl AddCommands for App {
    fn add_commands(
        &mut self,
        table: &'static [Command],
    ) -> &mut Self {
        let world = self.world_mut();
        for command in table {
            let run = command.run;
            let system = world.register_system(
                move |In(line): In<Line>, mut core: Core| {
                    run(&mut core, &line)
                },
            );
            let mut deeds = world.get_resource_or_init::<Deeds>();
            assert!(
                deeds.system(command.name).is_none(),
                "the command /{} is added twice",
                command.name
            );
            deeds.0.push((command, system));
        }
        self
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.add_commands(SESSIONS)
        .add_commands(OVERSIGHT)
        .add_commands(VERSIONS)
        .add_commands(SHELVES)
        .add_commands(society::SOCIETY);
}

/// A line to an agent that begins with `/`: a command (H24, H26),
/// run by the system it was added with, answered once the state it
/// leaves is published.
pub(crate) fn said(
    In((client, say)): In<(u64, protocol::Say)>,
    world: &mut World,
) -> Result<()> {
    let line = world
        .run_system_cached_with(line_of, (client, say.clone()))
        .map_err(|error| anyhow!("{error}"))??;
    let answer = match line {
        None => return Ok(()),
        Some(Err(refusal)) => Err(refusal),
        Some(Ok(line)) => {
            let found = world.resource::<Deeds>().system(&line.name);
            match found {
                Some(system) => world
                    .run_system_with(system, line)
                    .map_err(|error| anyhow!("{error}"))??,
                None => Err(Refusal::Usage {
                    words: format!(
                        "There is no command /{}; the client lists \
                         the commands.",
                        line.name
                    ),
                }),
            }
        }
    };
    world
        .run_system_cached_with(answered, (client, say.id, answer))
        .map_err(|error| anyhow!("{error}"))?
}

/// The command `say` holds, said to the agent it names; none when
/// the agent is not this chief's to reach, which was answered.
fn line_of(
    In((client, say)): In<(u64, protocol::Say)>,
    mut core: Core,
) -> Result<Option<Result<Line, Refusal>>> {
    let Some(agent) = core.reached(client, &say.id, say.to.0)? else {
        return Ok(None);
    };
    if !say.images.is_empty() || !say.files.is_empty() {
        return Ok(Some(Err(Refusal::CommandAttachments)));
    }
    let text = say.text.trim().trim_start_matches('/');
    let (name, rest) = text
        .split_once(char::is_whitespace)
        .map_or((text, ""), |(name, rest)| (name, rest.trim()));
    let db = core.realm.store().connection();
    let here = hires::find(db, agent)?.map(|hire| hire.hamlet);
    Ok(Some(Ok(Line {
        agent,
        here,
        name: name.into(),
        rest: rest.into(),
    })))
}

/// Answers request `id` of `client` with what its command answered.
fn answered(
    In((client, id, answer)): In<(
        u64,
        protocol::RequestId,
        Result<String, Refusal>,
    )>,
    mut core: Core,
) -> Result<()> {
    // A command may write what no change publishes, such as a
    // revoked grant.
    core.publish();
    core.answer(client, crate::clients::check::reply(&id, answer))
}

/// `/hand`, `/hamlet` and `/agent`: who the chief talks to is the
/// client's to choose.
fn choose_in_client(_: &mut Core<'_, '_>, _: &Line) -> Answer {
    Ok(Ok("Choose who you talk to in your client.".into()))
}

/// Goes back to session `n`, or lists the sessions when none is
/// given.
fn resume(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let number =
        line.words().first().and_then(|w| w.parse::<i64>().ok());
    if let Some(number) = number {
        return core.resume_session(line.agent, number);
    }
    let chief = core.realm.chief();
    let db = core.realm.store().connection();
    let sessions =
        crate::talk::chats::sessions(db, line.agent, chief)?;
    let list = sessions
        .iter()
        .map(|(number, _, _)| format!("- session {number}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Ok(if list.is_empty() {
        "No sessions yet.".into()
    } else {
        list
    }))
}

/// Stops the work of the agent said to, the one named, or all.
fn stop(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let targets = match line.words().first() {
        None => vec![line.agent],
        Some(&"all") => core.reachable()?,
        Some(text) => {
            match core.resolve(
                text,
                line.here.as_ref(),
                Reach::Living,
            )? {
                Ok(found) => vec![found],
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
    };
    Ok(Ok(core.stop_turns(&targets)?))
}

impl Core<'_, '_> {
    /// The scholar `text` addresses: a living one of the chief's, at
    /// its `main` or pinned to `@version`.
    fn hiring(
        &mut self,
        text: &str,
        chief: hamlet_store::ids::ChiefId,
    ) -> Done<Hiring> {
        let (scholar, version) = match self.scholar_pinned(text)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        if city::get(db, scholar)?.chief != Some(chief) {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: addresses::scholar_name_of(db, scholar)?,
            }));
        }
        self.existing(scholar, version)
    }

    /// The living scholar `text` addresses, `scholar#<id>` with
    /// `@<version>` to pin one, and that version.
    pub(crate) fn scholar_pinned(
        &mut self,
        text: &str,
    ) -> Done<(ScholarId, Option<String>)> {
        let (address, version) = match text.split_once('@') {
            Some((address, version)) => {
                (address, Some(version.to_owned()))
            }
            None => (text, None),
        };
        Ok(self
            .scholar_at(address, false)?
            .map(|scholar| (scholar, version)))
    }

    /// A hire of the living `scholar`, at its `main` or pinned to
    /// `version` when that is one the scholar has.
    pub(crate) fn existing(
        &mut self,
        scholar: ScholarId,
        version: Option<String>,
    ) -> Done<Hiring> {
        if let Some(version) = &version {
            let found = Repository::open(
                &self.realm.layout().repository(scholar),
            )
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
            if found.is_none() || version == hamlet_git::MAIN {
                let db = self.realm.store().connection();
                return Ok(Err(Refusal::NoVersion {
                    name: addresses::scholar_name_of(db, scholar)?,
                    version: version.clone(),
                }));
            }
        }
        Ok(Ok(Hiring::Existing {
            scholar,
            pin: version,
        }))
    }

    /// The agent `text` addresses, said in `here`.
    fn resolve(
        &mut self,
        text: &str,
        here: Option<&HamletId>,
        reach: Reach,
    ) -> Done<AgentId> {
        let address = match addresses::parse(text) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        addresses::resolve(db, &address, here, reach)
    }

    /// The scholar `text` addresses, among the living, or the
    /// archived too when `archived`.
    pub(crate) fn scholar_at(
        &mut self,
        text: &str,
        archived: bool,
    ) -> Done<ScholarId> {
        let id = match addresses::parse_scholar(text) {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let found = city::find(db, id)?.filter(|found| {
            found.kind == city::ScholarKind::Scholar
                && (archived || found.is_living())
        });
        Ok(match found {
            Some(found) => Ok(found.id),
            None => Err(Refusal::NoScholar {
                address: text.into(),
            }),
        })
    }

    /// Refuses unless this server's chief is a chief of `hamlet`.
    fn chief_of(&mut self, hamlet: &HamletId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if hamlets::find(db, hamlet)?.is_none() {
            return Ok(Err(Refusal::NoHamlet {
                id: hamlet.to_string(),
            }));
        }
        if !authority::is_chief_of(db, chief, hamlet)? {
            return Ok(Err(Refusal::NotChief {
                hamlet: hamlet.to_string(),
            }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief leads the hire `hire`.
    pub(crate) fn leads(&mut self, hire: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if agents::get(db, hire)?.kind != AgentKind::Hire {
            let name = addresses::name_of(db, hire)?;
            return Ok(Err(Refusal::NotHire { name }));
        }
        if !authority::may_lead(db, Actor::Chief(chief), hire)? {
            let name = addresses::name_of(db, hire)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief may choose the model,
    /// effort and fast tier of `agent`.
    fn steers(&mut self, agent: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if !authority::may_steer(db, Actor::Chief(chief), agent)? {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    fn no_hamlet(&self) -> Refusal {
        Refusal::Usage {
            words: "Say this to a hamlet's mayor or one of its \
                    hires."
                .into(),
        }
    }
}

//! The chief's commands: typed lines that begin with `/` (H24, H26),
//! said to an agent, whose hamlet is the one they mean. Each command
//! is a function of its own, named in its table beside how it is
//! written, and runs as a system of its own.

mod oversight;
mod shelves;
mod society;
mod targets;
mod versions;

use anyhow::{Result, anyhow};
use bevy_app::App;
use bevy_ecs::message::Messages;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{In, SystemId};
use bevy_ecs::world::World;
use hamlet_protocol::client as protocol;
use hamlet_store::ids::{AgentId, HamletId};

use crate::app::input::AddHandler;
use crate::clients::handlers::FromClient;
use crate::core::Core;
use crate::realm::change::Done;
use crate::society::addresses::Reach;
use crate::society::hires;
use crate::society::refusal::Refusal;

type Run = fn(&mut Core<'_, '_>, &Line) -> Answer;

pub(crate) fn plugin(app: &mut App) {
    app.add_handler::<FromClient<ChiefCommand>, _>(said)
        .add_commands(SESSIONS)
        .add_commands(oversight::OVERSIGHT)
        .add_commands(versions::VERSIONS)
        .add_commands(shelves::SHELVES)
        .add_commands(society::SOCIETY);
}

/// A command a chief can type: its name, how it is written, what it
/// does, and the function that does it.
pub(crate) struct Command {
    name: &'static str,
    usage: &'static str,
    does: &'static str,
    run: Run,
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
        does: "Talk to a hamlet's lead",
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
    /// The words that follow the name.
    pub fn words(&self) -> Vec<&str> {
        self.rest.split_whitespace().collect()
    }

    /// The words from the `from`th on, as one text.
    pub fn tail(&self, from: usize) -> String {
        self.words().get(from..).unwrap_or_default().join(" ")
    }
}

/// A command's answer: words for the chief, or a refusal.
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

    /// The usage of command `name`, as a refusal.
    pub fn usage(&self, name: &str) -> Refusal {
        let usage = self
            .0
            .iter()
            .find(|(command, _)| command.name == name)
            .map_or("", |(command, _)| command.usage);
        Refusal::CommandUsage {
            usage: usage.into(),
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

/// A chief's line to an agent that begins with `/`.
pub(crate) struct ChiefCommand(pub protocol::Say);

/// The lines to agents that begin with `/`, each run as a command.
fn said(world: &mut World) -> bevy_ecs::error::Result {
    let taken = world
        .resource_mut::<Messages<FromClient<ChiefCommand>>>()
        .drain()
        .collect::<Vec<_>>();
    taken.into_iter().try_for_each(
        |FromClient {
             client,
             request: ChiefCommand(say),
         }| run(world, client, say),
    )
}

/// A line to an agent that begins with `/`: a command (H24, H26),
/// run by the system it was added with, answered once the state it
/// leaves is published.
fn run(
    world: &mut World,
    client: u64,
    say: protocol::Say,
) -> bevy_ecs::error::Result {
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
                None => Err(Refusal::NoCommand { name: line.name }),
            }
        }
    };
    world.run_system_cached_with(
        answered,
        (client, say.id, answer),
    )??;
    Ok(())
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
        crate::conversations::chats::sessions(db, line.agent, chief)?;
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

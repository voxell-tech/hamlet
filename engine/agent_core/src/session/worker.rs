//! The task that owns a session's agent and runs its jobs in order,
//! and the messages it sends back to the app's thread.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use bevy_ecs::prelude::*;
use hamlet_model::provider::Provider;
use hamlet_rules::budget;
use hamlet_rules::hamlet::Hamlet;
use hamlet_rules::library::Library;
use hamlet_rules::realm::Rules;
use hamlet_rules::vote::Motion;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use super::gate::{ToolStarting, gate};
use super::mail::Mail;
use super::{Overseen, Reply, SessionKey, place};
use crate::agent::{Agent, Event as AgentEvent, Objection};
use crate::core::{HomeDir, Model, Sandboxing, Settings, Tokio};
use crate::tools::Tools;

/// The settings a worker is handed with a job, as they stood when
/// it was queued.
pub(super) struct Brief {
    provider: Arc<dyn Provider>,
    /// The sandbox and network line of the system prompt.
    pub(super) environment: String,
    /// The model that runs where nobody chose one, by name.
    pub(super) model: String,
    /// Turns between reflections. 0 never reflects.
    reflect_every: usize,
}

impl Brief {
    pub(super) fn read(world: &World) -> Self {
        let config = &world.resource::<Settings>().config;
        let model = world.resource::<Model>();
        let net = if config.allow_net {
            "on"
        } else if world.contains_resource::<Overseen>() {
            "off unless a chief allows it; a command that needs it \
             is held until the chief answers, so set `network` to \
             true on it and carry on"
        } else {
            "off, the user can turn it on in the Agents settings"
        };
        Brief {
            provider: model.provider(),
            environment: format!(
                "Sandbox: {}. Network for tools: {net}.",
                world.resource::<Sandboxing>().0.name(),
            ),
            model: match model.replay {
                true => "replay".into(),
                false => config.model.clone(),
            },
            reflect_every: config.reflect_every,
        }
    }
}

/// A message to answer.
pub(super) struct Turn {
    pub(super) user: String,
    pub(super) text: String,
    pub(super) mail: Option<Mail>,
    pub(super) instruction: Option<Entity>,
    /// The number of the chief's task this turn answers (H7).
    pub(super) owed: Option<u64>,
}

/// One piece of work for a session's worker.
pub(super) enum Job {
    Turn(Turn),
    Reflect,
    /// `/retry`: carry on the turn that failed or was stopped.
    Retry,
    /// Take the conversation back to just before the message at
    /// line `to` of its log, which the transcript has as `entry`.
    Rewind {
        to: usize,
        entry: u64,
    },
    /// `/vote`: a chief asks the hamlet's agents whether `candidate`
    /// should be their mayor.
    Vote {
        user: String,
        candidate: String,
        reason: String,
    },
    /// `/new`: reflect if needed, archive the log, stop the worker.
    /// `/resume` then makes the archived log `resume` the session's
    /// own again.
    Archive {
        resume: Option<PathBuf>,
    },
}

/// A message from a worker to the app's thread.
pub(super) enum Msg {
    Output {
        session: Entity,
        /// The agent whose worker sent it.
        by: String,
        reply: Reply,
    },
    Gate(ToolStarting),
    Started {
        session: Entity,
        instruction: Option<Entity>,
        task: String,
    },
    Archived {
        session: Entity,
        /// True when an archived log was made the session's own.
        resumed: bool,
    },
    Finished {
        session: Entity,
    },
    /// A turn that will not run carried a chief's instruction.
    Dropped(Entity),
    /// The log has been taken back to just before the entry `entry`.
    Rewound {
        session: Entity,
        entry: u64,
    },
    /// The reply to a letter, for the session that sent it.
    Answer {
        to: SessionKey,
        from: String,
        text: String,
        task: String,
    },
}

/// The live worker of a session. Absent until the first message and
/// after `/new`.
#[derive(Component)]
pub(super) struct Jobs {
    pub(super) queue: UnboundedSender<Queued>,
    pub(super) stops: UnboundedSender<Halt>,
    /// The number of the last job queued.
    pub(super) last: u64,
}

/// A job with its number in its worker's queue.
pub(super) type Queued = (u64, Brief, Job);

/// A reason to stop, and the number of the last job it ends.
pub(super) type Halt = (String, u64);

#[derive(Resource)]
pub(super) struct Inbox {
    pub(super) tx: UnboundedSender<Msg>,
    pub(super) rx: UnboundedReceiver<Msg>,
}

#[derive(Resource, Default)]
pub(super) struct Workers(
    pub(super) Vec<tokio::task::JoinHandle<()>>,
);

pub(super) fn start_worker(
    world: &mut World,
    session: Entity,
    key: &SessionKey,
) -> Result<Jobs> {
    // Every agent works in its own house.
    let (seat, home, path) = place(world, key)?;
    let root = world.resource::<HomeDir>().0.root.clone();
    let hamlet = Hamlet::at(&root, &seat.hamlet);
    let resident = hamlet.member(&seat.name).ok_or_else(|| {
        anyhow::anyhow!("no agent named `{}`", seat.name)
    })?;
    let by = seat.name;
    let brief = Brief::read(world);
    let secrets = world.resource::<Settings>().config.keys();
    let out = world.resource::<Inbox>().tx.clone();
    let overseen = world.contains_resource::<Overseen>();
    let library = Library::of(&world.resource::<HomeDir>().0.root);
    let tools = Tools::new(home, world.resource::<Sandboxing>().0)
        .with_library(library);
    let mut agent = Agent::open(
        brief.provider,
        tools,
        brief.environment,
        &path,
        secrets,
    )?;
    agent.model = brief.model;
    agent.hamlet = Some(hamlet);
    agent.resident = Some(resident);
    agent.overseen = overseen;
    agent.gate = Some(gate(out.clone(), session));

    let (queue, jobs) = unbounded_channel();
    let (stops, stopped) = unbounded_channel();
    let worker = Worker {
        agent,
        session,
        me: key.address().to_string(),
        by,
        out,
        jobs,
        stops: stopped,
        ahead: None,
        halted: (String::new(), 0),
    };
    let handle = world.resource::<Tokio>().0.spawn(worker.run());
    world.resource_mut::<Workers>().0.push(handle);
    Ok(Jobs {
        queue,
        stops,
        last: 0,
    })
}

/// The outcome of `work` on the job `number`, or the first stop that
/// ends that job.
async fn unless_stopped<T>(
    stops: &mut UnboundedReceiver<Halt>,
    number: u64,
    work: impl Future<Output = T>,
) -> Result<T, Halt> {
    let stop = async {
        loop {
            match stops.recv().await {
                Some(halt) if halt.1 >= number => return halt,
                // It ended jobs that are over.
                Some(_) => {}
                // Nobody is left to stop it.
                None => std::future::pending::<()>().await,
            }
        }
    };
    tokio::select! {
        biased;
        halt = stop => Err(halt),
        done = work => Ok(done),
    }
}

/// The task that owns a session's agent and runs its jobs in order.
struct Worker {
    agent: Agent,
    session: Entity,
    /// The name the agent goes by between agents.
    me: String,
    /// The agent's own name.
    by: String,
    out: UnboundedSender<Msg>,
    jobs: UnboundedReceiver<Queued>,
    stops: UnboundedReceiver<Halt>,
    /// A job taken from the queue that the last stop did not end.
    ahead: Option<Queued>,
    /// The last stop heeded.
    halted: Halt,
}

impl Worker {
    fn emit(&self, reply: Reply) {
        let _ = self.out.send(Msg::Output {
            session: self.session,
            by: self.by.clone(),
            reply,
        });
    }

    fn finish(&self, result: Result<String>) {
        match result {
            Ok(text) => self.emit(Reply::Done(text)),
            Err(e) => self.emit(Reply::Error(format!("{e:#}"))),
        }
    }

    fn finished(&self) {
        let _ = self.out.send(Msg::Finished {
            session: self.session,
        });
    }

    /// The listener the agent's events go to.
    fn hear(&self) -> impl FnMut(AgentEvent) + Send + 'static {
        let (out, session, by) =
            (self.out.clone(), self.session, self.by.clone());
        move |event| {
            let _ = out.send(Msg::Output {
                session,
                by: by.clone(),
                reply: Reply::Event(event),
            });
        }
    }

    /// Notes how the turn on the chief's task `owed` ended.
    fn owe(&self, owed: Option<u64>, ok: bool) {
        if let (Some(owed), Some(hamlet)) = (owed, &self.agent.hamlet)
        {
            let _ = hamlet.answered(&self.me, owed, ok);
        }
    }

    /// Sends `text` back to the session a letter came from.
    fn answer(&self, mail: Option<Mail>, text: String) {
        let Some(Mail {
            reply_to: Some(to),
            task,
        }) = mail
        else {
            return;
        };
        let _ = self.out.send(Msg::Answer {
            to,
            from: self.me.clone(),
            text,
            task,
        });
    }

    /// Settles a job that will not run, for the reason `why`.
    /// `ended` says a chief ended it. True when it was a turn.
    fn lose(&self, job: Job, why: &str, ended: bool) -> bool {
        let turn = match job {
            Job::Turn(turn) => {
                self.owe(turn.owed, ended);
                self.answer(
                    turn.mail,
                    format!("could not finish: {why}"),
                );
                if let Some(instruction) = turn.instruction {
                    let _ = self.out.send(Msg::Dropped(instruction));
                }
                true
            }
            _ => false,
        };
        self.finished();
        turn
    }

    /// Loses the queued jobs the last stop ends and tells the
    /// session. `dropped` counts the turns already lost to it.
    fn stopped(&mut self, mut dropped: usize) {
        let (reason, last) = self.halted.clone();
        while let Ok(queued) = self.jobs.try_recv() {
            if queued.0 > last {
                self.ahead = Some(queued);
                break;
            }
            dropped +=
                usize::from(self.lose(queued.2, &reason, true));
        }
        self.emit(Reply::Notice(match dropped {
            0 => format!("Stopped: {reason}"),
            n => format!("Stopped: {reason} ({n} queued dropped)"),
        }));
        self.emit(Reply::Done(String::new()));
    }

    /// Closes the run that `halt` cut off, and ends what else it
    /// covers.
    fn cut(&mut self, halt: Halt) {
        let _ = self.agent.stopped(&halt.0);
        // The turn may have been cut off while another agent held
        // the gate.
        self.agent.gate = Some(gate(self.out.clone(), self.session));
        self.halted = halt;
        self.stopped(0);
    }

    async fn run(mut self) {
        loop {
            let next = match self.ahead.take() {
                Some(next) => Some(next),
                None => self.jobs.recv().await,
            };
            let Some((number, brief, job)) = next else {
                break;
            };
            while let Ok(halt) = self.stops.try_recv() {
                if halt.1 >= self.halted.1 {
                    self.halted = halt;
                }
            }
            // A stop asked for before the job was taken up ends it
            // all the same.
            if number <= self.halted.1 {
                let reason = self.halted.0.clone();
                let turn = self.lose(job, &reason, true);
                self.stopped(usize::from(turn));
                continue;
            }
            let Brief {
                provider,
                environment,
                model,
                reflect_every,
            } = brief;
            self.agent.provider = provider;
            self.agent.environment = environment;
            self.agent.model = model;
            // The chief may have changed the rules since the last.
            if let Some(hamlet) = &self.agent.hamlet {
                self.agent.rules = Rules::read(hamlet.home());
            }
            let ran = match job {
                Job::Turn(turn) => {
                    self.turn(number, reflect_every, turn).await
                }
                Job::Reflect => self.reflect(number).await,
                Job::Retry => self.retry(number).await,
                Job::Rewind { to, entry } => {
                    self.rewind(to, entry);
                    Ok(())
                }
                Job::Vote {
                    user,
                    candidate,
                    reason,
                } => {
                    let motion = Motion::ReplaceMayor { candidate };
                    let mut on = self.hear();
                    unless_stopped(
                        &mut self.stops,
                        number,
                        self.agent.hold_vote(
                            motion,
                            &reason,
                            Some(&user),
                            &mut on,
                        ),
                    )
                    .await
                    .map(|counted| self.finish(counted))
                }
                Job::Archive { resume } => {
                    self.archive(number, resume).await;
                    return;
                }
            };
            if let Err(halt) = ran {
                self.cut(halt);
            }
            self.finished();
        }
        // The app is shutting down.
        if self.agent.unreflected > 0 {
            let mut on = self.hear();
            let _ = self.agent.reflect(&mut on).await;
        }
    }

    /// Runs a turn, and the reflection that falls due after it.
    /// `Err` when a stop cut it off.
    async fn turn(
        &mut self,
        number: u64,
        reflect_every: usize,
        turn: Turn,
    ) -> Result<(), Halt> {
        let Turn {
            user,
            text,
            mail,
            instruction,
            owed,
        } = turn;
        // Named here, so whoever follows the turn can charge to the
        // same task.
        let task = match &mail {
            Some(mail) => mail.task.clone(),
            None => budget::new_task(&self.me),
        };
        let _ = self.out.send(Msg::Started {
            session: self.session,
            instruction,
            task: task.clone(),
        });
        self.agent.objection = match instruction {
            Some(_) => Objection::Open,
            None => Objection::Closed,
        };
        let mut on = self.hear();
        let ran = unless_stopped(
            &mut self.stops,
            number,
            self.agent.turn(Some(&task), &user, &text, &mut on),
        )
        .await;
        // A letter is answered to the session it came from,
        // whatever happened.
        let result = match ran {
            Ok(result) => result,
            Err(halt) => {
                // A chief ended it, so it is owed no more.
                self.owe(owed, true);
                self.answer(
                    mail,
                    format!("could not finish: {}", halt.0),
                );
                return Err(halt);
            }
        };
        self.owe(owed, result.is_ok());
        self.answer(
            mail,
            match &result {
                Ok(text) => text.clone(),
                Err(e) => format!("could not finish: {e:#}"),
            },
        );
        self.finish(result);
        if reflect_every > 0
            && self.agent.unreflected >= reflect_every
        {
            self.emit(Reply::Notice("reflecting".into()));
            self.reflect(number).await?;
        }
        Ok(())
    }

    /// Has the agent reflect, as part of the job `number`. `Err`
    /// when a stop cut it off.
    async fn reflect(&mut self, number: u64) -> Result<(), Halt> {
        let mut on = self.hear();
        let said = unless_stopped(
            &mut self.stops,
            number,
            self.agent.reflect(&mut on),
        )
        .await?;
        self.finish(said);
        Ok(())
    }

    /// Has the agent carry on the turn that failed, as the job
    /// `number`. `Err` when a stop cut it off.
    async fn retry(&mut self, number: u64) -> Result<(), Halt> {
        let mut on = self.hear();
        let said = unless_stopped(
            &mut self.stops,
            number,
            self.agent.again(&mut on),
        )
        .await?;
        self.finish(said);
        Ok(())
    }

    /// Takes the conversation back to just before line `to` of its
    /// log, and says the entry `entry` went with it.
    fn rewind(&mut self, to: usize, entry: u64) {
        match self.agent.rewind(to) {
            Ok(()) => {
                let _ = self.out.send(Msg::Rewound {
                    session: self.session,
                    entry,
                });
            }
            // No turn failed, so it is no error of the agent's.
            Err(e) => self.emit(Reply::Notice(format!(
                "Could not rewind the conversation: {e:#}"
            ))),
        }
    }

    /// Puts the session away as the job `number`, with a reflection
    /// first if one is due, and makes the archived log `resume` the
    /// session's own. The jobs queued behind it are lost.
    async fn archive(
        &mut self,
        number: u64,
        resume: Option<PathBuf>,
    ) {
        if self.agent.unreflected > 0 {
            // Said at once: the reflection is a call to the model
            // and takes a while.
            self.emit(Reply::Notice(
                "Putting this session away. The agent keeps what is \
                 worth keeping first."
                    .into(),
            ));
            // A stop ends the reflection and the session is put
            // away all the same.
            if let Err(halt) = self.reflect(number).await {
                self.cut(halt);
            }
        }
        // Old sessions feed usage counting and replay, so the log is
        // kept.
        let path = self.agent.log_path().to_path_buf();
        let mut stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let aside = |stamp: u64| {
            path.with_extension(format!("{stamp}.jsonl"))
        };
        // Two archived in one second must not share a name.
        while aside(stamp).exists() {
            stamp += 1;
        }
        let empty =
            std::fs::metadata(&path).is_ok_and(|log| log.len() == 0);
        // An empty log is not worth archiving.
        let archived = if empty {
            std::fs::remove_file(&path)
        } else {
            std::fs::rename(&path, aside(stamp))
        };
        match archived {
            Ok(()) => {
                let back = resume
                    .as_ref()
                    .map(|past| std::fs::rename(past, &path));
                if let Some(Err(e)) = &back {
                    self.emit(Reply::Error(format!(
                        "could not go back to that session: {e}"
                    )));
                }
                let _ = self.out.send(Msg::Archived {
                    session: self.session,
                    resumed: matches!(back, Some(Ok(()))),
                });
            }
            Err(e) => self.emit(Reply::Error(format!(
                "could not archive the session: {e}"
            ))),
        }
        self.finished();
        // Everything queued behind it is counted as pending and will
        // not run here.
        self.jobs.close();
        let mut lost = 0;
        while let Some((_, _, queued)) =
            self.ahead.take().or_else(|| self.jobs.try_recv().ok())
        {
            let why = "the session was put away";
            lost += usize::from(self.lose(queued, why, false));
        }
        if lost > 0 {
            self.emit(Reply::Error(format!(
                "{lost} message(s) sent while the session was put \
                 away were not delivered. Send them again."
            )));
        }
    }
}

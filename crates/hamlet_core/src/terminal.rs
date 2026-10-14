//! The terminal channel: one session, one line per message. For
//! debugging.

use std::io::Write;

use bevy_app::{App, AppExit, Plugin, Update};
use bevy_ecs::prelude::*;
use hamlet_rules::chief::{self, Target};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::agent::Event as AgentEvent;
use crate::oversight::{Waiting, strip};
use crate::session::{
    FinishOnExit, JobFinished, Reply, SessionKey, SessionOutput,
    UserMessage,
};

const CHANNEL: &str = "terminal";

/// Reads lines from the terminal and prints the session's replies.
pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = unbounded_channel();
        std::thread::spawn(move || {
            for line in std::io::stdin().lines() {
                let Ok(line) = line else { break };
                if tx.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = tx.send(None);
        });
        let user = std::env::var("HAMLET_USER")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "user".into());
        let key = SessionKey {
            channel: CHANNEL.into(),
            chat: user.clone(),
            thread: "main".into(),
        };
        eprintln!(
            "hamlet: /new starts a session, /reflect reflects, \
             /budget shows the spend, Ctrl-D quits\n/queue shows \
             what waits for you; /allow, /always, /deny, /go, \
             /withdraw, /reply and /dismiss answer it\n/stop \
             [agent] stops work, /undo restores the last agent to \
             leave, /to <agent> <text> instructs an agent, /grants \
             and /revoke <agent> <permission> manage \
             permissions\n/chief lists what you can do to the \
             hamlets, their mayors and their votes"
        );
        prompt();
        app.insert_resource(Lines(rx))
            .insert_resource(Me {
                key,
                user,
                streamed: false,
            })
            .add_systems(Update, read_lines)
            .add_observer(render)
            .add_observer(
                |raised: On<Add, Waiting>,
                 waiting: Query<&Waiting>| {
                    if let Ok(waiting) = waiting.get(raised.entity) {
                        eprintln!(
                            "\n  NEEDS YOU: {}",
                            strip(waiting)
                        );
                    }
                },
            )
            .add_observer(
                |done: On<JobFinished>, keys: Query<&SessionKey>| {
                    if keys
                        .get(done.event().session)
                        .is_ok_and(|k| k.channel == CHANNEL)
                    {
                        prompt();
                    }
                },
            );
    }
}

/// Lines typed so far; `None` marks end of input.
#[derive(Resource)]
struct Lines(UnboundedReceiver<Option<String>>);

#[derive(Resource)]
struct Me {
    key: SessionKey,
    user: String,
    /// True when text of the current reply already streamed to the
    /// screen.
    streamed: bool,
}

fn prompt() {
    eprint!("\n> ");
}

fn read_lines(
    mut lines: ResMut<Lines>,
    mut me: ResMut<Me>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    while let Ok(line) = lines.0.try_recv() {
        match line {
            None => {
                // The end of piped input: answer what was asked
                // before leaving.
                commands.insert_resource(FinishOnExit);
                exit.write(AppExit::Success);
            }
            Some(line) if line.trim().is_empty() => prompt(),
            Some(text) => {
                let Some(to) = chief::switch(&text) else {
                    commands.trigger(UserMessage {
                        key: me.key.clone(),
                        user: me.user.clone(),
                        text,
                    });
                    continue;
                };
                me.key = match to {
                    Target::Agent(name) => {
                        eprintln!("  (talking to the agent {name})");
                        SessionKey::with_agent(
                            CHANNEL, &me.user, &name,
                        )
                    }
                    Target::Mayor(name) => {
                        eprintln!(
                            "  (talking to the mayor of {name})"
                        );
                        SessionKey::with_mayor(
                            CHANNEL, &me.user, &name,
                        )
                    }
                };
                prompt();
                // The lines after it go to the new session, once this
                // tick has changed over.
                break;
            }
        }
    }
}

fn render(out: On<SessionOutput>, mut me: ResMut<Me>) {
    let out = out.event();
    if out.key.channel != CHANNEL {
        return;
    }
    match &out.reply {
        Reply::Event(AgentEvent::Text(t)) => {
            me.streamed = true;
            print!("{t}");
            let _ = std::io::stdout().flush();
        }
        Reply::Event(AgentEvent::Reasoning(r)) => {
            eprint!("\x1b[2m{r}\x1b[0m")
        }
        Reply::Event(AgentEvent::Tool { name, summary, .. }) => {
            eprintln!("\n  [{name}] {summary}")
        }
        Reply::Event(AgentEvent::ToolDone { exit_code, .. })
            if *exit_code != 0 =>
        {
            eprintln!("  [exit {exit_code}]")
        }
        Reply::Event(
            AgentEvent::ToolDone { .. }
            | AgentEvent::ModelCall(_)
            | AgentEvent::Consulted { .. }
            | AgentEvent::Tell { .. }
            | AgentEvent::Question { .. },
        ) => {}
        Reply::Event(AgentEvent::AgentCreated { name, purpose }) => {
            eprintln!("\n  (created agent {name}: {purpose})")
        }
        Reply::Event(AgentEvent::Agent { name, event }) => {
            match &**event {
                AgentEvent::Text(t) => eprint!("\x1b[2m{t}\x1b[0m"),
                AgentEvent::Tool {
                    name: tool,
                    summary,
                    ..
                } => {
                    eprintln!("\n  [{name}: {tool}] {summary}")
                }
                // Told however deep among the agents it happened.
                mut inner => {
                    while let AgentEvent::Agent { event, .. } = inner
                    {
                        inner = event;
                    }
                    if let AgentEvent::Civic(civic) = inner {
                        eprintln!("\n  ({})", civic.notice())
                    }
                }
            }
        }
        Reply::Event(AgentEvent::Notice(text)) => {
            eprintln!("\n  ({text})")
        }
        Reply::Event(AgentEvent::Message { from, to }) => {
            eprintln!("\n  ({from} sent a message to {to})")
        }
        Reply::Event(AgentEvent::RoadBuilt { a, b }) => {
            eprintln!("\n  (a road now joins {a} and {b})")
        }
        Reply::Event(AgentEvent::AgentRemoved { name }) => {
            eprintln!("\n  (the mayor removed {name})")
        }
        Reply::Event(AgentEvent::AgentsMerged { kept, gone }) => {
            eprintln!("\n  (the mayor merged {gone} into {kept})")
        }
        Reply::Event(AgentEvent::Civic(civic)) => {
            eprintln!("\n  ({})", civic.notice())
        }
        Reply::Notice(n) => eprintln!("\n  ({n})"),
        Reply::Done(text) => {
            if !me.streamed {
                print!("{text}");
            }
            println!();
            me.streamed = false;
        }
        Reply::Error(e) => {
            eprintln!("\nerror: {e}");
            me.streamed = false;
        }
    }
}

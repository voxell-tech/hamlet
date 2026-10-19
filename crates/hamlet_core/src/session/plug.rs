//! The tools of plugins on their way between an agent's worker and
//! the app's thread, where the plugins are.

use bevy_ecs::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use super::worker::Msg;
use crate::agent::{Ask, Given, Plug, What};
use crate::tools::plugged::Answer;
use crate::tools::{AgentTool, AgentToolCalled, Offered};

/// What a worker asks the app thread about the tools of plugins.
pub(super) struct Plugs {
    who: String,
    hamlet: String,
    task: Task,
}

enum Task {
    Offer(oneshot::Sender<Vec<Offered>>),
    Call {
        /// The name the agent called the tool by.
        tool: String,
        args: String,
        answer: oneshot::Sender<Result<String, String>>,
    },
}

/// Answers what a worker asked: the tools its agent is offered, or
/// the call of one, which an observer of the plugin takes.
pub(super) fn serve(world: &mut World, plugs: Plugs) {
    let Plugs { who, hamlet, task } = plugs;
    let mut tools = world.query::<&AgentTool>();
    let offered = tools
        .iter(world)
        .filter(|tool| tool.offered_to(&hamlet))
        .filter_map(|tool| Some((tool.clone(), tool.definition()?)))
        .collect::<Vec<_>>();
    match task {
        Task::Offer(answer) => {
            let _ = answer.send(
                offered
                    .into_iter()
                    .map(|(_, offered)| offered)
                    .collect(),
            );
        }
        Task::Call { tool, args, answer } => {
            let found =
                offered.into_iter().find(|(_, o)| o.name == tool);
            let Some((tool, _)) = found else {
                let _ = answer.send(Err(format!(
                    "no tool `{tool}` is offered"
                )));
                return;
            };
            match tool.arguments(&args) {
                Ok(args) => {
                    let mut called = AgentToolCalled {
                        agent: who,
                        hamlet,
                        plugin: tool.plugin,
                        tool: tool.name,
                        args,
                        answer: Some(Answer(answer)),
                    };
                    world.trigger_ref(&mut called);
                    world.flush();
                }
                Err(why) => {
                    let _ = answer.send(Err(why));
                }
            }
        }
    }
}

/// The way the agent of a worker reaches the plugins.
pub(super) fn plug(out: UnboundedSender<Msg>) -> Plug {
    Box::new(move |ask: Ask<'_>| {
        let (who, hamlet) =
            (ask.who.to_string(), ask.hamlet.to_string());
        match ask.what {
            What::Offer => {
                let (answer, offered) = oneshot::channel();
                let _ = out.send(Msg::Plug(Plugs {
                    who,
                    hamlet,
                    task: Task::Offer(answer),
                }));
                Box::pin(async move {
                    Given::Offered(offered.await.unwrap_or_default())
                })
            }
            What::Call { tool, args } => {
                let (answer, done) = oneshot::channel();
                let _ = out.send(Msg::Plug(Plugs {
                    who,
                    hamlet,
                    task: Task::Call {
                        tool: tool.to_string(),
                        args: args.to_string(),
                        answer,
                    },
                }));
                Box::pin(async move {
                    Given::Done(done.await.unwrap_or_else(|_| {
                        Err("the plugin is gone, or did not answer"
                            .into())
                    }))
                })
            }
        }
    })
}

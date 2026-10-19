//! The tools of plugins, and the plugins that steer a turn, on their
//! way between an agent's worker and the app's thread, where the
//! plugins are.

use bevy_ecs::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use super::worker::Msg;
use crate::agent::{Ask, Given, Plug, What};
use crate::tools::plugged::Answer;
use crate::tools::{
    AgentTool, AgentToolCalled, Offered, Steer, Steering,
};

/// The tools offered before a model call, what the plugins that steer
/// the agent made of it, and whether one may steer it.
type Offer = (Vec<Offered>, Vec<Steer>, bool);

/// What a worker asks the app thread about the tools of plugins.
pub(super) struct Plugs {
    who: String,
    hamlet: String,
    task: Task,
}

enum Task {
    Offer {
        step: u32,
        steer: bool,
        answer: oneshot::Sender<Offer>,
    },
    Call {
        /// The name the agent called the tool by.
        tool: String,
        args: String,
        answer: oneshot::Sender<Result<String, String>>,
    },
    Steer {
        step: u32,
        tool: String,
        args: String,
        answer: oneshot::Sender<Vec<Steer>>,
    },
}

/// Answers what a worker asked: the tools its agent is offered and
/// what the plugins that steer it make of its model call, the call
/// of a tool, which an observer of the plugin takes, or what the
/// plugins make of a tool call about to run.
pub(super) fn serve(world: &mut World, plugs: Plugs) {
    let Plugs { who, hamlet, task } = plugs;
    let mut tools = world.query::<&AgentTool>();
    let offered = tools
        .iter(world)
        .filter(|tool| tool.offered_to(&hamlet))
        .filter_map(|tool| Some((tool.clone(), tool.definition()?)))
        .collect::<Vec<_>>();
    match task {
        Task::Offer {
            step,
            steer,
            answer,
        } => {
            let steering = match steer {
                true => steer_at(world, &who, &hamlet, step, None),
                false => Steering::at(&who, &hamlet, step, None),
            };
            let _ = answer.send((
                offered
                    .into_iter()
                    .map(|(_, offered)| offered)
                    .collect(),
                steering.steers,
                steering.watched,
            ));
        }
        Task::Steer {
            step,
            tool,
            args,
            answer,
        } => {
            let steering = steer_at(
                world,
                &who,
                &hamlet,
                step,
                Some((tool, args)),
            );
            let _ = answer.send(steering.steers);
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

/// Puts a point of a turn to the observers of the plugins that steer
/// the agents, which answer while it is triggered.
fn steer_at(
    world: &mut World,
    who: &str,
    hamlet: &str,
    step: u32,
    tool: Option<(String, String)>,
) -> Steering {
    let mut steering = Steering::at(who, hamlet, step, tool);
    world.trigger_ref(&mut steering);
    world.flush();
    steering
}

/// The way the agent of a worker reaches the plugins.
pub(super) fn plug(out: UnboundedSender<Msg>) -> Plug {
    Box::new(move |ask: Ask<'_>| {
        let (who, hamlet) =
            (ask.who.to_string(), ask.hamlet.to_string());
        let send = |task| {
            let _ = out.send(Msg::Plug(Plugs { who, hamlet, task }));
        };
        match ask.what {
            What::Offer { step, steer } => {
                let (answer, offered) = oneshot::channel();
                send(Task::Offer {
                    step,
                    steer,
                    answer,
                });
                Box::pin(async move {
                    let (tools, steers, watched) =
                        offered.await.unwrap_or_default();
                    Given::Offered {
                        tools,
                        steers,
                        watched,
                    }
                })
            }
            What::Call { tool, args } => {
                let (answer, done) = oneshot::channel();
                send(Task::Call {
                    tool: tool.to_string(),
                    args: args.to_string(),
                    answer,
                });
                Box::pin(async move {
                    Given::Done(done.await.unwrap_or_else(|_| {
                        Err("the plugin is gone, or did not answer"
                            .into())
                    }))
                })
            }
            What::Steer { step, call } => {
                let (answer, steered) = oneshot::channel();
                send(Task::Steer {
                    step,
                    tool: call.name.clone(),
                    args: call.args.clone(),
                    answer,
                });
                Box::pin(async move {
                    Given::Steers(steered.await.unwrap_or_default())
                })
            }
        }
    })
}

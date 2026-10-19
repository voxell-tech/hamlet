//! A model call a plugin makes: checked and counted here, made on the
//! app's runtime through the core with the model the chief runs, and
//! answered in a later `step`. The plugin never sees a key or an
//! address.

use bevy::prelude::*;
use hamlet_agents_api::model::{Model, ModelCall, Role, call_model};
use hamlet_base::{HomeDir, Tokio};
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::plugin::PluginInfo;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent, ModelAnswer, ModelRequest, ModelRole,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;

/// A call that finished: the plugin, the number of its call and the
/// reply, or why there is none.
type Answered = (Entity, u32, Result<String, String>);

/// The model calls under way.
#[derive(Resource)]
pub struct Calling {
    tx: UnboundedSender<Answered>,
    rx: UnboundedReceiver<Answered>,
    rate: Rate,
    /// The plugin of each call under way.
    waiting: Vec<Entity>,
}

impl Default for Calling {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            tx,
            rx,
            rate: Rate::default(),
            waiting: Vec::new(),
        }
    }
}

impl Calling {
    pub fn any(calling: Res<Calling>) -> bool {
        !calling.waiting.is_empty()
    }
}

/// Starts the model calls the plugins that were stepped asked for.
/// One that is not made is answered at once with why. With no
/// runtime, as in the harness, a call is made on the frame.
pub fn start(
    mut done: MessageReader<Done>,
    mut plugins: Query<(&PluginInfo, &Granted, &mut Inbox)>,
    model: Option<Res<Model>>,
    home: Option<Res<HomeDir>>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    tokio: Option<Res<Tokio>>,
    mut calling: ResMut<Calling>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        let Ok((info, granted, mut inbox)) = plugins.get_mut(*plugin)
        else {
            continue;
        };
        for request in
            intents.iter().filter_map(|intent| match intent {
                Intent::ModelCall(request) => Some(request),
                _ => None,
            })
        {
            let call = asked(&info.id, request, &limits);
            let refused = match granted.verdict(Capability::ModelCall)
            {
                Some(Verdict::Allowed) => refusal(
                    &call,
                    &limits,
                    &mut calling,
                    *plugin,
                    now,
                ),
                Some(Verdict::Ask) => {
                    commands.trigger(Wanted {
                        plugin: *plugin,
                        capability: Capability::ModelCall,
                        target: None,
                        reason: String::new(),
                    });
                    Some("model.call is yet to be decided".into())
                }
                _ => Some("model.call not granted".into()),
            };
            let made =
                match (refused, model.as_deref(), home.as_deref()) {
                    (Some(why), ..) => Err(why),
                    (None, Some(model), Some(home)) => {
                        Ok((model.provider(), home.0.root.clone()))
                    }
                    _ => Err("there is no model here".into()),
                };
            let (provider, root) = match made {
                Ok(made) => made,
                Err(why) => {
                    warn!(
                        "plugin {}: model call refused: {why}",
                        info.id
                    );
                    inbox.push(Incoming::ModelAnswered(
                        ModelAnswer {
                            id: request.id,
                            outcome: Err(why),
                        },
                    ));
                    continue;
                }
            };
            info!(
                "plugin {}: calls the model, {} tokens at most, on \
                 the budget",
                info.id,
                call.tokens()
            );
            let (tx, id, entity) =
                (calling.tx.clone(), request.id, *plugin);
            calling.waiting.push(entity);
            let asking = async move {
                let said = call_model(provider, root, call).await;
                let _ = tx.send((entity, id, said));
            };
            match tokio.as_deref() {
                Some(tokio) => {
                    tokio.0.spawn(asking);
                }
                None => futures::executor::block_on(asking),
            }
        }
    }
}

/// The call a plugin's request makes, its reply held to what the
/// host allows.
fn asked(
    plugin: &str,
    request: &ModelRequest,
    limits: &Limits,
) -> ModelCall {
    let most = limits.model_reply_tokens;
    ModelCall {
        plugin: plugin.into(),
        system: request.system.clone(),
        messages: request
            .messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    ModelRole::User => Role::User,
                    ModelRole::Assistant => Role::Assistant,
                };
                (role, message.text.clone())
            })
            .collect(),
        reply_tokens: request
            .max_tokens
            .map_or(most, |asked| asked.min(most)),
    }
}

/// Why `call` is not made, if it is not: it is over the tokens of a
/// call, or the plugin has too many waiting or made too many in the
/// last minute.
fn refusal(
    call: &ModelCall,
    limits: &Limits,
    calling: &mut Calling,
    plugin: Entity,
    now: f32,
) -> Option<String> {
    let waiting = calling
        .waiting
        .iter()
        .filter(|held| **held == plugin)
        .count();
    if call.tokens() > limits.model_call_tokens {
        Some(format!(
            "the call is {} tokens, the most is {}",
            call.tokens(),
            limits.model_call_tokens
        ))
    } else if waiting >= limits.model_calls_in_flight {
        Some(format!(
            "{waiting} model calls are waiting, the most is {}",
            limits.model_calls_in_flight
        ))
    } else if !calling.rate.admit(
        plugin,
        now,
        limits.model_calls_per_minute,
    ) {
        Some(format!(
            "over {} model calls a minute",
            limits.model_calls_per_minute
        ))
    } else {
        None
    }
}

/// Gives each plugin the answers to its model calls that came in.
pub fn finish(
    mut calling: ResMut<Calling>,
    mut inboxes: Query<&mut Inbox>,
) {
    while let Ok((plugin, id, outcome)) = calling.rx.try_recv() {
        if let Some(at) =
            calling.waiting.iter().position(|held| *held == plugin)
        {
            calling.waiting.swap_remove(at);
        }
        if let Ok(mut inbox) = inboxes.get_mut(plugin) {
            inbox.push(Incoming::ModelAnswered(ModelAnswer {
                id,
                outcome,
            }));
        }
    }
}

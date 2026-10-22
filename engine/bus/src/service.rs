//! Services between plugins. A call is checked against the caller's
//! manifest and grants, its depth in a chain and the caller's calls
//! of the second, and the signature both sides name; then it is
//! handed to the plugin that serves it, and its result to the caller.

use std::collections::HashMap;
use std::time::Duration;

use bevy::app::{App, PostUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{Message, MessageReader, MessageWriter};
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::time::{Real, Time};
use hamlet_engine_api::{
    Capability, Failure, PluginId, Reply, Request, ServiceId, Ticket,
};
use hamlet_engine_core::Budgets;
use hamlet_engine_grants::{
    Decided, FirstUse, Grants, Verdict, Waiting,
};
use hamlet_engine_plugins::{
    Asked, Deliver, Enabled, PluginInfo, Stopped,
};

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Serve>()
        .add_message::<Served>()
        .init_resource::<Calls>()
        .add_systems(PostUpdate, (call, answer).chain());
}

/// A call for the plugin `callee` to serve: its service `service`,
/// called by `caller` with `argument`, `depth` plugins deep.
#[derive(Message, Clone, Debug)]
pub struct Serve {
    pub call: u64,
    pub callee: Entity,
    pub caller: PluginId,
    pub service: String,
    pub argument: Vec<u8>,
    pub depth: u32,
}

/// The call `call` served, and its result.
#[derive(Message, Clone, Debug)]
pub struct Served {
    pub call: u64,
    pub result: Result<Vec<u8>, Failure>,
}

/// The calls being served, and how many each caller made in its
/// current second.
#[derive(Resource, Default, Debug)]
pub struct Calls {
    next: u64,
    open: HashMap<u64, Open>,
    seconds: HashMap<Entity, Second>,
}

#[derive(Debug)]
struct Open {
    caller: Entity,
    ticket: Ticket,
    depth: u32,
    callee: Entity,
}

/// The calls one caller made in a second.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Second {
    /// When it began, on the real clock.
    pub since: Duration,
    pub calls: u32,
}

impl Calls {
    /// The calls `caller` made in the second that holds `now`.
    pub fn second(
        &mut self,
        caller: Entity,
        now: Duration,
    ) -> Second {
        let second = self.seconds.entry(caller).or_default();
        if now.saturating_sub(second.since) >= Duration::from_secs(1)
        {
            *second = Second {
                since: now,
                ..Second::default()
            };
        }
        *second
    }

    fn open(&mut self, now: Duration, open: Open) -> u64 {
        self.second(open.caller, now);
        if let Some(second) = self.seconds.get_mut(&open.caller) {
            second.calls = second.calls.saturating_add(1);
        }
        self.next += 1;
        self.open.insert(self.next, open);
        self.next
    }
}

/// What is known of a call before it is made.
pub struct Call<'a> {
    pub caller: &'a PluginInfo,
    pub service: &'a str,
    pub depth: u32,
    /// The calls the caller made this second.
    pub spent: Second,
    /// Whether the person allowed this call just now.
    pub allowed: bool,
}

/// Where a call that may be made goes.
#[derive(Clone, PartialEq, Debug)]
pub enum Admitted<T> {
    /// To this plugin, its service of this name.
    To(T, String),
    /// The person is to be asked about calling this service first.
    Asking(ServiceId),
}

impl Call<'_> {
    /// Where the call goes among `plugins`, each with whether it
    /// runs, or why it is not made.
    pub fn admit<'p, T: Copy>(
        &self,
        plugins: impl Iterator<Item = (T, &'p PluginInfo, bool)>,
        grants: &Grants,
        budgets: &Budgets,
    ) -> Result<Admitted<T>, Failure> {
        let caller = self.caller;
        let id = ServiceId::try_from(self.service)
            .map_err(|why| Failure::Refused(why.to_string()))?;
        let Some(import) = caller
            .services
            .imports
            .iter()
            .find(|import| import.service == id)
        else {
            return Err(Failure::Refused(format!(
                "{} does not import {id}",
                caller.id
            )));
        };
        if id.owner() == caller.id.as_str() {
            return Err(Failure::Refused(
                "a plugin does not call itself".into(),
            ));
        }
        if self.depth > budgets.chain_depth {
            return Err(Failure::OutOfBudget(format!(
                "the chain crosses more than {} plugins",
                budgets.chain_depth
            )));
        }
        if self.spent.calls >= budgets.service_calls_per_second {
            return Err(Failure::OutOfBudget(format!(
                "{} calls a second, the most",
                budgets.service_calls_per_second
            )));
        }
        if !self.allowed {
            match grants.verdict(
                caller,
                Capability::ServiceCall,
                id.as_str(),
            ) {
                Verdict::Allowed => {}
                Verdict::Asking => return Ok(Admitted::Asking(id)),
                Verdict::Refused => {
                    return Err(Failure::Refused(format!(
                        "{} {id} is not allowed",
                        Capability::ServiceCall.name()
                    )));
                }
            }
        }
        let serving = plugins
            .filter(|(_, info, running)| {
                *running && info.id.as_str() == id.owner()
            })
            .find_map(|(callee, info, _)| {
                let export = info
                    .services
                    .exports
                    .iter()
                    .find(|export| export.name == id.name())?;
                Some((callee, info, export))
            });
        let Some((callee, info, export)) = serving else {
            return Err(Failure::NoImplementer);
        };
        if (&export.argument, &export.result)
            != (&import.argument, &import.result)
        {
            return Err(Failure::Mismatch(format!(
                "{} calls {id} with {} for {}, and {} serves it \
                 with {} for {}",
                caller.id,
                import.argument,
                import.result,
                info.id,
                export.argument,
                export.result
            )));
        }
        Ok(Admitted::To(callee, export.name.clone()))
    }
}

/// Hands each call that may be made to the plugin that serves it, and
/// answers the others with why not.
fn call(
    mut asked: MessageReader<Asked>,
    mut decided: MessageReader<Decided>,
    plugins: Query<(Entity, &PluginInfo, Has<Enabled>, Has<Stopped>)>,
    grants: Res<Grants>,
    budgets: Res<Budgets>,
    time: Res<Time<Real>>,
    mut calls: ResMut<Calls>,
    mut parked: Local<Waiting<Asked>>,
    mut serve: MessageWriter<Serve>,
    mut deliver: MessageWriter<Deliver>,
    mut first_use: MessageWriter<FirstUse>,
) {
    let now = time.elapsed();
    parked.keep(|plugin| plugins.contains(plugin));
    calls.seconds.retain(|caller, _| plugins.contains(*caller));
    let mut again = Vec::new();
    for (waiting, allowed) in parked.settle(decided.read()) {
        if allowed {
            again.push((waiting, true));
        } else {
            deliver.write(waiting.reply(Err(Failure::Refused(
                format!(
                    "{} was not allowed",
                    Capability::ServiceCall.name()
                ),
            ))));
        }
    }
    let fresh = asked.read().map(|asked| (asked.clone(), false));
    for (request, allowed) in again.into_iter().chain(fresh) {
        let Request::Call { service, argument } = &request.request
        else {
            continue;
        };
        let Ok((_, caller, ..)) = plugins.get(request.plugin) else {
            continue;
        };
        let check = Call {
            caller,
            service,
            depth: request.depth,
            spent: calls.second(request.plugin, now),
            allowed,
        };
        let running =
            plugins.iter().map(|(plugin, info, on, stopped)| {
                (plugin, info, on && !stopped)
            });
        match check.admit(running, &grants, &budgets) {
            Ok(Admitted::To(callee, name)) => {
                let open = Open {
                    caller: request.plugin,
                    ticket: request.ticket,
                    depth: request.depth,
                    callee,
                };
                serve.write(Serve {
                    call: calls.open(now, open),
                    callee,
                    caller: caller.id.clone(),
                    service: name,
                    argument: argument.clone(),
                    depth: request.depth,
                });
            }
            Ok(Admitted::Asking(id)) => parked.park(
                request.plugin,
                Capability::ServiceCall,
                id.to_string(),
                request.clone(),
                &mut first_use,
            ),
            Err(why) => {
                deliver.write(request.reply(Err(why)));
            }
        }
    }
}

/// Gives each caller the result of its call. A call whose plugin
/// stopped before it was served fails.
fn answer(
    mut served: MessageReader<Served>,
    plugins: Query<(Has<Enabled>, Has<Stopped>)>,
    mut calls: ResMut<Calls>,
    mut deliver: MessageWriter<Deliver>,
) {
    let mut done = served
        .read()
        .map(|served| (served.call, served.result.clone()))
        .collect::<Vec<_>>();
    let gone = calls
        .open
        .iter()
        .filter(|(_, open)| {
            plugins.get(open.callee) != Ok((true, false))
        })
        .map(|(call, _)| *call)
        .collect::<Vec<_>>();
    done.extend(gone.into_iter().map(|call| {
        let why = "the plugin that serves it stopped".to_string();
        (call, Err(Failure::Failed(why)))
    }));
    for (call, result) in done {
        let Some(open) = calls.open.remove(&call) else {
            continue;
        };
        deliver.write(Deliver {
            plugin: open.caller,
            message: hamlet_engine_api::Message::Reply {
                ticket: open.ticket,
                reply: result.map(Reply::Answered),
            },
            depth: open.depth,
        });
    }
}

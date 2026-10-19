//! Calling a function of a typed interface another plugin exports:
//! the checks, a `step` of the plugin that implements it at once,
//! under a budget the caller pays for, and the result back, checked
//! against its type.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::command::Notice;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    Enable, Manifest, PluginInfo, Status, Type,
};

use super::calls::Calls;
use super::mail::{Who, send};
use crate::fault::Fault;
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Invocation, InvokeAnswer, InvokeError, InvokeRequest,
};
use crate::limits::Limits;
use crate::reach::{self, Sharing};
use crate::run::{self, Done};
use crate::stop::{self, Automatic};
use crate::warned::Warned;

/// The calls admitted and not yet given to their targets, and the
/// fuel each caller's calls used in the last second.
#[derive(Resource, Default)]
pub struct Invoking {
    due: Vec<Due>,
    spent: HashMap<Entity, (f32, u64)>,
}

/// A call admitted, to be given to its target.
struct Due {
    caller: (Entity, u64),
    target: Entity,
    invocation: Invocation,
}

impl Invoking {
    /// Whether a call waits to be given to its target.
    pub fn any(invoking: Res<Invoking>) -> bool {
        !invoking.due.is_empty()
    }

    /// The fuel `caller` may still have its calls use at `now`,
    /// seconds by the app's clock.
    fn left(
        &mut self,
        caller: Entity,
        now: f32,
        limits: &Limits,
    ) -> u64 {
        let (since, spent) =
            self.spent.entry(caller).or_insert((now, 0));
        if now - *since >= 1.0 || now < *since {
            (*since, *spent) = (now, 0);
        }
        limits.invoke_fuel_per_second.saturating_sub(*spent)
    }

    fn charge(&mut self, caller: Entity, now: f32, fuel: u64) {
        let (_, spent) = self.spent.entry(caller).or_insert((now, 0));
        *spent += fuel;
    }
}

/// What the host knows of a call before it is made.
struct Asking<'a> {
    caller: &'a Manifest,
    granted: &'a Granted,
    request: &'a InvokeRequest,
    /// The plugin named, if it is installed: its manifest, its
    /// status and its grants.
    target: Option<(&'a Manifest, &'a Status, &'a Granted)>,
    depth: usize,
    in_flight: usize,
    /// Whether a call of that number waits.
    waiting: bool,
    fuel_left: u64,
    /// Whether a call of the caller stopped the plugin named.
    barred: bool,
}

/// A call that may be made: the interface, the function and the type
/// of its result.
#[derive(Debug, PartialEq)]
struct Admitted<'a> {
    interface: &'a str,
    function: &'a str,
    result: Type,
}

/// Why a call is not made, and whose capability the chief is to be
/// asked about.
#[derive(Debug, PartialEq)]
struct Refused {
    error: InvokeError,
    ask: Option<Asked>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Asked {
    /// `plugin.use` of the caller, for the target.
    Use,
    /// `plugin.export` of the target.
    Export,
}

impl Refused {
    fn because(error: InvokeError) -> Self {
        Self { error, ask: None }
    }
}

/// Admits the call `request` of `caller`, `depth` plugins deep, to be
/// given to its target, or answers `caller` with why it is not made.
pub(super) fn request(
    caller: Entity,
    request: &InvokeRequest,
    depth: usize,
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &mut Calls,
    invoking: &mut Invoking,
    limits: &Limits,
    warned: &mut Warned,
    now: f32,
    commands: &mut Commands,
) {
    let Ok((_, info, manifest, _, granted)) = who.get(caller) else {
        return;
    };
    let target =
        who.iter().find(|(_, info, ..)| info.id == request.plugin);
    let asking = Asking {
        caller: manifest,
        granted,
        request,
        target: target.map(|(_, _, manifest, status, granted)| {
            (manifest, status, granted)
        }),
        depth,
        in_flight: calls.in_flight(caller),
        waiting: calls.invoking(caller, request.call_id),
        fuel_left: invoking.left(caller, now, limits),
        barred: target
            .is_some_and(|(entity, ..)| calls.barred(caller, entity)),
    };
    let to = format!("{}/{}", request.plugin, request.function);
    match (admit(&asking, limits), target) {
        (Ok(admitted), Some((entity, ..))) => {
            let id = calls.open_invoke(
                (caller, request.call_id),
                (entity, &request.plugin),
                admitted.result,
                depth,
                limits.call_frames,
            );
            info!("plugin {}: calls {to}", info.id);
            invoking.due.push(Due {
                caller: (caller, request.call_id),
                target: entity,
                invocation: Invocation {
                    call_id: id,
                    sender: info.id.clone(),
                    interface: admitted.interface.into(),
                    function: admitted.function.into(),
                    args: request.args.clone(),
                },
            });
        }
        (Ok(_), None) => {}
        (Err(Refused { error, ask }), target) => {
            warned.warn(
                &info.id,
                "invoke",
                &format!("{to} refused: {error:?}"),
            );
            let wanted = match (ask, target) {
                (Some(Asked::Use), _) => Some(Wanted {
                    plugin: caller,
                    capability: Capability::PluginUse,
                    target: Some(request.plugin.clone()),
                    reason: String::new(),
                }),
                (Some(Asked::Export), Some((entity, ..))) => {
                    Some(Wanted {
                        plugin: entity,
                        capability: Capability::PluginExport,
                        target: None,
                        reason: String::new(),
                    })
                }
                _ => None,
            };
            if let Some(wanted) = wanted {
                commands.trigger(wanted);
            }
            answer(inboxes, (caller, request.call_id), Err(error));
        }
    }
}

/// The interface, function and result type of the call `asking`
/// describes, or why it is not to be made.
fn admit<'a>(
    asking: &Asking<'a>,
    limits: &Limits,
) -> Result<Admitted<'a>, Refused> {
    let refused =
        |why: String| Refused::because(InvokeError::Refused(why));
    let request = asking.request;
    let plugin = request.plugin.as_str();
    let Some((interface, function)) =
        request.function.split_once('/')
    else {
        return Err(refused(format!(
            "`{}` is not `interface/function`",
            request.function
        )));
    };
    let (imported, wanted) = asking
        .caller
        .imports
        .interface(interface)
        .and_then(|imported| {
            Some((imported, imported.function(function)?))
        })
        .ok_or_else(|| {
            refused(format!(
                "`{interface}/{function}` is not in the manifest's \
                 imports"
            ))
        })?;
    if !imported.from(plugin) {
        return Err(refused(format!(
            "`{interface}` is not imported from `{plugin}`"
        )));
    }
    if plugin == asking.caller.id {
        return Err(refused("a plugin cannot call itself".into()));
    }
    match asking.granted.verdict_for(Capability::PluginUse, plugin) {
        Some(Verdict::Allowed) => {}
        Some(Verdict::Ask) => {
            return Err(Refused {
                error: InvokeError::Refused(
                    "plugin.use not granted".into(),
                ),
                ask: Some(Asked::Use),
            });
        }
        Some(Verdict::Denied) | None => {
            return Err(refused("plugin.use not granted".into()));
        }
    }
    if asking.barred {
        return Err(refused(format!(
            "a call of this plugin stopped `{plugin}`, which it may \
             not call until the chief restarts it"
        )));
    }
    if request.args.len() > limits.call_bytes {
        return Err(refused(format!(
            "the arguments have {} bytes, the most is {}",
            request.args.len(),
            limits.call_bytes
        )));
    }
    if asking.waiting {
        return Err(refused(format!(
            "call {} is already waiting",
            request.call_id
        )));
    }
    if asking.in_flight >= limits.calls_in_flight {
        return Err(refused(format!(
            "{} calls are waiting, the most is {}",
            asking.in_flight, limits.calls_in_flight
        )));
    }
    if asking.depth > limits.call_depth {
        return Err(refused(format!(
            "the call would cross {} plugins, the most is {}",
            asking.depth, limits.call_depth
        )));
    }
    // A call is run with all its fuel or not at all, so that no
    // caller can have its target run out and stop.
    if asking.fuel_left < limits.invoke_fuel {
        return Err(Refused::because(out_of_budget(limits)));
    }
    let mismatch =
        |why: String| Refused::because(InvokeError::Mismatch(why));
    let wanted = wanted.signature().map_err(mismatch)?;
    wanted.params.check(&request.args).map_err(|why| {
        mismatch(format!("the arguments are {why}"))
    })?;
    let none = |why: String| {
        Refused::because(InvokeError::NoImplementer(why))
    };
    let (manifest, status, granted) =
        asking.target.ok_or_else(|| {
            none(format!("no plugin `{plugin}` is installed"))
        })?;
    let exported = manifest
        .exports
        .interface(interface)
        .and_then(|exported| exported.function(function))
        .ok_or_else(|| {
            none(format!(
                "`{plugin}` does not export `{interface}/{function}`"
            ))
        })?;
    let has = exported.signature().map_err(mismatch)?;
    if has != wanted {
        return Err(mismatch(format!(
            "`{plugin}` exports `{interface}/{function}` as {has}, \
             the import says {wanted}"
        )));
    }
    if *status != Status::Running {
        return Err(none(format!(
            "`{plugin}` is {}",
            status.words()
        )));
    }
    match granted.verdict(Capability::PluginExport) {
        Some(Verdict::Allowed) => Ok(Admitted {
            interface,
            function,
            result: wanted.result,
        }),
        verdict => Err(Refused {
            error: InvokeError::NoImplementer(format!(
                "`{plugin}` does not hold plugin.export"
            )),
            ask: (verdict == Some(Verdict::Ask))
                .then_some(Asked::Export),
        }),
    }
}

/// Why a caller with less than a call's fuel left this second is
/// refused.
fn out_of_budget(limits: &Limits) -> InvokeError {
    InvokeError::OutOfBudget(format!(
        "the calls of the last second used their {} fuel, and a \
         call needs {}",
        limits.invoke_fuel_per_second, limits.invoke_fuel
    ))
}

/// Gives each call admitted to its target in a `step` of its own, at
/// once, with the time and fuel of a call of an interface, which the
/// caller must have left. The fuel the `step` uses is charged to the
/// caller. A target that fails or runs out is stopped, its caller
/// answered and barred from it, and the chief told; the other calls
/// of it in the frame are answered without running it.
pub(super) fn deliver(
    mut invoking: ResMut<Invoking>,
    mut calls: ResMut<Calls>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    survey: Res<Survey>,
    time: Option<Res<Time>>,
    mut plugins: Query<(
        &PluginInfo,
        &Manifest,
        &Granted,
        &mut Instance,
    )>,
    sharing: Sharing,
    mut inboxes: Query<(&Status, &mut Inbox)>,
    mut done: MessageWriter<Done>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    let mut stopped = HashSet::new();
    for due in std::mem::take(&mut invoking.due) {
        let id = due.invocation.call_id;
        let sender = plugins.get(due.caller.0).map_or_else(
            |_| due.invocation.sender.clone(),
            |(info, ..)| info.name.clone(),
        );
        let left = invoking.left(due.caller.0, now, &limits);
        let found = plugins
            .get_mut(due.target)
            .ok()
            .filter(|_| !stopped.contains(&due.target));
        let unmade = match &found {
            None => {
                Some(InvokeError::Failed("it is not running".into()))
            }
            Some(_) if left < limits.invoke_fuel => {
                Some(out_of_budget(&limits))
            }
            Some((info, _, _, instance))
                if instance.spent.over(&limits) =>
            {
                Some(InvokeError::OutOfBudget(format!(
                    "`{}` spent its time for this second",
                    info.id
                )))
            }
            Some(_) => None,
        };
        let (Some((info, manifest, granted, mut instance)), None) =
            (found, unmade.clone())
        else {
            if let (Some(error), Some(_)) = (unmade, calls.close(id))
            {
                answer(&mut inboxes, due.caller, Err(error));
            }
            continue;
        };
        instance.see(granted, &agents, &papers);
        instance.see_world(granted, &survey);
        instance.reach(reach::of(manifest, &sharing, &limits));
        let budget = limits.invoking(limits.invoke_fuel);
        let callers = calls.callers(due.target);
        let stepped = instance
            .step(&budget, &[Incoming::Invoked(due.invocation)]);
        invoking.charge(
            due.caller.0,
            now,
            instance.fuel_used(&budget),
        );
        match stepped {
            Ok(output) => {
                output.logs.iter().for_each(|line| {
                    run::say(&mut commands, &info.id, line)
                });
                run::want(&mut commands, due.target, &output.wants);
                done.write(Done {
                    plugin: due.target,
                    intents: output.intents,
                    changes: output.changes,
                    callers,
                });
            }
            Err(fault) => {
                stop::fail(
                    &mut commands,
                    due.target,
                    &info.id,
                    &fault,
                );
                stopped.insert(due.target);
                calls.bar(due.caller.0, due.target);
                commands.trigger(Notice(format!(
                    "A call of {sender} stopped {}: {fault}. \
                     {sender} may not call it again until you \
                     restart {}.",
                    info.name, info.name
                )));
                let error = match fault {
                    Fault::OutOfFuel => {
                        InvokeError::OutOfBudget(format!(
                            "`{}` ran out of the call's fuel",
                            info.id
                        ))
                    }
                    Fault::OutOfTime => {
                        InvokeError::OutOfBudget(format!(
                            "`{}` ran out of the call's time",
                            info.id
                        ))
                    }
                    fault => InvokeError::Failed(format!(
                        "`{}`: {fault}",
                        info.id
                    )),
                };
                if calls.close(id).is_some() {
                    answer(&mut inboxes, due.caller, Err(error));
                }
            }
        }
    }
}

/// Lets every plugin reach the plugin the chief starts again. A start
/// the host makes by itself lifts nothing.
pub(super) fn lift(
    enable: On<Enable>,
    plugins: Query<(Entity, &PluginInfo), Without<Automatic>>,
    mut calls: ResMut<Calls>,
) {
    for (target, _) in
        plugins.iter().filter(|(_, info)| info.id == enable.0)
    {
        calls.lift(target);
    }
}

/// The result `target` answered a call with, checked against the
/// type `result`.
pub(super) fn checked(
    result: &Type,
    outcome: &Result<Vec<u8>, String>,
    target: &str,
    limits: &Limits,
) -> Result<Vec<u8>, InvokeError> {
    match outcome {
        Ok(bytes) if bytes.len() > limits.call_bytes => {
            Err(InvokeError::Refused(format!(
                "the result has {} bytes, the most is {}",
                bytes.len(),
                limits.call_bytes
            )))
        }
        Ok(bytes) => match result.check(bytes) {
            Ok(()) => Ok(bytes.clone()),
            Err(why) => Err(InvokeError::Mismatch(format!(
                "`{target}` returned {why}"
            ))),
        },
        Err(why) => Err(InvokeError::Error(
            why[..why.floor_char_boundary(limits.text_bytes)].into(),
        )),
    }
}

/// Gives `caller` the answer to its call numbered as it numbered it.
pub(super) fn answer(
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    (caller, id): (Entity, u64),
    outcome: Result<Vec<u8>, InvokeError>,
) {
    send(
        inboxes,
        caller,
        Incoming::Returned(InvokeAnswer {
            call_id: id,
            outcome,
        }),
    );
}

#[cfg(test)]
mod tests {
    use hamlet_sdk::plugin::{Function, Imported, Interface};

    use super::*;

    fn function(params: &[&str], result: &str) -> Function {
        Function {
            name: "height".into(),
            params: params
                .iter()
                .map(|text| text.to_string())
                .collect(),
            result: Some(result.into()),
        }
    }

    fn caller() -> Manifest {
        let mut manifest = Manifest::built_in("harbour");
        manifest.imports.interfaces.push(Imported {
            name: "tide-table".into(),
            from: Vec::new(),
            functions: vec![function(&["string"], "option<f64>")],
        });
        manifest
    }

    fn target() -> Manifest {
        let mut manifest = Manifest::built_in("tides");
        manifest.exports.interfaces.push(Interface {
            name: "tide-table".into(),
            functions: vec![function(&["string"], "option<f64>")],
        });
        manifest
    }

    fn request(args: Vec<u8>) -> InvokeRequest {
        InvokeRequest {
            call_id: 1,
            plugin: "tides".into(),
            function: "tide-table/height".into(),
            args,
        }
    }

    fn admitted(
        granted: &Granted,
        request: &InvokeRequest,
        depth: usize,
    ) -> Result<Type, Refused> {
        let (caller, target) = (caller(), target());
        let exporting =
            Granted::allowing(&[Capability::PluginExport]);
        let asking = Asking {
            caller: &caller,
            granted,
            request,
            target: Some((&target, &Status::Running, &exporting)),
            depth,
            in_flight: 0,
            waiting: false,
            fuel_left: Limits::default().invoke_fuel,
            barred: false,
        };
        admit(&asking, &Limits::default())
            .map(|admitted| admitted.result)
    }

    #[test]
    fn a_call_is_checked_for_its_grant_its_depth_and_its_types() {
        let allowed = Granted::default()
            .allowing_targets(Capability::PluginUse, &["tides"]);
        let undecided = Granted::default()
            .allowing_targets(Capability::PluginUse, &[]);
        let port =
            request(postcard::to_allocvec(&("Kettlebay",)).unwrap());
        let deepest = Limits::default().call_depth;

        assert_eq!(
            admitted(&allowed, &port, deepest),
            Ok(Type::parse("option<f64>").unwrap())
        );
        assert_eq!(
            admitted(&undecided, &port, 1),
            Err(Refused {
                error: InvokeError::Refused(
                    "plugin.use not granted".into()
                ),
                ask: Some(Asked::Use),
            })
        );
        assert!(matches!(
            admitted(&allowed, &port, deepest + 1),
            Err(Refused { error: InvokeError::Refused(why), ask: None })
                if why.contains("would cross")
        ));
        let number =
            request(postcard::to_allocvec(&(7u32,)).unwrap());
        assert!(matches!(
            admitted(&allowed, &number, 1),
            Err(Refused {
                error: InvokeError::Mismatch(_),
                ..
            })
        ));
    }
}

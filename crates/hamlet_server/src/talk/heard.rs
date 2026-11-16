//! A runtime's messages: one system for each kind. `PreUpdate` reads
//! a runtime's line into its message, written as a [`FromRuntime`] of
//! its kind, and `Update` runs the system for that kind. A tool
//! call's request awaits the workspace, so it goes to the calls
//! instead.

use std::collections::BTreeMap;

use bevy_app::{App, PostUpdate, PreUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::error::Result;
use bevy_ecs::message::Message;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_log::warn;
use hamlet_protocol::runtime::{
    self, AdmittedStatus, ProtocolError, RuntimeMessage,
    ServerMessage, SettledStatus,
};
use hamlet_store::ids::Actor;

use super::inputs::{self, Resend, Status};
use super::leases::Authority;
use super::process::Received;
use super::runtimes::{Pending, Runtimes};
use super::turns::{self, Admission};
use super::{conversations, items, verdicts};
use crate::core::Core;
use crate::credentials::Credentials;
use crate::follow_up::{InputEffects, keep_or_roll_back_input};
use crate::providers::FAUX;
use crate::realm::Realm;
use crate::server::{AddHandler, InputMessage, InputOrigin};
use crate::{FollowUpSystems, clock};

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<RuntimeLineReceived>()
        .add_systems(
            PreUpdate,
            take_in_runtime_line
                .run_if(on_message::<RuntimeLineReceived>),
        )
        .add_systems(
            PostUpdate,
            answer_credentials
                .run_if(is_from_runtime)
                .after(keep_or_roll_back_input)
                .in_set(FollowUpSystems::KeepOrRollBackInput),
        )
        .add_handler::<FromRuntime<runtime::Allow>, _>(allow)
        .add_handler::<FromRuntime<runtime::Operation>, _>(operation)
        .add_handler::<FromRuntime<runtime::Hello>, _>(hello)
        .add_handler::<FromRuntime<runtime::Created>, _>(created)
        .add_handler::<FromRuntime<runtime::Admitted>, _>(admitted)
        .add_handler::<FromRuntime<runtime::Settled>, _>(settled)
        .add_handler::<FromRuntime<runtime::Kept>, _>(kept)
        .add_handler::<FromRuntime<runtime::Live>, _>(live)
        .add_handler::<FromRuntime<runtime::Cancelled>, _>(cancelled)
        .add_handler::<FromRuntime<runtime::Configured>, _>(
            configured,
        )
        .add_handler::<FromRuntime<runtime::Prompt>, _>(prompt)
        .add_handler::<FromRuntime<runtime::Attempt>, _>(attempt)
        .add_handler::<FromRuntime<runtime::UsageReport>, _>(usage)
        .add_handler::<FromRuntime<runtime::Yield>, _>(yielded)
        .add_handler::<FromRuntime<runtime::Scrub>, _>(scrub)
        .add_handler::<FromRuntime<ProtocolError>, _>(error)
        .add_handler::<FromRuntime<runtime::CredentialRequest>, _>(
            credential_request,
        )
        .add_handler::<FromRuntime<runtime::ProviderResult>, _>(
            provider_result,
        )
        .add_handler::<FromRuntime<runtime::ProviderEvent>, _>(
            provider_event,
        );
}

/// A line from a runtime, or the end of its output.
#[derive(Message)]
pub(crate) struct RuntimeLineReceived(pub Received);

/// A message of the kind `M` from the runtime `runtime`.
#[derive(Message)]
pub(crate) struct FromRuntime<M: Send + Sync + 'static> {
    pub runtime: Entity,
    pub message: M,
}

/// Takes in a runtime's line: the end of its output fails the
/// runtime, and a message is written as one of its kind.
fn take_in_runtime_line(
    mut lines: InputMessage<RuntimeLineReceived>,
    mut core: Core,
    mut origin: ResMut<InputOrigin>,
    mut commands: Commands,
) -> Result {
    let Some(RuntimeLineReceived(received)) = lines.take() else {
        return Ok(());
    };
    if !core.runtimes.is_current(&received) {
        return Ok(());
    }
    let from = received.runtime;
    let Some(line) = received.line else {
        core.runtimes.failed(from);
        core.leases.gone(from);
        return Ok(());
    };
    let message = match serde_json::from_str::<RuntimeMessage>(&line)
    {
        Ok(message) => message,
        Err(error) => {
            let words =
                core.credentials.scrub_text(&error.to_string());
            warn!("a runtime's line is not a message: {words}");
            return Ok(());
        }
    };
    *origin = InputOrigin::Runtime {
        runtime: from,
        awaited: awaited(&message),
    };
    write(&mut commands, from, message);
    Ok(())
}

/// Writes `message` from the runtime `from` as a [`FromRuntime`] of
/// its kind, for its system to read.
pub(crate) fn write(
    commands: &mut Commands,
    from: Entity,
    message: RuntimeMessage,
) {
    fn to<M: Send + Sync + 'static>(
        commands: &mut Commands,
        runtime: Entity,
        message: M,
    ) {
        commands.write_message(FromRuntime { runtime, message });
    }
    match message {
        RuntimeMessage::Allow(m) => to(commands, from, m),
        RuntimeMessage::Operation(m) => to(commands, from, m),
        RuntimeMessage::Hello(m) => to(commands, from, m),
        RuntimeMessage::Created(m) => to(commands, from, m),
        RuntimeMessage::Admitted(m) => to(commands, from, m),
        RuntimeMessage::Settled(m) => to(commands, from, m),
        RuntimeMessage::Kept(m) => to(commands, from, m),
        RuntimeMessage::Live(m) => to(commands, from, m),
        RuntimeMessage::Cancelled(m) => to(commands, from, m),
        RuntimeMessage::Configured(m) => to(commands, from, m),
        RuntimeMessage::Prompt(m) => to(commands, from, m),
        RuntimeMessage::Attempt(m) => to(commands, from, m),
        RuntimeMessage::Usage(m) => to(commands, from, m),
        RuntimeMessage::Yield(m) => to(commands, from, m),
        RuntimeMessage::Scrub(m) => to(commands, from, m),
        RuntimeMessage::Error(m) => to(commands, from, m),
        RuntimeMessage::CredentialRequest(m) => to(commands, from, m),
        RuntimeMessage::ProviderResult(m) => to(commands, from, m),
        RuntimeMessage::ProviderEvent(m) => to(commands, from, m),
        RuntimeMessage::Page(_) | RuntimeMessage::Statuses(_) => {}
    }
}

/// The id of `message` when the runtime waits for its answer.
fn awaited(message: &RuntimeMessage) -> Option<String> {
    let id = match message {
        RuntimeMessage::Prompt(prompt) => &prompt.id,
        RuntimeMessage::Attempt(attempt) => &attempt.id,
        RuntimeMessage::Usage(report) => &report.id,
        RuntimeMessage::Allow(allow) => &allow.id,
        RuntimeMessage::Operation(operation) => &operation.id,
        RuntimeMessage::Yield(answer) => &answer.id,
        RuntimeMessage::Scrub(scrub) => &scrub.id,
        RuntimeMessage::CredentialRequest(request) => &request.id,
        _ => return None,
    };
    Some(id.clone())
}

/// Run condition for an input from a runtime.
fn is_from_runtime(origin: Res<InputOrigin>) -> bool {
    matches!(*origin, InputOrigin::Runtime { .. })
}

/// Answers the credentials the leases now let through, after every
/// line of a runtime, failed or not.
fn answer_credentials(mut core: Core) {
    core.answer_credentials();
}

/// The permission check of a tool call, made before it is kept as
/// begun.
fn allow(
    mut heard: InputMessage<FromRuntime<runtime::Allow>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: allow,
    }) = heard.take()
    else {
        return Ok(());
    };
    core.allow_call(from, allow)?;
    Ok(())
}

/// A tool call carried out, once under its key.
fn operation(
    mut heard: InputMessage<FromRuntime<runtime::Operation>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: operation,
    }) = heard.take()
    else {
        return Ok(());
    };
    core.operation(from, operation)?;
    Ok(())
}

/// A runtime started: taken on and sent what it is owed, or refused
/// for good (S24).
fn hello(
    mut heard: InputMessage<FromRuntime<runtime::Hello>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: hello,
    }) = heard.take()
    else {
        return Ok(());
    };
    let refusal = core.runtimes.hello(
        core.realm.store().connection(),
        from,
        &hello,
    )?;
    let re = hello.id;
    match refusal {
        Some(words) => {
            warn!("{words}");
            core.runtimes.send(from, |v, id| {
                ServerMessage::Refused(runtime::Refused {
                    id,
                    re,
                    v,
                    words,
                })
            });
        }
        None => {
            let tools = core.tools.catalogue.0.clone();
            core.runtimes.send(from, |v, id| {
                ServerMessage::Welcome(runtime::Welcome {
                    id,
                    re,
                    tools,
                    v,
                    windows: BTreeMap::new(),
                })
            });
            core.flush_one(from, Resend::Unended)?;
            core.send_providers()?;
        }
    }
    Ok(())
}

/// A conversation was made or forked: its handle is kept.
fn created(
    mut heard: InputMessage<FromRuntime<runtime::Created>>,
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
    mut changed: ResMut<InputEffects>,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: created,
    }) = heard.take()
    else {
        return Ok(());
    };
    changed.runtime_answered = true;
    if let Some((_, Pending::Create(conversation))) =
        runtimes.reply(&created.re)
    {
        let db = realm.store().connection();
        let (adapter, version) = runtimes.versions(db, from)?;
        conversations::bound(
            db,
            conversation,
            &created.conversation,
            &adapter,
            &version,
        )?;
    }
    Ok(())
}

/// An input was admitted, and may have ended there.
fn admitted(
    mut heard: InputMessage<FromRuntime<runtime::Admitted>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        message: admitted, ..
    }) = heard.take()
    else {
        return Ok(());
    };
    core.changed.runtime_answered = true;
    let Some((_, Pending::Submit(conversation, request))) =
        core.runtimes.reply(&admitted.re)
    else {
        return Ok(());
    };
    let status = match admitted.status {
        AdmittedStatus::Queued => Status::Queued,
        AdmittedStatus::Placed => Status::Placed,
        AdmittedStatus::Done => Status::Done,
        AdmittedStatus::Unanswered => Status::Unanswered,
    };
    let detail = admitted.detail.map(|d| d.to_string());
    let ended = inputs::admitted(
        core.realm.store().connection(),
        conversation,
        &request,
        &admitted.submission,
        status,
        admitted.reason.as_deref(),
        detail.as_deref(),
    )?;
    if ended {
        core.input_ended(conversation, &request);
    }
    Ok(())
}

/// An input admitted before ended.
fn settled(
    mut heard: InputMessage<FromRuntime<runtime::Settled>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: settled,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    let db = core.realm.store().connection();
    let conversation =
        turns::conversation(db, &runtime, &settled.conversation)?;
    let status = match settled.status {
        SettledStatus::Done => Status::Done,
        SettledStatus::Unanswered => Status::Unanswered,
    };
    let detail = settled.detail.map(|d| d.to_string());
    if inputs::settled(
        db,
        conversation.id,
        &settled.request,
        status,
        settled.reason.as_deref(),
        detail.as_deref(),
    )? {
        core.input_ended(conversation.id, &settled.request);
    }
    Ok(())
}

/// An item of a transcript was kept, once, and its watchers see it.
fn kept(
    mut heard: InputMessage<FromRuntime<runtime::Kept>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: kept,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    let db = core.realm.store().connection();
    let conversation =
        turns::conversation(db, &runtime, &kept.conversation)?;
    if !items::keep(db, conversation.id, &kept.item)? {
        return Ok(());
    }
    let loaded = core.live.contains_key(&conversation.id);
    let live = core.live(conversation.id)?;
    // A fold read now already holds the entry.
    if loaded {
        live.fold.entry(&kept.item)?;
        live.activity.kept(&kept.item);
    }
    if !core.connections.watching(conversation.id).is_empty() {
        core.stream(conversation.id)?;
    }
    Ok(())
}

/// A turn's current doing, for its watchers and the realm's state.
fn live(
    mut heard: InputMessage<FromRuntime<runtime::Live>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: running,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    let db = core.realm.store().connection();
    let conversation =
        turns::conversation(db, &runtime, &running.conversation)?;
    let secrets = core.credentials.secrets();
    let live = core.live(conversation.id)?;
    live.fold.live(
        running.running,
        running.pending,
        running.partial.as_ref(),
        &secrets,
    );
    live.activity.live(
        running.running,
        running.pending,
        running.partial.as_ref(),
    );
    core.stream(conversation.id)?;
    core.publish();
    Ok(())
}

/// A cancel was acknowledged: later inputs may go to the runtime.
fn cancelled(
    mut heard: InputMessage<FromRuntime<runtime::Cancelled>>,
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
    mut changed: ResMut<InputEffects>,
) -> Result {
    let Some(FromRuntime {
        message: cancelled, ..
    }) = heard.take()
    else {
        return Ok(());
    };
    changed.runtime_answered = true;
    if let Some((_, Pending::Cancel(conversation))) =
        runtimes.reply(&cancelled.re)
    {
        conversations::set_cancelling(
            realm.store().connection(),
            conversation,
            false,
        )?;
    }
    Ok(())
}

/// A conversation's tools were told.
fn configured(
    mut heard: InputMessage<FromRuntime<runtime::Configured>>,
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
    mut changed: ResMut<InputEffects>,
) -> Result {
    let Some(FromRuntime {
        message: configured,
        ..
    }) = heard.take()
    else {
        return Ok(());
    };
    changed.runtime_answered = true;
    if let Some((_, Pending::Configure(conversation, told))) =
        runtimes.reply(&configured.re)
    {
        conversations::told_tools(
            realm.store().connection(),
            conversation,
            &told,
        )?;
    }
    Ok(())
}

/// The prompt of a conversation's next call, or what it waits for
/// first.
fn prompt(
    mut heard: InputMessage<FromRuntime<runtime::Prompt>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: prompt,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    let db = core.realm.store().connection();
    let conversation =
        turns::conversation(db, &runtime, &prompt.conversation)?;
    let re = prompt.id.clone();
    if let Some((key, words)) =
        core.before_prompt(&conversation, prompt.input.as_deref())?
    {
        core.runtimes.send(from, |v, id| {
            ServerMessage::Waiting(runtime::Waiting {
                id,
                key,
                re,
                v,
                words,
            })
        });
        return Ok(());
    }
    let sections = core.prompt(&conversation)?;
    core.runtimes.send(from, |v, id| {
        ServerMessage::Sections(runtime::Sections {
            id,
            re,
            sections,
            v,
        })
    });
    Ok(())
}

/// The ruling on a model call: it goes on or stops, and with which
/// model and credential.
fn attempt(
    mut heard: InputMessage<FromRuntime<runtime::Attempt>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: attempt,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    let at = clock::now();
    let admission = turns::allow(
        core.realm.store().connection(),
        &core.env,
        &core.providers,
        &runtime,
        &attempt,
        at,
    )?;
    let re = attempt.id;
    match admission {
        Admission::Allowed(selection) => {
            let provider = selection.model.provider.clone();
            core.leases.grant(from, &attempt.key, &provider);
            let db = core.realm.store().connection();
            let conversation = turns::conversation(
                db,
                &runtime,
                &attempt.conversation,
            )?;
            core.live(conversation.id)?.activity.attempted();
            core.publish();
            let serving = core
                .env
                .settings
                .provider()
                .unwrap_or_default()
                .to_owned();
            let credential = (provider != FAUX)
                .then(|| core.credentials.model(&provider, &serving))
                .flatten()
                .map(crate::credentials::pack)
                .transpose()?;
            core.runtimes.send(from, |v, id| {
                ServerMessage::Allowance(runtime::Allowance {
                    credential,
                    id,
                    re,
                    selection,
                    v,
                })
            });
        }
        Admission::Stopped(words) => {
            let notice = Some(
                super::marks::Mark::Notice {
                    text: words.clone(),
                    at,
                }
                .wire(),
            );
            core.runtimes.send(from, |v, id| {
                ServerMessage::Stopped(runtime::Stopped {
                    id,
                    notice,
                    re,
                    v,
                    words,
                })
            });
        }
    }
    Ok(())
}

/// A model call's usage, charged once.
fn usage(
    mut heard: InputMessage<FromRuntime<runtime::UsageReport>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: report,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    core.leases.finish(from, &report.key);
    let charged = turns::charge(
        core.realm.store().connection(),
        &runtime,
        &report.key,
        &report.usage,
        clock::now(),
    )?;
    if let Some(conversation) = charged {
        core.stream_usage(conversation)?;
        core.publish();
    }
    let re = report.id;
    core.runtimes.send(from, |v, id| {
        ServerMessage::Charged(runtime::Charged { id, re, v })
    });
    Ok(())
}

/// A turn's answer, ruled on.
fn yielded(
    mut heard: InputMessage<FromRuntime<runtime::Yield>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: answer,
    }) = heard.take()
    else {
        return Ok(());
    };
    let runtime = core.runtimes.id(from)?;
    let db = core.realm.store().connection();
    let conversation =
        turns::conversation(db, &runtime, &answer.conversation)?;
    let ruled = core.change(Actor::Realm, |cx, _| {
        verdicts::rule(cx, &conversation, &answer)
    })?;
    let re = answer.id;
    core.runtimes.send(from, |v, id| {
        ServerMessage::Verdict(runtime::Verdict {
            compact: ruled.compact.then_some(true),
            id,
            notice: ruled.notice,
            re,
            v,
            words: ruled.words,
        })
    });
    Ok(())
}

/// A value given back with every secret taken out.
fn scrub(
    mut heard: InputMessage<FromRuntime<runtime::Scrub>>,
    credentials: Res<Credentials>,
    mut runtimes: Runtimes,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: scrub,
    }) = heard.take()
    else {
        return Ok(());
    };
    let mut value = scrub.value;
    credentials.scrub(&mut value);
    let re = scrub.id;
    runtimes.send(from, |v, id| {
        ServerMessage::Scrubbed(runtime::Scrubbed {
            id,
            re,
            v,
            value,
        })
    });
    Ok(())
}

/// A message of the server's the runtime could not handle.
fn error(
    mut heard: InputMessage<FromRuntime<ProtocolError>>,
    credentials: Res<Credentials>,
    runtimes: Runtimes,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: error,
    }) = heard.take()
    else {
        return Ok(());
    };
    let words = credentials.scrub_text(&error.message);
    warn!("{}: {words}", runtimes.id(from)?);
    Ok(())
}

/// A credential requested, answered once the leases let it through.
fn credential_request(
    mut heard: InputMessage<FromRuntime<runtime::CredentialRequest>>,
    mut leases: ResMut<Authority>,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: request,
    }) = heard.take()
    else {
        return Ok(());
    };
    leases.request(from, request);
    Ok(())
}

fn provider_result(
    mut heard: InputMessage<FromRuntime<runtime::ProviderResult>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: result,
    }) = heard.take()
    else {
        return Ok(());
    };
    core.provider_result(Some(from), result)?;
    Ok(())
}

fn provider_event(
    mut heard: InputMessage<FromRuntime<runtime::ProviderEvent>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: event,
    }) = heard.take()
    else {
        return Ok(());
    };
    core.provider_event(from, event)?;
    Ok(())
}

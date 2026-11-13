//! A runtime's messages: one system for each kind. The
//! runner takes a runtime's line apart into its message and runs the
//! message's system once, with the runtime it came from
//! ([`hear`]). A tool call's request awaits the workspace, so it goes
//! to the calls instead.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{In, IntoSystem, Res, ResMut};
use bevy_ecs::world::World;
use hamlet_protocol::runtime::{
    self, AdmittedStatus, ProtocolError, RuntimeMessage,
    ServerMessage, SettledStatus,
};
use hamlet_store::ids::Actor;

use super::inputs::{self, Resend, Status};
use super::leases::Authority;
use super::runtimes::{Pending, Runtimes};
use super::turns::{self, Admission};
use super::{conversations, items, verdicts};
use crate::apply::Changed;
use crate::clock;
use crate::core::Core;
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::providers::FAUX;
use crate::realm::Realm;

/// A message of a runtime, with the runtime it came from, as a
/// system's input.
type Heard<M> = In<(Entity, M)>;

/// Runs the system for `message` from the runtime `from`.
pub(crate) fn hear(
    world: &mut World,
    from: Entity,
    message: RuntimeMessage,
) -> Result<()> {
    match message {
        RuntimeMessage::Allow(m) => once(world, allow, (from, m)),
        RuntimeMessage::Operation(m) => {
            once(world, operation, (from, m))
        }
        RuntimeMessage::Hello(m) => once(world, hello, (from, m)),
        RuntimeMessage::Created(m) => once(world, created, (from, m)),
        RuntimeMessage::Admitted(m) => {
            once(world, admitted, (from, m))
        }
        RuntimeMessage::Settled(m) => once(world, settled, (from, m)),
        RuntimeMessage::Kept(m) => once(world, kept, (from, m)),
        RuntimeMessage::Live(m) => once(world, live, (from, m)),
        RuntimeMessage::Cancelled(m) => {
            once(world, cancelled, (from, m))
        }
        RuntimeMessage::Configured(m) => {
            once(world, configured, (from, m))
        }
        RuntimeMessage::Prompt(m) => once(world, prompt, (from, m)),
        RuntimeMessage::Attempt(m) => once(world, attempt, (from, m)),
        RuntimeMessage::Usage(m) => once(world, usage, (from, m)),
        RuntimeMessage::Yield(m) => once(world, yielded, (from, m)),
        RuntimeMessage::Scrub(m) => once(world, scrub, (from, m)),
        RuntimeMessage::Error(m) => once(world, error, (from, m)),
        RuntimeMessage::CredentialRequest(m) => {
            once(world, credential_request, (from, m))
        }
        RuntimeMessage::ProviderResult(m) => {
            once(world, provider_result, (from, m))
        }
        RuntimeMessage::ProviderEvent(m) => {
            once(world, provider_event, (from, m))
        }
        RuntimeMessage::Page(_) | RuntimeMessage::Statuses(_) => {
            Ok(())
        }
    }
}

/// The permission check of a tool call, made before it is kept as
/// begun.
fn allow(
    In((from, asked)): Heard<runtime::Allow>,
    mut core: Core,
) -> Result<()> {
    core.allow_call(from, asked)
}

/// A tool call carried out, once under its key.
fn operation(
    In((from, asked)): Heard<runtime::Operation>,
    mut core: Core,
) -> Result<()> {
    core.operation(from, asked)
}

/// Runs `system` once with `input`, as a cached one-shot system.
fn once<I, M>(
    world: &mut World,
    system: impl IntoSystem<In<I>, Result<()>, M> + 'static,
    input: I,
) -> Result<()>
where
    I: Send + 'static,
{
    world
        .run_system_cached_with(system, input)
        .map_err(|error| anyhow!("{error}"))?
}

/// The id of `message` when the runtime waits for its answer.
pub(crate) fn awaited(message: &RuntimeMessage) -> Option<String> {
    let id = match message {
        RuntimeMessage::Prompt(asked) => &asked.id,
        RuntimeMessage::Attempt(asked) => &asked.id,
        RuntimeMessage::Usage(report) => &report.id,
        RuntimeMessage::Allow(asked) => &asked.id,
        RuntimeMessage::Operation(asked) => &asked.id,
        RuntimeMessage::Yield(answer) => &answer.id,
        RuntimeMessage::Scrub(asked) => &asked.id,
        RuntimeMessage::CredentialRequest(request) => &request.id,
        _ => return None,
    };
    Some(id.clone())
}

/// A runtime started: taken on and sent what it is owed, or refused
/// for good (S24).
fn hello(
    In((from, hello)): Heard<runtime::Hello>,
    mut core: Core,
) -> Result<()> {
    let refusal = core.runtimes.hello(
        core.realm.store().connection(),
        from,
        &hello,
    )?;
    let re = hello.id;
    match refusal {
        Some(words) => {
            tell(Secrets::Fixed, &words);
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
    In((from, created)): Heard<runtime::Created>,
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
    mut changed: ResMut<Changed>,
) -> Result<()> {
    changed.answered = true;
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
    In((_, admitted)): Heard<runtime::Admitted>,
    mut core: Core,
) -> Result<()> {
    core.changed.answered = true;
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
    In((from, settled)): Heard<runtime::Settled>,
    mut core: Core,
) -> Result<()> {
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
    In((from, kept)): Heard<runtime::Kept>,
    mut core: Core,
) -> Result<()> {
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
    In((from, running)): Heard<runtime::Live>,
    mut core: Core,
) -> Result<()> {
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
    In((_, cancelled)): Heard<runtime::Cancelled>,
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
    mut changed: ResMut<Changed>,
) -> Result<()> {
    changed.answered = true;
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
    In((_, configured)): Heard<runtime::Configured>,
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
    mut changed: ResMut<Changed>,
) -> Result<()> {
    changed.answered = true;
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
    In((from, asked)): Heard<runtime::Prompt>,
    mut core: Core,
) -> Result<()> {
    let runtime = core.runtimes.id(from)?;
    let db = core.realm.store().connection();
    let conversation =
        turns::conversation(db, &runtime, &asked.conversation)?;
    let re = asked.id.clone();
    if let Some((key, words)) =
        core.before_prompt(&conversation, asked.input.as_deref())?
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
    In((from, asked)): Heard<runtime::Attempt>,
    mut core: Core,
) -> Result<()> {
    let runtime = core.runtimes.id(from)?;
    let at = clock::now();
    let admission = turns::allow(
        core.realm.store().connection(),
        &core.env,
        &core.providers,
        &runtime,
        &asked,
        at,
    )?;
    let re = asked.id;
    match admission {
        Admission::Allowed(selection) => {
            let provider = selection.model.provider.clone();
            core.leases.grant(from, &asked.key, &provider);
            let db = core.realm.store().connection();
            let conversation = turns::conversation(
                db,
                &runtime,
                &asked.conversation,
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
    In((from, report)): Heard<runtime::UsageReport>,
    mut core: Core,
) -> Result<()> {
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
    In((from, answer)): Heard<runtime::Yield>,
    mut core: Core,
) -> Result<()> {
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
    In((from, asked)): Heard<runtime::Scrub>,
    credentials: Res<Credentials>,
    mut runtimes: Runtimes,
) -> Result<()> {
    let mut value = asked.value;
    credentials.scrub(&mut value);
    let re = asked.id;
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
    In((from, error)): Heard<ProtocolError>,
    credentials: Res<Credentials>,
    runtimes: Runtimes,
) -> Result<()> {
    tell(
        Secrets::Of(&credentials),
        format!("{}: {}", runtimes.id(from)?, error.message),
    );
    Ok(())
}

/// A credential asked for, answered once the leases let it through.
fn credential_request(
    In((from, request)): Heard<runtime::CredentialRequest>,
    mut leases: ResMut<Authority>,
) -> Result<()> {
    leases.request(from, request);
    Ok(())
}

fn provider_result(
    In((from, result)): Heard<runtime::ProviderResult>,
    mut core: Core,
) -> Result<()> {
    core.provider_result(Some(from), result)
}

fn provider_event(
    In((from, event)): Heard<runtime::ProviderEvent>,
    mut core: Core,
) -> Result<()> {
    core.provider_event(from, event)
}

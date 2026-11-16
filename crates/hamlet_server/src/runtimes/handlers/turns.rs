//! The messages of a running turn: its tool calls, its prompt, the
//! ruling on each model call and its usage, its answer, and values
//! given back with their secrets taken out. A tool call's request
//! awaits the workspace, so it goes to the calls.

use bevy_app::App;
use bevy_ecs::error::Result;
use bevy_ecs::system::Res;
use hamlet_protocol::runtime::{self, ServerMessage};
use hamlet_store::ids::Actor;

use super::FromRuntime;
use crate::app::clock;
use crate::app::input::{AddHandler, InputMessage};
use crate::conversations::turns::{self, Admission};
use crate::conversations::{self, verdicts};
use crate::core::Core;
use crate::providers::FAUX;
use crate::realm::credentials::Credentials;
use crate::runtimes::Runtimes;

pub(crate) fn plugin(app: &mut App) {
    app.add_handler::<FromRuntime<runtime::Allow>, _>(allow)
        .add_handler::<FromRuntime<runtime::Operation>, _>(operation)
        .add_handler::<FromRuntime<runtime::Prompt>, _>(prompt)
        .add_handler::<FromRuntime<runtime::Attempt>, _>(attempt)
        .add_handler::<FromRuntime<runtime::UsageReport>, _>(usage)
        .add_handler::<FromRuntime<runtime::Yield>, _>(yielded)
        .add_handler::<FromRuntime<runtime::Scrub>, _>(scrub);
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
                .map(crate::realm::credentials::pack)
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
                conversations::marks::Mark::Notice {
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

//! The messages of a runtime's and its conversations' lifecycle: a
//! runtime's hello, a conversation made, configured or cancelled, an
//! input admitted and settled, and what a turn kept and is doing.

use std::collections::BTreeMap;

use bevy_app::App;
use bevy_ecs::error::Result;
use bevy_ecs::system::ResMut;
use bevy_log::warn;
use hamlet_protocol::runtime::{
    self, AdmittedStatus, ServerMessage, SettledStatus,
};

use super::FromRuntime;
use crate::app::follow_up::InputEffects;
use crate::app::input::{AddHandler, InputMessage};
use crate::conversations::inputs::{self, Resend, Status};
use crate::conversations::{self, items, turns};
use crate::core::Core;
use crate::realm::Realm;
use crate::runtimes::{Pending, Runtimes};

pub(crate) fn plugin(app: &mut App) {
    app.add_handler::<FromRuntime<runtime::Hello>, _>(hello)
        .add_handler::<FromRuntime<runtime::Created>, _>(created)
        .add_handler::<FromRuntime<runtime::Admitted>, _>(admitted)
        .add_handler::<FromRuntime<runtime::Settled>, _>(settled)
        .add_handler::<FromRuntime<runtime::Kept>, _>(kept)
        .add_handler::<FromRuntime<runtime::Live>, _>(live)
        .add_handler::<FromRuntime<runtime::Cancelled>, _>(cancelled)
        .add_handler::<FromRuntime<runtime::Configured>, _>(
            configured,
        );
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

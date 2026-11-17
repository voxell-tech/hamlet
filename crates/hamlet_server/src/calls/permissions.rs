//! The permission check of a tool call, made before it is kept as
//! begun (C7, H11): the call admitted under its key, then allowed,
//! refused, held for a chief or looked at off the loop first.

use std::collections::BTreeMap;

use anyhow::Result;
use bevy_ecs::entity::Entity;
use hamlet_protocol::runtime::{self, RuntimeMessage};
use hamlet_store::marker::Request;
use serde_json::{Value, json};

use super::looks::Look;
use super::objections::OBJECTING;
use super::specs::is_offered;
use super::{
    Answer, Call, CallId, Decision, IDENTITY, NOBODY, Reply, held,
};
use crate::app::tasks::{BackgroundResult, RunAgainRequest};
use crate::conversations;
use crate::core::Core;
use crate::tools::catalogue::Society;

impl Core<'_, '_> {
    /// The call a runtime made, with its key reserved for its
    /// request; the reply to give when the call cannot go on.
    pub(super) fn admit<'a>(
        &mut self,
        from: Entity,
        conversation: &str,
        key: &str,
        input: Option<&'a str>,
        tool: &'a str,
        arguments: &'a BTreeMap<String, Value>,
    ) -> Result<std::result::Result<Call<'a>, Answer>> {
        let runtime = self.runtimes.id(from)?;
        let db = self.realm.store().connection();
        let Some(kept) =
            conversations::by_handle(db, &runtime, conversation)?
        else {
            return Ok(Err(Answer::Refusal(NOBODY.into())));
        };
        let request =
            Request::new("tool", &json!([input, tool, arguments]))?;
        if !held::reserve(
            db,
            &runtime,
            key,
            kept.id,
            request.digest(),
            input,
            tool,
        )? {
            return Ok(Err(Answer::Refusal(IDENTITY.into())));
        }
        Ok(Ok(Call {
            call: CallId {
                runtime,
                key: key.into(),
            },
            conversation: kept,
            input,
            tool,
            society: Society::named(tool),
            arguments,
            request,
        }))
    }

    /// Where the answer to the runtime `from`'s `request` goes.
    pub(super) fn reply_of(
        &self,
        from: Entity,
        request: &str,
    ) -> Reply {
        Reply {
            to: from,
            generation: self.runtimes.generation(from).unwrap_or(0),
            request: request.into(),
        }
    }

    /// Answers a runtime whether the call `request` may run, before
    /// it is kept as begun.
    pub(crate) fn allow_call(
        &mut self,
        from: Entity,
        request: runtime::Allow,
    ) -> Result<()> {
        let again = RuntimeMessage::Allow(request.clone());
        let reply = self.reply_of(from, &request.id);
        let call = match self.admit(
            from,
            &request.conversation,
            &request.key,
            request.input.as_deref(),
            &request.tool,
            &request.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &request.key, answer);
                return Ok(());
            }
        };
        if call.society != Some(Society::Object)
            && self.is_withdrawn(&call)?
        {
            self.tell_call(
                &reply,
                &request.key,
                Answer::Refusal(held::WITHDRAWN.into()),
            );
            return Ok(());
        }
        if self.is_objecting(&call)? {
            self.tell_call(
                &reply,
                &request.key,
                Answer::Waiting(OBJECTING.into()),
            );
            return Ok(());
        }
        let answer = if !is_offered(&call.conversation, call.tool)?
            || matches!(
                call.society,
                Some(Society::RequestChief | Society::Object)
            ) {
            Answer::Allowed
        } else {
            match self.decide(&call)? {
                Decision::Run { .. } => Answer::Allowed,
                Decision::Block(words) => Answer::Refusal(words),
                Decision::Wait(words) => {
                    Answer::Waiting(words.into())
                }
                Decision::Look(look) => {
                    self.look(&call, look, from, again);
                    return Ok(());
                }
            }
        };
        self.tell_call(&reply, &request.key, answer);
        Ok(())
    }

    /// Looks at what `call` waits on, on a task; once found, the
    /// runtime's request `again` runs again and finds it.
    pub(super) fn look(
        &mut self,
        call: &Call<'_>,
        look: Box<Look>,
        from: Entity,
        again: RuntimeMessage,
    ) {
        let secrets = self.credentials.secrets();
        let again = RunAgainRequest {
            from,
            generation: self.runtimes.generation(from).unwrap_or(0),
            message: again,
        };
        let id = call.call.clone();
        self.later.spawn(async move {
            BackgroundResult::Looked {
                call: id,
                found: look.run(secrets).await,
                again,
            }
        });
    }
}

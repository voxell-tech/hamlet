//! The calls a runtime asks of the host for its conversations, each
//! answered with [`Harnesses::answer`].

use anyhow::Result;
use hamlet_protocol::runtime;

use super::{Call, Harnesses, Notice, ReplyAddress, Wake};
use crate::Store;

impl Harnesses {
    pub(super) async fn allow(
        &mut self,
        store: &Store,
        index: usize,
        allow: runtime::Allow,
    ) -> Result<Vec<Notice>> {
        let call = Call::Allow {
            key: allow.key,
            input: allow.input,
            tool: allow.tool,
            arguments: allow.arguments,
        };
        self.call(store, index, &allow.conversation, allow.id, call)
            .await
    }

    pub(super) async fn operation(
        &mut self,
        store: &Store,
        index: usize,
        operation: runtime::Operation,
    ) -> Result<Vec<Notice>> {
        let call = Call::Operation {
            key: operation.key,
            input: operation.input,
            tool: operation.tool,
            arguments: operation.arguments,
        };
        self.call(
            store,
            index,
            &operation.conversation,
            operation.id,
            call,
        )
        .await
    }

    pub(super) async fn prompt(
        &mut self,
        store: &Store,
        index: usize,
        prompt: runtime::Prompt,
    ) -> Result<Vec<Notice>> {
        let call = Call::Prompt {
            input: prompt.input,
        };
        self.call(store, index, &prompt.conversation, prompt.id, call)
            .await
    }

    pub(super) async fn yield_answer(
        &mut self,
        store: &Store,
        index: usize,
        answer: runtime::Yield,
    ) -> Result<Vec<Notice>> {
        let call = Call::Yield {
            input: answer.input,
            answer: answer.answer,
            history: answer.history.unwrap_or(false),
            retried: answer.retried.unwrap_or(false),
            summary_failure: answer.summary_failure,
        };
        self.call(store, index, &answer.conversation, answer.id, call)
            .await
    }

    /// A call of the conversation `conversation`, for the host to
    /// answer.
    async fn call(
        &mut self,
        store: &Store,
        index: usize,
        conversation: &str,
        id: String,
        call: Call,
    ) -> Result<Vec<Notice>> {
        let Some(binding) = self
            .conversation(store, index, conversation, &id)
            .await?
        else {
            return Ok(Vec::new());
        };
        let wake = match &call {
            Call::Allow { key, .. } | Call::Operation { key, .. } => {
                Wake::Operation(key.clone())
            }
            Call::Prompt { .. } | Call::Yield { .. } => {
                Wake::Turn(binding.id)
            }
        };
        let address = ReplyAddress {
            index,
            generation: self.runtimes[index].generation,
            id,
            wake,
        };
        Ok(vec![Notice::Call {
            binding: binding.id,
            address,
            call,
        }])
    }
}

//! The chats' gate and deeds as the harness tests' driver reaches
//! them, without the client protocol.

use anyhow::{Context, Result};

use super::records::{self, Place};
use super::{calls, deeds};
use crate::RuntimeInstance;
use crate::harness::{
    BindingId, Call, Harnesses, Notice, ReplyAddress,
};
use crate::protocol::Refusable;
use crate::record::AgentRef;
use crate::service::Service;

impl Service {
    /// Binds a conversation of the chief with `agent`, whose calls
    /// the gate rules on as a chat's.
    pub async fn bind_chat(
        &mut self,
        harnesses: &mut Harnesses,
        agent: &AgentRef,
    ) -> Result<(BindingId, Vec<Notice>)> {
        let (binding, notices) =
            harnesses.bind(self.home.store(), agent, false).await?;
        let place = Place {
            agent: agent.clone(),
            chief: Some(self.chief.clone()),
            session: 1,
        };
        records::keep(self.db(), binding, &place, None, 0)?;
        Ok((binding, notices))
    }

    /// Answers the tool call `call` of `binding` through the gate.
    pub async fn answer_call(
        &mut self,
        harnesses: &mut Harnesses,
        binding: BindingId,
        address: ReplyAddress,
        call: Call,
    ) -> Result<()> {
        calls::call(self, harnesses, binding, address, call).await
    }

    /// The open wait that holds the call `key` of `runtime`.
    pub fn wait_of(
        &mut self,
        runtime: &RuntimeInstance,
        key: &str,
    ) -> Result<Option<String>> {
        crate::oversight::held::open_wait_of(
            self.db(),
            &crate::OperationId {
                runtime: runtime.clone(),
                key: key.into(),
            },
        )
    }

    /// The chief's `choice` on the wait `wait`, in the words a reply
    /// gives.
    pub async fn answer_wait(
        &mut self,
        harnesses: &mut Harnesses,
        wait: &str,
        choice: &hamlet_protocol::client::Choice,
    ) -> Result<Refusable<String>> {
        calls::answer_words(self, harnesses, wait, choice, "", None)
            .await
    }

    /// Stops the turn of the conversation `binding`, as a chief's
    /// stop does.
    pub async fn stop_chat(
        &mut self,
        harnesses: &mut Harnesses,
        binding: BindingId,
    ) -> Result<()> {
        let kept = records::kept(self.db(), binding)?
            .context("a chat's conversation")?;
        deeds::stop_conversation(
            self,
            harnesses,
            &kept.place.agent,
            binding,
        )
        .await
    }
}

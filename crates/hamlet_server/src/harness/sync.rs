//! Conversation sync: what each runtime is owed of its conversations,
//! and what it reports of them in turn.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use super::link::{self, Mark, ServerMessage};
use super::session::Pending;
use super::{
    Binding, BindingId, Harnesses, NOTICE, Notice, Phase, SYNC_PAGE,
};
use crate::Store;
use crate::models::Choices;

impl Harnesses {
    /// Sends what `index` is owed: conversations to make, then each
    /// conversation's kept inputs in order, and when `resync` also
    /// those admitted whose end was not heard, which answer again.
    pub(super) async fn flush(
        &mut self,
        store: &mut Store,
        index: usize,
        resync: bool,
    ) -> Result<()> {
        if self.runtimes[index].phase != Phase::Ready {
            return Ok(());
        }
        for request in
            self.provider_requests.remove(&index).unwrap_or_default()
        {
            self.send(index, |_, _| {
                ServerMessage::ProviderRequest(request)
            })
            .await;
        }
        let mut choices = None;
        let runtime_id = self.runtimes[index].id.clone();
        for binding in store.bindings_of(&runtime_id)? {
            let Some(conversation) = binding.conversation.clone()
            else {
                if !self.session.in_flight(index, |p| {
                    matches!(p, Pending::Create(b) if *b == binding.id)
                }) {
                    self.make(store, index, &binding).await?;
                }
                continue;
            };
            // No later input reaches pi until the cancel is durably
            // acknowledged.
            if store.cancelling(binding.id)? {
                if !self.session.in_flight(index, |p| {
                    matches!(p, Pending::Cancel(b) if *b == binding.id)
                }) {
                    self.send_cancel(index, binding.id, conversation)
                        .await;
                }
                continue;
            }
            // The tools a conversation offers reach pi before the
            // inputs kept after they changed.
            if let Some(tools) = store.tools_owed(binding.id)? {
                if !self.session.in_flight(index, |p| {
                    matches!(p, Pending::Configure(b, _) if *b == binding.id)
                }) {
                    let told = serde_json::to_string(&tools)?;
                    self.request(
                        index,
                        Pending::Configure(binding.id, told),
                        |v, id| {
                            ServerMessage::Configure(link::Configure {
                                conversation,
                                id,
                                tools,
                                v,
                            })
                        },
                    )
                    .await;
                }
                continue;
            }
            let owed = store.owed(binding.id, resync)?;
            if owed.is_empty() {
                continue;
            }
            let choices = match &mut choices {
                Some(choices) => choices,
                None => choices.insert(self.choices(store)?),
            };
            let window =
                choices.select(store, &binding)?.context_window;
            for owed in owed {
                let request = owed.request.clone();
                if self.session.in_flight(index, |p| {
                    matches!(p, Pending::Submit(b, r) if *b == binding.id && *r == request)
                }) {
                    continue;
                }
                let conversation = conversation.clone();
                self.request(
                    index,
                    Pending::Submit(binding.id, request),
                    |v, id| {
                        if owed.write {
                            ServerMessage::Write(link::Write {
                                boundary: owed
                                    .boundary
                                    .then_some(true),
                                conversation,
                                id,
                                mark: owed.mark.unwrap_or(Mark {
                                    data: BTreeMap::new(),
                                    kind: NOTICE.into(),
                                }),
                                request: owed.request,
                                v,
                            })
                        } else {
                            ServerMessage::Submit(link::Submit {
                                content: owed.content,
                                conversation,
                                window,
                                id,
                                mark: owed.mark,
                                request: owed.request,
                                steer: owed.steer,
                                v,
                            })
                        }
                    },
                )
                .await;
            }
        }
        Ok(())
    }

    /// What model calls are chosen from now.
    pub(super) fn choices(&self, store: &Store) -> Result<Choices> {
        Choices::read(
            store,
            self.layout.root(),
            self.config.pi.faux.is_some(),
        )
    }

    pub(super) async fn send_cancel(
        &mut self,
        index: usize,
        binding: BindingId,
        conversation: String,
    ) {
        self.request(index, Pending::Cancel(binding), |v, id| {
            ServerMessage::Cancel(link::Cancel {
                conversation,
                id,
                v,
            })
        })
        .await;
    }

    /// Asks the runtime to make the conversation of `binding`: a new
    /// one, or a copy of the one it forks.
    async fn make(
        &mut self,
        store: &Store,
        index: usize,
        binding: &Binding,
    ) -> Result<()> {
        let key = binding.id.0.to_string();
        let fork = match (binding.fork_of, &binding.fork_at) {
            (Some(parent), Some(at)) => Some((
                self.bound(store, parent)?
                    .conversation
                    .context("a fork of a conversation not made")?,
                at.clone(),
            )),
            _ => None,
        };
        let agent = format!(
            "{}:{}",
            binding.agent.hamlet, binding.agent.number
        );
        let errand = binding.errand;
        let model = binding.model.clone();
        self.request(
            index,
            Pending::Create(binding.id),
            move |v, id| match fork {
                Some((conversation, at)) => {
                    ServerMessage::Fork(link::Fork {
                        at,
                        conversation,
                        id,
                        key,
                        v,
                    })
                }
                None => ServerMessage::Create(link::Create {
                    agent,
                    errand,
                    id,
                    key,
                    model,
                    v,
                }),
            },
        )
        .await;
        Ok(())
    }

    /// Asks for the entries of `conversation` before `before`, to
    /// keep those the server lacks.
    pub(super) async fn sync(
        &mut self,
        index: usize,
        binding: BindingId,
        conversation: String,
        before: Option<String>,
    ) {
        self.request(index, Pending::Sync(binding), |v, id| {
            ServerMessage::History(link::History {
                before,
                conversation,
                id,
                limit: SYNC_PAGE,
                v,
            })
        })
        .await;
    }

    /// Keeps the entries of a page read to catch up, asking for the
    /// page before it until one holds an entry already kept.
    async fn synced(
        &mut self,
        store: &mut Store,
        index: usize,
        binding: BindingId,
        page: link::Page,
    ) -> Result<Vec<Notice>> {
        let mut known = false;
        let mut notices = Vec::new();
        for item in page.items.iter() {
            if store.has_item(binding, &item.entry)? {
                known = true;
            } else if store.keep_item(binding, item)? {
                notices.push(Notice::Kept {
                    binding,
                    item: item.clone(),
                });
            }
        }
        if page.earlier
            && !known
            && let (Some(first), Some(conversation)) = (
                page.items.first(),
                self.bound(store, binding)?.conversation,
            )
        {
            let before = first.entry.clone();
            self.sync(index, binding, conversation, Some(before))
                .await;
        }
        Ok(notices)
    }

    pub(super) async fn created(
        &mut self,
        store: &mut Store,
        index: usize,
        created: link::Created,
    ) -> Result<Vec<Notice>> {
        let Some(Pending::Create(binding)) =
            self.session.reply(&created.re)
        else {
            return Ok(Vec::new());
        };
        let row = store.runtime(&self.runtimes[index].id)?;
        store.conversation_made(
            binding,
            &created.conversation,
            &row.adapter.unwrap_or_default(),
            &row.version.unwrap_or_default(),
        )?;
        self.flush(store, index, false).await?;
        Ok(vec![Notice::Created { binding }])
    }

    pub(super) fn admitted(
        &mut self,
        store: &mut Store,
        admitted: link::Admitted,
    ) -> Result<Vec<Notice>> {
        let Some(Pending::Submit(binding, request)) =
            self.session.reply(&admitted.re)
        else {
            return Ok(Vec::new());
        };
        store.input_stands(
            binding,
            &request,
            Some(&admitted.submission),
            match admitted.status {
                link::AdmittedStatus::Queued => "queued",
                link::AdmittedStatus::Placed => "placed",
                link::AdmittedStatus::Done => "done",
                link::AdmittedStatus::Unanswered => "unanswered",
            },
            admitted.reason.as_deref(),
            admitted.detail.as_ref(),
        )?;
        let standing = store
            .standing(binding, &request)?
            .context("an admitted input")?;
        Ok(vec![Notice::Admitted {
            binding,
            request,
            standing,
        }])
    }

    pub(super) fn settled(
        &self,
        store: &mut Store,
        index: usize,
        settled: link::Settled,
    ) -> Result<Vec<Notice>> {
        let runtime = &self.runtimes[index].id;
        let Some(binding) =
            store.bound(runtime, &settled.conversation)?
        else {
            return Ok(Vec::new());
        };
        store.input_stands(
            binding.id,
            &settled.request,
            None,
            match settled.status {
                link::SettledStatus::Done => "done",
                link::SettledStatus::Unanswered => "unanswered",
            },
            settled.reason.as_deref(),
            settled.detail.as_ref(),
        )?;
        // An input this server never kept, such as one the runtime
        // made itself, ends unheard of.
        let Some(standing) =
            store.standing(binding.id, &settled.request)?
        else {
            return Ok(Vec::new());
        };
        Ok(vec![Notice::Settled {
            binding: binding.id,
            request: settled.request,
            standing,
        }])
    }

    pub(super) async fn cancelled(
        &mut self,
        store: &mut Store,
        index: usize,
        cancelled: link::Cancelled,
    ) -> Result<Vec<Notice>> {
        let Some(Pending::Cancel(binding)) =
            self.session.reply(&cancelled.re)
        else {
            return Ok(Vec::new());
        };
        store.cancelled(binding)?;
        self.flush(store, index, true).await?;
        Ok(vec![Notice::Cancelled { binding }])
    }

    pub(super) async fn configured(
        &mut self,
        store: &mut Store,
        index: usize,
        configured: link::Configured,
    ) -> Result<Vec<Notice>> {
        let Some(Pending::Configure(binding, tools)) =
            self.session.reply(&configured.re)
        else {
            return Ok(Vec::new());
        };
        store.tools_told(binding, &tools)?;
        self.flush(store, index, false).await?;
        Ok(Vec::new())
    }

    pub(super) async fn page(
        &mut self,
        store: &mut Store,
        index: usize,
        page: link::Page,
    ) -> Result<Vec<Notice>> {
        match self.session.reply(&page.re) {
            Some(Pending::History(binding)) => {
                Ok(vec![Notice::Page {
                    binding,
                    items: page.items,
                    earlier: page.earlier,
                }])
            }
            Some(Pending::Sync(binding)) => {
                self.synced(store, index, binding, page).await
            }
            _ => Ok(Vec::new()),
        }
    }

    pub(super) fn kept(
        &self,
        store: &mut Store,
        index: usize,
        kept: link::Kept,
    ) -> Result<Vec<Notice>> {
        let runtime = &self.runtimes[index].id;
        let Some(binding) =
            store.bound(runtime, &kept.conversation)?
        else {
            return Ok(Vec::new());
        };
        Ok(if store.keep_item(binding.id, &kept.item)? {
            vec![Notice::Kept {
                binding: binding.id,
                item: kept.item,
            }]
        } else {
            Vec::new()
        })
    }

    pub(super) fn live(
        &self,
        store: &Store,
        index: usize,
        live: link::Live,
    ) -> Result<Vec<Notice>> {
        let runtime = &self.runtimes[index].id;
        Ok(match store.bound(runtime, &live.conversation)? {
            Some(binding) => vec![Notice::Live {
                binding: binding.id,
                running: live.running,
                pending: live.pending,
                partial: live.partial,
            }],
            None => Vec::new(),
        })
    }
}

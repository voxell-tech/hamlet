//! File work whose completion returns to the service loop.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde_json::json;
use tokio::task::JoinSet;

use crate::OperationId;
use crate::harness::Harnesses;
use crate::papers::tree::Entry;
use crate::service::Service;

/// House work running apart from the service loop, and what waits
/// for it, by the operation that started it.
#[derive(Default)]
pub(crate) struct Work {
    tasks: JoinSet<Completed>,
    pending: HashSet<OperationId>,
    offers: HashMap<OperationId, Result<BTreeMap<String, Entry>>>,
    cleaned: HashMap<OperationId, Result<bool>>,
    founding: HashSet<(String, u64)>,
}

pub(crate) enum Completed {
    Founded {
        hamlet: String,
        vote: u64,
        result: Result<()>,
    },
    Initialized {
        id: OperationId,
        result: Result<()>,
    },
    Offered {
        id: OperationId,
        result: Result<BTreeMap<String, Entry>>,
    },
    Cleaned {
        id: OperationId,
        result: Result<bool>,
    },
}

impl Work {
    /// The next piece of work that finished.
    pub async fn next(
        &mut self,
    ) -> Option<Result<Completed, tokio::task::JoinError>> {
        self.tasks.join_next().await
    }

    /// Whether the operation `id` waits for its work.
    pub fn pending(&self, id: &OperationId) -> bool {
        self.pending.contains(id)
    }

    /// Marks the operation `id` as waiting for work; false when it
    /// already was.
    pub fn hold(&mut self, id: OperationId) -> bool {
        self.pending.insert(id)
    }

    /// Marks the operation `id` as no longer waiting.
    pub fn release(&mut self, id: &OperationId) {
        self.pending.remove(id);
    }

    /// Reads the files a skill offer of `id` brings with `read`.
    pub fn offer(
        &mut self,
        id: OperationId,
        read: impl Future<Output = Result<BTreeMap<String, Entry>>>
        + Send
        + 'static,
    ) {
        self.pending.insert(id.clone());
        self.tasks.spawn(async move {
            Completed::Offered {
                id,
                result: read.await,
            }
        });
    }

    /// Takes a shelved skill out of the house for `id` with `clean`,
    /// which says whether the house's copy was unchanged.
    pub fn clean(
        &mut self,
        id: OperationId,
        clean: impl Future<Output = Result<bool>> + Send + 'static,
    ) {
        self.pending.insert(id.clone());
        self.tasks.spawn(async move {
            Completed::Cleaned {
                id,
                result: clean.await,
            }
        });
    }

    /// The offered files read for `id`, once.
    pub fn take_offer(
        &mut self,
        id: &OperationId,
    ) -> Option<Result<BTreeMap<String, Entry>>> {
        self.offers.remove(id)
    }

    /// Whether the house's copy was unchanged when it was cleaned for
    /// `id`, once.
    pub fn take_cleaned(
        &mut self,
        id: &OperationId,
    ) -> Option<Result<bool>> {
        self.cleaned.remove(id)
    }

    /// Prepares the house of the mayor of the hamlet founded by vote
    /// `vote` of `hamlet` with `initialize`.
    pub fn found(
        &mut self,
        hamlet: String,
        vote: u64,
        initialize: impl Future<Output = Result<()>> + Send + 'static,
    ) {
        self.founding.insert((hamlet.clone(), vote));
        self.tasks.spawn(async move {
            let result = initialize.await;
            Completed::Founded {
                hamlet,
                vote,
                result,
            }
        });
    }

    /// Whether the hamlet founded by vote `vote` of `hamlet` is still
    /// being prepared.
    pub fn founding(&self, hamlet: &str, vote: u64) -> bool {
        self.founding.contains(&(hamlet.into(), vote))
    }

    /// Forgets the read and cleaned results of calls that stopped.
    pub fn retire(
        &mut self,
        db: &rusqlite::Connection,
    ) -> Result<()> {
        let ids = self
            .offers
            .keys()
            .chain(self.cleaned.keys())
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            if !crate::oversight::held::unfinished(
                db,
                id.runtime.as_str(),
                &id.key,
            )? {
                self.offers.remove(&id);
                self.cleaned.remove(&id);
            }
        }
        Ok(())
    }

    /// Prepares the house of the agent recorded by `id` with
    /// `initialize`.
    pub fn initialize(
        &mut self,
        id: OperationId,
        initialize: impl Future<Output = Result<()>> + Send + 'static,
    ) {
        self.pending.insert(id.clone());
        self.tasks.spawn(async move {
            let result = initialize.await;
            Completed::Initialized { id, result }
        });
    }
}

#[expect(clippy::print_stderr, reason = "stderr is the server's log")]
pub(crate) async fn complete(
    service: &mut Service,
    harnesses: &mut Harnesses,
    completed: Completed,
) -> Result<()> {
    match completed {
        Completed::Founded {
            hamlet,
            vote,
            result,
        } => {
            service.house_work.founding.remove(&(hamlet, vote));
            if let Err(error) = result {
                let mut diagnostic = json!(format!(
                    "Could not prepare the founded hamlet's house: \
                     {error}"
                ));
                service.credentials.scrub(&mut diagnostic);
                eprintln!("{diagnostic}");
            }
            super::votes::changed(service, harnesses).await?;
        }
        Completed::Offered { id, result } => {
            service.house_work.pending.remove(&id);
            if crate::oversight::held::unfinished(
                service.home.store().connection(),
                id.runtime.as_str(),
                &id.key,
            )? {
                harnesses.wake(&id.runtime, &id.key).await;
                service.house_work.offers.insert(id, result);
            }
        }
        Completed::Cleaned { id, result } => {
            service.house_work.pending.remove(&id);
            if crate::oversight::held::unfinished(
                service.home.store().connection(),
                id.runtime.as_str(),
                &id.key,
            )? {
                harnesses.wake(&id.runtime, &id.key).await;
                service.house_work.cleaned.insert(id, result);
            }
        }
        Completed::Initialized { id, result } => {
            service.house_work.pending.remove(&id);
            if let Err(error) = result {
                let mut diagnostic = json!(format!(
                    "Could not prepare the recorded agent's house: \
                     {error}"
                ));
                service.credentials.scrub(&mut diagnostic);
                eprintln!("{diagnostic}");
            }
            harnesses.wake(&id.runtime, &id.key).await;
        }
    }
    Ok(())
}

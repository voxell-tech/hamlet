//! File work whose completion returns to the service loop.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde_json::json;
use tokio::task::JoinSet;

use crate::OperationId;
use crate::harness::Harnesses;
use crate::papers::tree::Entry;
use crate::service::Service;
use crate::workspace::Remote;

#[derive(Default)]
pub(crate) struct Work {
    pub tasks: JoinSet<Completed>,
    pub pending: HashSet<(String, String)>,
    pub offers:
        HashMap<(String, String), Result<BTreeMap<String, Entry>>>,
    pub cleaned: HashMap<(String, String), Result<bool>>,
    pub founding: HashSet<(String, u64)>,
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
    pub fn retire(
        &mut self,
        db: &rusqlite::Connection,
    ) -> Result<()> {
        let keys = self
            .offers
            .keys()
            .chain(self.cleaned.keys())
            .cloned()
            .collect::<Vec<_>>();
        for (runtime, key) in keys {
            if !super::library::active(db, &runtime, &key)? {
                self.offers.remove(&(runtime.clone(), key.clone()));
                self.cleaned.remove(&(runtime, key));
            }
        }
        Ok(())
    }

    pub fn initialize(&mut self, id: OperationId, remote: Remote) {
        self.pending
            .insert((id.runtime.as_str().into(), id.key.clone()));
        self.tasks.spawn(async move {
            let result =
                remote.initialize().await.map_err(Into::into);
            Completed::Initialized { id, result }
        });
    }
}

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
                    "Could not prepare the founded hamlet's remote \
                     house: {error}"
                ));
                service.credentials.scrub(&mut diagnostic);
                eprintln!("{diagnostic}");
            }
            super::votes::changed(service, harnesses).await?;
        }
        Completed::Offered { id, result } => {
            let key = (id.runtime.as_str().into(), id.key.clone());
            service.house_work.pending.remove(&key);
            if super::library::active(
                service.home.store().connection(),
                id.runtime.as_str(),
                &id.key,
            )? {
                service.house_work.offers.insert(key, result);
                harnesses.wake(&id.runtime, &id.key).await;
            }
        }
        Completed::Cleaned { id, result } => {
            let key = (id.runtime.as_str().into(), id.key.clone());
            service.house_work.pending.remove(&key);
            if super::library::active(
                service.home.store().connection(),
                id.runtime.as_str(),
                &id.key,
            )? {
                service.house_work.cleaned.insert(key, result);
                harnesses.wake(&id.runtime, &id.key).await;
            }
        }
        Completed::Initialized { id, result } => {
            service.house_work.pending.remove(&(
                id.runtime.as_str().into(),
                id.key.clone(),
            ));
            if let Err(error) = result {
                let mut diagnostic = json!(format!(
                    "Could not prepare the recorded agent's remote \
                     house: {error}"
                ));
                service.credentials.scrub(&mut diagnostic);
                eprintln!("{diagnostic}");
            }
            harnesses.wake(&id.runtime, &id.key).await;
        }
    }
    Ok(())
}

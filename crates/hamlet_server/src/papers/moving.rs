//! Copies living houses to the next-start backend while the active
//! backend stays selected.

use anyhow::Result;
use hamlet_workspace_wire::{TREE_BYTES, TREE_ENTRIES};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::Houses;
use super::copy::Snapshot;
use crate::protocol::Refusal;
use crate::record::AgentRef;
use crate::service::Service;

pub(crate) struct Moving {
    pub tasks: JoinSet<(u64, String, Result<String, Refusal>)>,
    progress: watch::Receiver<Value>,
}

impl Default for Moving {
    fn default() -> Self {
        Self {
            tasks: JoinSet::new(),
            progress: watch::channel(Value::Null).1,
        }
    }
}

impl Moving {
    pub fn view(&self) -> Value {
        self.progress.borrow().clone()
    }
}

impl Service {
    pub(crate) fn start_move(
        &mut self,
        owner: u64,
        id: &str,
    ) -> Result<Result<(), Refusal>> {
        let refuse =
            |text: &str| Ok(Err(Refusal::Invalid(text.into())));
        if self.overrides.workspace.is_some() {
            return refuse(
                "HAMLET_WORKSPACE_URL chooses the workspace for \
                 this run, so houses are not moved to the one in \
                 Settings.",
            );
        }
        let from = self.houses.workspace();
        let to = self.credentials.workspace();
        if from.map(|c| (&c.url, &c.token))
            == to.map(|c| (&c.url, &c.token))
        {
            return refuse(
                "Hamlet already works in the workspace chosen in \
                 Settings, so there are no houses to move.",
            );
        }
        if !self.moving.tasks.is_empty() {
            return refuse("The houses are already moving.");
        }
        let to = to.cloned();
        let live =
            crate::realm::live(self.home.store().connection())?;
        let agents =
            crate::service::records(self.home.store().connection())?
                .into_iter()
                .filter(|record| live.contains(&record.id))
                .flat_map(|record| {
                    record.agents.into_iter().map(move |agent| {
                        AgentRef {
                            hamlet: record.id.clone(),
                            number: agent.number,
                        }
                    })
                })
                .collect::<Vec<_>>();
        let root = self.home.layout().root();
        let source = Houses::open(root, from.cloned())?;
        let target = Houses::open(root, to.clone())?;
        let (progress, receiver) = watch::channel(
            json!({"state":"moving","done":0,"of":agents.len(),"text":""}),
        );
        self.moving.progress = receiver;
        let id = id.to_owned();
        self.moving.tasks.spawn(async move {
            let mut skipped = Vec::new();
            for (done, agent) in agents.iter().enumerate() {
                match move_house(&source, &target, agent).await {
                    Ok(left) => skipped.extend(left.into_iter().map(|path| format!("{}/{}/{path}",agent.hamlet, agent.number))),
                    Err(error) => {
                        let text = format!("The house of agent {} in {} was not moved: {error} {done} houses before it moved; moving again copies them all once more. Inspect the target before retrying.", agent.number, agent.hamlet);
                        progress.send_replace(json!({"state":"failed","done":done,"of":agents.len(),"text":text}));
                        return (owner, id, Err(Refusal::Invalid(text)));
                    }
                }
                progress.send_replace(json!({"state":"moving","done":done+1,"of":agents.len(),"text":""}));
            }
            let mut text = format!("Moved {} {} to {}. Restart Hamlet to work there; what agents change before then stays behind.", agents.len(), if agents.len() == 1 {"house"} else {"houses"}, if to.is_some() {"the remote server"} else {"this computer"});
            if !skipped.is_empty() { text.push_str(&format!(" Links and special files are not moved: {}{}.", skipped.iter().take(5).cloned().collect::<Vec<_>>().join(", "), if skipped.len() > 5 {format!(", and {} more",skipped.len()-5)} else {String::new()})); }
            progress.send_replace(json!({"state":"moved","done":agents.len(),"of":agents.len(),"text":text}));
            (owner, id, Ok(text))
        });
        Ok(Ok(()))
    }
}

async fn move_house(
    source: &Houses,
    target: &Houses,
    agent: &AgentRef,
) -> Result<Vec<String>> {
    let snapshot = Snapshot::read(
        &source.house(&agent.hamlet, agent.number)?,
        TREE_BYTES,
        TREE_ENTRIES as usize,
        true,
    )
    .await?;
    target
        .replace(&agent.hamlet, agent.number, &snapshot)
        .await?;
    Ok(snapshot.skipped)
}

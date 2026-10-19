//! Copies living houses to the next-start backend while the active
//! backend stays selected.

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::copy::{Snapshot, chmod, same};
use super::files::remote_entries;
use super::{Houses, tree};
use crate::credentials::Workspace;
use crate::protocol::Refusal;
use crate::record::AgentRef;
use crate::service::Service;
use crate::workspace::Remote;

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
        let from = self.houses.workspace.as_ref();
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
        let mut source = Houses::open(self.home.layout().root())?;
        source.workspace = self.houses.workspace.clone();
        let local = Houses::open(self.home.layout().root())?;
        let (progress, receiver) = watch::channel(
            json!({"state":"moving","done":0,"of":agents.len(),"text":""}),
        );
        self.moving.progress = receiver;
        let id = id.to_owned();
        self.moving.tasks.spawn(async move {
            let mut skipped = Vec::new();
            for (done, agent) in agents.iter().enumerate() {
                match move_house(&source, &local, agent, to.as_ref()).await {
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
    local: &Houses,
    agent: &AgentRef,
    to: Option<&Workspace>,
) -> Result<Vec<String>> {
    let snapshot = Snapshot::read(
        &source.files(&agent.hamlet, agent.number)?,
        512 * 1024 * 1024,
        20_000,
        true,
    )
    .await?;
    if let Some(to) = to {
        let remote = Remote::new(
            to.clone(),
            agent.clone(),
            false,
            false,
            false,
        )?;
        replace_remote(&snapshot, &remote).await?;
        Ok(snapshot.skipped)
    } else {
        let root = local.root().try_clone()?;
        let agent = agent.clone();
        tokio::task::spawn_blocking(move || {
            use rustix::fs::{AtFlags, renameat, unlinkat};
            let stages = tree::directory(&root, ".gifts", true)?;
            let name = format!("move-{}", uuid::Uuid::new_v4());
            let stage = tree::directory(&stages, &name, true)?;
            let result = (|| {
                snapshot.write_local(&stage)?;
                let houses = tree::directory(&root, "houses", true)?;
                let parent =
                    tree::directory(&houses, &agent.hamlet, true)?;
                let number = agent.number.to_string();
                let backup = format!("old-{}", uuid::Uuid::new_v4());
                let detached = match renameat(
                    &parent,
                    number.as_str(),
                    &stages,
                    backup.as_str(),
                ) {
                    Ok(()) => true,
                    Err(rustix::io::Errno::NOENT) => false,
                    Err(error) => return Err(error.into()),
                };
                if let Err(error) = renameat(
                    &stages,
                    name.as_str(),
                    &parent,
                    number.as_str(),
                ) {
                    if detached {
                        renameat(
                            &stages,
                            backup.as_str(),
                            &parent,
                            number.as_str(),
                        )?;
                    }
                    return Err(error.into());
                }
                if detached {
                    match tree::directory(&stages, &backup, false) {
                        Ok(_) => tree::erase(&stages, &backup)?,
                        Err(_) => unlinkat(
                            &stages,
                            backup.as_str(),
                            AtFlags::empty(),
                        )?,
                    }
                }
                Ok(())
            })();
            if result.is_err() {
                let _ = tree::erase(&stages, &name);
            }
            result.map(|()| snapshot.skipped)
        })
        .await?
    }
}

async fn replace_remote(
    snapshot: &Snapshot,
    remote: &Remote,
) -> Result<()> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    ensure!(
        snapshot
            .files
            .values()
            .all(|entry| entry.bytes.len()
                <= crate::workspace::MAX_FILE),
        "Remote files are limited to 4 MiB per transfer."
    );
    remote.initialize().await?;
    let listed =
        remote_entries(remote, "/house", 512 * 1024 * 1024, 20_000)
            .await?;
    let mut kept = std::collections::BTreeSet::new();
    let mut removed = Vec::<String>::new();
    for entry in listed {
        if entry.path == "."
            || removed.iter().any(|parent| {
                entry.path.starts_with(&format!("{parent}/"))
            })
        {
            continue;
        }
        if snapshot.entries.iter().any(|wanted| {
            wanted.path == entry.path
                && (same(wanted, &entry)
                    || wanted.kind == "directory"
                        && entry.kind == "directory")
        }) {
            kept.insert(entry.path);
        } else {
            remote
                .parents(&format!("/house/{}", entry.path), false)
                .await
                .map_err(|error| {
                    anyhow::anyhow!("{}", error.message)
                })?;
            remote.call::<()>(json!({"kind":"remove","path":format!("/house/{}",entry.path),"recursive":true,"force":true})).await?;
            removed.push(entry.path);
        }
    }
    for entry in &snapshot.entries {
        if entry.path == "." || kept.contains(&entry.path) {
            continue;
        }
        let path = format!("/house/{}", entry.path);
        remote
            .parents(&path, false)
            .await
            .map_err(|error| anyhow::anyhow!("{}", error.message))?;
        if entry.kind == "directory" {
            remote.call::<()>(json!({"kind":"mkdir","path":path,"recursive":false})).await?;
        } else {
            remote.call::<()>(json!({"kind":"tree_write","path":path,"data":STANDARD.encode(&snapshot.files[&entry.path].bytes),"mode":entry.mode & 0o777})).await?;
        }
    }
    for entry in snapshot.entries.iter().rev().filter(|entry| {
        entry.path != "." && entry.kind == "directory"
    }) {
        chmod(remote, &format!("/house/{}", entry.path), entry.mode)
            .await?;
    }
    let after =
        remote_entries(remote, "/house", 512 * 1024 * 1024, 20_000)
            .await?;
    ensure!(
        after.len() == snapshot.entries.len()
            && snapshot.entries.iter().all(|entry| after.iter().any(
                |actual| actual.path == entry.path
                    && (entry.path == "."
                        && actual.kind == "directory"
                        || same(actual, entry))
            )),
        "The copy in the remote workspace did not match it \
         afterwards."
    );
    Ok(())
}

//! The chief's terminal requests.

use anyhow::Result;
use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{
    Reply, ReplyKind, RequestId, ServerMessage, Terminal as Asked,
};
use hamlet_store::ids::HamletId;

use super::screen::lock;
use super::shell::{Key, Owner, open};
use crate::core::Core;

impl Core {
    /// Answers the chief's terminal request `asked` from `client`.
    pub(crate) async fn terminal(
        &mut self,
        client: u64,
        asked: Asked,
    ) -> Result<()> {
        let answers = match asked {
            Asked::Open {
                cols,
                hamlet,
                id,
                rows,
            } => {
                let size = Size { rows, cols };
                let owner = match hamlet {
                    Some(hamlet) => Owner::Hamlet(HamletId(hamlet)),
                    None => Owner::Unbounded,
                };
                let answer =
                    match open(self, &owner, None, size, true)? {
                        Ok(terminal) => self.terminals.watch(
                            client,
                            &terminal,
                            Some(&id),
                        ),
                        Err(words) => Some(said(&id, Err(words))),
                    };
                self.publish().await?;
                answer.into_iter().collect()
            }
            Asked::Input { data, terminal } => {
                self.terminals.type_in(&terminal, data.into_bytes());
                Vec::new()
            }
            Asked::Resize {
                cols,
                rows,
                terminal,
            } => {
                let size = Size { rows, cols };
                if let Some(open) =
                    self.terminals.open.get_mut(&terminal)
                {
                    open.view.rows = size.rows;
                    open.view.cols = size.cols;
                    lock(&open.screen).resize(size);
                    if let Some(keys) = &open.keys {
                        let _ = keys.send(Key::Resize(size));
                    }
                }
                Vec::new()
            }
            Asked::Watch { id, terminal } => {
                match self.terminals.watch(
                    client,
                    &terminal,
                    Some(&id),
                ) {
                    Some(output) => vec![output],
                    None => vec![said(&id, Err(gone(&terminal)))],
                }
            }
            Asked::Unwatch { terminal } => {
                if let Some(open) =
                    self.terminals.open.get_mut(&terminal)
                {
                    open.watchers.remove(&client);
                }
                Vec::new()
            }
            Asked::Close { id, terminal } => {
                let result = if self.terminals.close(&terminal) {
                    Ok("Closed.".into())
                } else {
                    Err(gone(&terminal))
                };
                self.publish().await?;
                vec![said(&id, result)]
            }
            Asked::Rename { id, name, terminal } => {
                let name = name.trim();
                let result = if name.is_empty() {
                    Err("A terminal needs a name.".into())
                } else if self.terminals.rename(&terminal, name) {
                    Ok("Renamed.".into())
                } else {
                    Err(gone(&terminal))
                };
                self.publish().await?;
                vec![said(&id, result)]
            }
        };
        for answer in answers {
            self.answer(client, answer)?;
        }
        Ok(())
    }
}

/// The reply to request `id`: a notice, or an error in words.
fn said(
    id: &RequestId,
    result: Result<String, String>,
) -> ServerMessage {
    let (kind, text) = match result {
        Ok(text) => (ReplyKind::Notice, text),
        Err(text) => (ReplyKind::Error, text),
    };
    ServerMessage::Reply(Reply {
        file: None,
        files: Vec::new(),
        image: None,
        images: Vec::new(),
        to: id.clone(),
        kind,
        text,
        upload: None,
    })
}

fn gone(terminal: &str) -> String {
    format!("There is no terminal `{terminal}`.")
}

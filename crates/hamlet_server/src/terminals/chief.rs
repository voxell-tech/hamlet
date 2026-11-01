//! The chief's terminal requests.

use anyhow::Result;
use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{ServerMessage, Terminal as Asked};

use super::screen::lock;
use super::shell::{Key, open};
use crate::protocol::{Refusal, reply};
use crate::service::Service;

/// Answers the chief's terminal request `asked` from connection
/// `connection`.
pub(crate) fn request(
    service: &mut Service,
    connection: u64,
    asked: &Asked,
) -> Result<Vec<ServerMessage>> {
    Ok(match asked {
        Asked::Open {
            cols,
            hamlet,
            id,
            rows,
        } => {
            let size = Size {
                rows: *rows,
                cols: *cols,
            };
            match open(
                service,
                hamlet.as_deref(),
                None,
                None,
                size,
                true,
            )? {
                Ok(terminal) => service
                    .terminals
                    .watch(connection, &terminal, Some(id))
                    .into_iter()
                    .collect(),
                Err(refusal) => vec![reply(id, Err(refusal))],
            }
        }
        Asked::Input { data, terminal } => {
            service
                .terminals
                .type_in(terminal, data.as_bytes().to_vec());
            Vec::new()
        }
        Asked::Resize {
            cols,
            rows,
            terminal,
        } => {
            let size = Size {
                rows: *rows,
                cols: *cols,
            };
            if let Some(open) =
                service.terminals.open.get_mut(terminal)
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
            match service.terminals.watch(
                connection,
                terminal,
                Some(id),
            ) {
                Some(output) => vec![output],
                None => vec![reply(id, Err(gone(terminal)))],
            }
        }
        Asked::Unwatch { terminal } => {
            if let Some(open) =
                service.terminals.open.get_mut(terminal)
            {
                open.watchers.remove(&connection);
            }
            Vec::new()
        }
        Asked::Close { id, terminal } => {
            let result = if service.terminals.close(terminal) {
                Ok("Closed.".into())
            } else {
                Err(gone(terminal))
            };
            vec![reply(id, result)]
        }
        Asked::Rename { id, name, terminal } => {
            let name = name.trim();
            let result = if name.is_empty() {
                Err(Refusal::Invalid(
                    "A terminal needs a name.".into(),
                ))
            } else if service.terminals.rename(terminal, name) {
                Ok("Renamed.".into())
            } else {
                Err(gone(terminal))
            };
            vec![reply(id, result)]
        }
    })
}

fn gone(terminal: &str) -> Refusal {
    Refusal::Invalid(format!("There is no terminal `{terminal}`."))
}

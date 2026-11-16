//! The chief's terminal requests.

use bevy_ecs::error::Result;
use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{
    Reply, ReplyKind, RequestId, ServerMessage, Terminal as Asked,
};

use super::screen::lock;
use super::shell::{Key, open};
use crate::clients::asked::FromClient;
use crate::core::Core;
use crate::server::InputMessage;

/// Answers the chief's terminal request from a client.
pub(super) fn asked(
    mut asked: InputMessage<FromClient<Asked>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: asked,
    }) = asked.take()
    else {
        return Ok(());
    };
    let answers = match asked {
        Asked::Open { cols, id, rows } => {
            let terminal = open(&mut core, Size { rows, cols });
            let answer =
                core.terminals.watch(client, &terminal, Some(&id));
            core.publish();
            answer.into_iter().collect()
        }
        Asked::Input { data, terminal } => {
            core.terminals.type_in(&terminal, data.into_bytes());
            Vec::new()
        }
        Asked::Resize {
            cols,
            rows,
            terminal,
        } => {
            let size = Size { rows, cols };
            if let Some(open) = core.terminals.open.get_mut(&terminal)
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
            match core.terminals.watch(client, &terminal, Some(&id)) {
                Some(output) => vec![output],
                None => vec![said(&id, Err(gone(&terminal)))],
            }
        }
        Asked::Unwatch { terminal } => {
            if let Some(open) = core.terminals.open.get_mut(&terminal)
            {
                open.watchers.remove(&client);
            }
            Vec::new()
        }
        Asked::Close { id, terminal } => {
            let result = if core.terminals.close(&terminal) {
                Ok("Closed.".into())
            } else {
                Err(gone(&terminal))
            };
            core.publish();
            vec![said(&id, result)]
        }
        Asked::Rename { id, name, terminal } => {
            let name = name.trim();
            let result = if name.is_empty() {
                Err("A terminal needs a name.".into())
            } else if core.terminals.rename(&terminal, name) {
                Ok("Renamed.".into())
            } else {
                Err(gone(&terminal))
            };
            core.publish();
            vec![said(&id, result)]
        }
    };
    for answer in answers {
        core.answer(client, answer)?;
    }
    Ok(())
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

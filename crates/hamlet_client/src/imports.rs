//! Conversations of other programs brought in as a session of a chat
//! (C35): found and read on the runtime's blocking threads, sent as a
//! file the way attached files are, then named in `import`.

use std::path::PathBuf;

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use hamlet_import::Found;
use hamlet_protocol::client as protocol;
use protocol::{AgentId, Imported};
use tokio::sync::oneshot;

use crate::files::{
    FilesForget, FilesKept, FilesRefused, Gather, Gathered,
    UploadFiles, off_frame,
};
use crate::messages::{Reconnected, Response};
use crate::outbox::{Outbox, RequestId};

/// The key the conversation brought in is sent under, apart from the
/// keys of a line's attachments.
const KEY: u64 = u64::MAX;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Imports>()
        .add_observer(find)
        .add_observer(bring_in)
        .add_observer(gathered)
        .add_observer(kept)
        .add_observer(refused)
        .add_observer(answered)
        .add_observer(lost)
        .add_systems(Update, collect.run_if(waiting));
}

/// Looks for conversations: those in the export `file`, or those on
/// this machine.
#[derive(Event, Clone, Debug)]
pub struct FindImports {
    pub file: Option<PathBuf>,
}

/// Brings in `found` as a new session of the chief's chat with `to`.
#[derive(Event, Clone, Debug)]
pub struct BringIn {
    pub found: Found,
    pub to: AgentId,
}

/// A conversation was brought in as a session of the chat with `to`.
#[derive(Event, Clone, Copy, Debug)]
pub struct BroughtIn {
    pub to: AgentId,
}

/// Where bringing a conversation in stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Step {
    #[default]
    Idle,
    Reading,
    Sending,
    Done(String),
    Failed(String),
}

impl Step {
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Reading | Self::Sending)
    }
}

/// The conversations found and the one being brought in.
#[derive(Resource, Default)]
pub struct Imports {
    /// The conversations last found, newest first.
    pub found: Vec<Found>,
    /// Looking for them now.
    pub looking: bool,
    /// Why the last look found nothing.
    pub unread: Option<String>,
    pub step: Step,
    to: Option<AgentId>,
    /// The file the conversation is written to for sending.
    file: Option<PathBuf>,
    pending: Option<RequestId>,
    finding: Option<oneshot::Receiver<Result<Vec<Found>, String>>>,
    reading: Option<oneshot::Receiver<Result<PathBuf, String>>>,
}

fn waiting(imports: Res<Imports>) -> bool {
    imports.finding.is_some() || imports.reading.is_some()
}

fn find(find: On<FindImports>, mut commands: Commands) {
    let file = find.file.clone();
    commands.queue(move |world: &mut World| {
        let done = off_frame(world, move || match file {
            Some(file) => hamlet_import::find_in(&file)
                .map_err(|e| format!("{e:#}")),
            None => Ok(hamlet_import::find()),
        });
        let mut imports = world.resource_mut::<Imports>();
        imports.looking = done.is_some();
        imports.finding = done;
    });
}

fn bring_in(bring: On<BringIn>, mut commands: Commands) {
    let BringIn { found, to } = bring.event().clone();
    commands.queue(move |world: &mut World| {
        if world.resource::<Imports>().step.is_busy() {
            return;
        }
        let done = off_frame(world, move || written(&found));
        let mut imports = world.resource_mut::<Imports>();
        imports.to = Some(to);
        imports.step = match done {
            Some(_) => Step::Reading,
            None => Step::Failed(
                "The window has no runtime to read with.".into(),
            ),
        };
        imports.reading = done;
    });
}

/// `found` read and written to a file of its own for sending.
fn written(found: &Found) -> Result<PathBuf, String> {
    let imported =
        hamlet_import::read(found).map_err(|e| format!("{e:#}"))?;
    let bytes =
        serde_json::to_vec(&imported).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > Imported::MOST_BYTES {
        return Err("That conversation is over 16 MiB, more than \
                    Hamlet takes."
            .into());
    }
    let folder = std::env::temp_dir().join("hamlet-imports");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!("{}.json", std::process::id()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn collect(world: &mut World) {
    let mut imports = world.resource_mut::<Imports>();
    if let Some(finding) = imports.finding.as_mut() {
        match finding.try_recv() {
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(oneshot::error::TryRecvError::Closed) => {
                imports.finding = None;
                imports.looking = false;
            }
            Ok(found) => {
                imports.finding = None;
                imports.looking = false;
                match found {
                    Ok(found) => {
                        imports.found = found;
                        imports.unread = None;
                    }
                    Err(why) => {
                        imports.found.clear();
                        imports.unread = Some(why);
                    }
                }
            }
        }
    }
    let Some(reading) = imports.reading.as_mut() else {
        return;
    };
    let read = match reading.try_recv() {
        Err(oneshot::error::TryRecvError::Empty) => return,
        Err(oneshot::error::TryRecvError::Closed) => {
            Err("The reading stopped.".into())
        }
        Ok(read) => read,
    };
    imports.reading = None;
    match read {
        Ok(path) => {
            imports.step = Step::Sending;
            imports.file = Some(path.clone());
            world.trigger(Gather {
                key: KEY,
                paths: vec![path],
            });
        }
        Err(why) => imports.step = Step::Failed(why),
    }
}

fn gathered(gathered: On<Gathered>, mut commands: Commands) {
    if gathered.key == KEY {
        commands.trigger(UploadFiles { key: KEY });
    }
}

fn kept(
    kept: On<FilesKept>,
    mut imports: ResMut<Imports>,
    mut outbox: ResMut<Outbox>,
) {
    if kept.key != KEY {
        return;
    }
    forget_file(&mut imports);
    let (Some(file), Some(to)) = (kept.files.first(), imports.to)
    else {
        imports.step =
            Step::Failed("The server kept no file.".into());
        return;
    };
    let file = file.id.clone();
    imports.pending = Some(outbox.send(|id| {
        protocol::ClientMessage::Import(protocol::Import {
            file,
            id,
            to,
        })
    }));
}

fn refused(refused: On<FilesRefused>, mut imports: ResMut<Imports>) {
    if refused.key == KEY {
        forget_file(&mut imports);
        imports.step = Step::Failed(refused.text.clone());
    }
}

fn answered(
    response: On<Response>,
    mut imports: ResMut<Imports>,
    mut commands: Commands,
) {
    if imports.pending.as_ref() != Some(&response.to) {
        return;
    }
    imports.pending = None;
    imports.step = match &response.message {
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Notice =>
        {
            if let Some(to) = imports.to {
                commands.trigger(BroughtIn { to });
            }
            Step::Done(reply.text.clone())
        }
        protocol::ServerMessage::Reply(reply) => {
            Step::Failed(reply.text.clone())
        }
        protocol::ServerMessage::Fault(fault) => {
            Step::Failed(fault.text.clone())
        }
        _ => return,
    };
}

/// A reply lost with the connection never comes; the chief may try
/// again.
fn lost(
    _: On<Reconnected>,
    mut imports: ResMut<Imports>,
    mut commands: Commands,
) {
    if imports.step.is_busy() {
        forget_file(&mut imports);
        imports.reading = None;
        imports.pending = None;
        imports.step = Step::Failed(
            "The connection was lost on the way; try again.".into(),
        );
        commands.trigger(FilesForget { key: KEY });
    }
}

fn forget_file(imports: &mut Imports) {
    if let Some(file) = imports.file.take() {
        let _ = std::fs::remove_file(file);
    }
}

//! Projects of other programs brought in (C35): found and read on the
//! runtime's blocking threads, their conversations sent as files the
//! way attached files are, then each project named in
//! `import-project`, one after another.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::SystemTime;

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use hamlet_import::Project;
use hamlet_protocol::client as protocol;
use protocol::Imported;
use tokio::sync::oneshot;

use crate::files::{
    FilesForget, FilesKept, FilesRefused, Gather, Gathered,
    UploadFiles, off_frame,
};
use crate::messages::{Reconnected, Response};
use crate::outbox::{Outbox, RequestId};

/// The key a project's conversations are sent under, apart from the
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

/// Looks for projects: those of the export `file`, or those of this
/// machine in the last 30 days.
#[derive(Event, Clone, Debug)]
pub struct FindProjects {
    pub file: Option<PathBuf>,
}

/// Brings in each of `projects`, in turn.
#[derive(Event, Clone, Debug)]
pub struct BringIn {
    pub projects: Vec<Project>,
}

/// What the server said of one project brought in, or why it was not.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub name: String,
    pub said: Result<String, String>,
}

/// The projects found and those being brought in.
#[derive(Resource, Default)]
pub struct Imports {
    /// The projects last found, newest first.
    pub projects: Vec<Project>,
    /// Looking for them now.
    pub looking: bool,
    /// Why the last look found nothing.
    pub unread: Option<String>,
    /// The project being brought in, and those still to come.
    pub bringing: Option<String>,
    /// The folder of the project being brought in.
    folder: Option<String>,
    queue: VecDeque<Project>,
    /// What each project brought in so far came to.
    pub outcomes: Vec<Outcome>,
    /// The files the project's conversations were written to.
    files: Vec<PathBuf>,
    pending: Option<RequestId>,
    finding: Option<oneshot::Receiver<Result<Vec<Project>, String>>>,
    reading: Option<oneshot::Receiver<Result<Vec<PathBuf>, String>>>,
}

impl Imports {
    pub fn is_busy(&self) -> bool {
        self.bringing.is_some()
    }
}

fn waiting(imports: Res<Imports>) -> bool {
    imports.finding.is_some() || imports.reading.is_some()
}

fn find(find: On<FindProjects>, mut commands: Commands) {
    let file = find.file.clone();
    commands.queue(move |world: &mut World| {
        let done = off_frame(world, move || {
            let found = match &file {
                Some(file) => hamlet_import::find_in(file)
                    .map_err(|e| format!("{e:#}"))?,
                None => hamlet_import::find(),
            };
            Ok(hamlet_import::projects(
                found,
                SystemTime::now() - hamlet_import::RECENT,
            ))
        });
        let mut imports = world.resource_mut::<Imports>();
        imports.looking = done.is_some();
        imports.finding = done;
    });
}

fn bring_in(bring: On<BringIn>, mut commands: Commands) {
    let projects = bring.projects.clone();
    commands.queue(move |world: &mut World| {
        let mut imports = world.resource_mut::<Imports>();
        if imports.is_busy() {
            return;
        }
        imports.outcomes.clear();
        imports.queue = projects.into();
        next(world);
    });
}

/// Reads the next project's conversations, off the frame.
fn next(world: &mut World) {
    let Some(project) =
        world.resource_mut::<Imports>().queue.pop_front()
    else {
        world.resource_mut::<Imports>().bringing = None;
        return;
    };
    let name = project.name.clone();
    let folder = project.folder.clone();
    let done = off_frame(world, move || written(&project));
    let mut imports = world.resource_mut::<Imports>();
    imports.bringing = Some(name.clone());
    imports.folder = folder;
    match done {
        Some(done) => imports.reading = Some(done),
        None => {
            imports.outcomes.push(Outcome {
                name,
                said: Err(
                    "The window has no runtime to read with.".into()
                ),
            });
            imports.queue.clear();
            imports.bringing = None;
        }
    }
}

/// The conversations of `project`, each read and written to a file of
/// its own for sending.
fn written(project: &Project) -> Result<Vec<PathBuf>, String> {
    let folder = std::env::temp_dir()
        .join("hamlet-imports")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (index, found) in project.conversations.iter().enumerate() {
        let Ok(imported) = hamlet_import::read(found) else {
            continue;
        };
        let bytes = serde_json::to_vec(&imported)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > Imported::MOST_BYTES {
            continue;
        }
        let path = folder.join(format!("{index}.json"));
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        paths.push(path);
    }
    if paths.is_empty() {
        return Err("No conversation of it could be read.".into());
    }
    Ok(paths)
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
                    Ok(projects) => {
                        imports.projects = projects;
                        imports.unread = None;
                    }
                    Err(why) => {
                        imports.projects.clear();
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
        Ok(paths) => {
            imports.files.clone_from(&paths);
            world.trigger(Gather { key: KEY, paths });
        }
        Err(why) => failed(world, why),
    }
}

/// Tells why the project being brought in was not, and goes on to the
/// next.
fn failed(world: &mut World, why: String) {
    let mut imports = world.resource_mut::<Imports>();
    forget_files(&mut imports);
    let name = imports.bringing.clone().unwrap_or_default();
    imports.outcomes.push(Outcome {
        name,
        said: Err(why),
    });
    next(world);
}

fn gathered(gathered: On<Gathered>, mut commands: Commands) {
    if gathered.key == KEY {
        commands.trigger(UploadFiles { key: KEY });
    }
}

fn kept(kept: On<FilesKept>, mut commands: Commands) {
    if kept.key != KEY {
        return;
    }
    let files =
        kept.files.iter().map(|file| file.id.clone()).collect();
    commands.queue(move |world: &mut World| {
        let mut imports = world.resource_mut::<Imports>();
        forget_files(&mut imports);
        let name = imports.bringing.clone().unwrap_or_default();
        let folder = imports.folder.clone();
        let message = |id| {
            protocol::ClientMessage::ImportProject(
                protocol::ImportProject {
                    files,
                    folder,
                    id,
                    name: name.chars().take(40).collect(),
                },
            )
        };
        let pending = world.resource_mut::<Outbox>().send(message);
        world.resource_mut::<Imports>().pending = Some(pending);
    });
}

fn refused(refused: On<FilesRefused>, mut commands: Commands) {
    if refused.key != KEY {
        return;
    }
    let why = refused.text.clone();
    commands.queue(move |world: &mut World| failed(world, why));
}

fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        let mut imports = world.resource_mut::<Imports>();
        if imports.pending.as_ref() != Some(&response.to) {
            return;
        }
        imports.pending = None;
        let said = match response.message {
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Notice =>
            {
                Ok(reply.text)
            }
            protocol::ServerMessage::Reply(reply) => Err(reply.text),
            protocol::ServerMessage::Fault(fault) => Err(fault.text),
            _ => return,
        };
        let name = imports.bringing.clone().unwrap_or_default();
        imports.outcomes.push(Outcome { name, said });
        next(world);
    });
}

/// A reply lost with the connection never comes; what was left stops,
/// and the chief may try again.
fn lost(_: On<Reconnected>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        let mut imports = world.resource_mut::<Imports>();
        if !imports.is_busy() {
            return;
        }
        forget_files(&mut imports);
        let name = imports.bringing.take().unwrap_or_default();
        imports.reading = None;
        imports.pending = None;
        imports.queue.clear();
        imports.outcomes.push(Outcome {
            name,
            said: Err("The connection was lost on the way; try \
                       again."
                .into()),
        });
        world.trigger(FilesForget { key: KEY });
    });
}

fn forget_files(imports: &mut Imports) {
    for file in imports.files.drain(..) {
        let _ = std::fs::remove_file(file);
    }
}

//! Projects of other programs brought in (C35): found and read on the
//! runtime's blocking threads, their conversations sent as files the
//! way attached files are, then each project named in
//! `import-project`, one after another.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
/// The projects [`Imports`] lists, and the conversations found
/// in them.
pub use hamlet_import::{Found, Project};
use hamlet_protocol::client as protocol;
use protocol::{ImportProject, Imported};
use tokio::sync::oneshot;

use crate::files::{
    FilesForget, FilesKept, FilesRejected, Gather, Gathered,
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
        .add_observer(rejected)
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
    /// The folder the project's conversations were written to.
    staged: Option<Staged>,
    /// The conversations of the project left out.
    left: Vec<Left>,
    pending: Option<RequestId>,
    finding: Option<oneshot::Receiver<anyhow::Result<Vec<Project>>>>,
    reading: Option<oneshot::Receiver<Result<Written, Unwritten>>>,
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
                Some(file) => hamlet_import::find_in(file)?,
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

/// The conversations of one project, written for sending.
struct Written {
    /// The folder of this reading's own they were written to.
    folder: Staged,
    paths: Vec<PathBuf>,
    /// The conversations picked that are not among them.
    left: Vec<Left>,
}

/// A folder of one reading's files, removed with them when dropped:
/// once they are sent, or when the reading or the window ends first.
struct Staged(PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A conversation picked that is not sent, and why.
#[derive(Debug)]
struct Left {
    title: String,
    why: LeftOut,
}

#[derive(Debug, thiserror::Error)]
enum LeftOut {
    #[error("could not be read: {0:#}")]
    Unread(#[source] anyhow::Error),
    #[error("is over {} MiB", Imported::MOST_BYTES >> 20)]
    TooLarge,
    #[error(
        "would take the project past {} MiB; bring it in alone",
        ImportProject::MOST_BYTES >> 20
    )]
    PastProject,
}

/// Why none of a project's conversations was sent.
#[derive(Debug, thiserror::Error)]
enum Unwritten {
    #[error("No conversation of it could be read.")]
    NoneRead,
    #[error(
        "Its conversations could not be written for sending: {0}"
    )]
    Io(#[from] std::io::Error),
    #[error(
        "Its conversations could not be written for sending: {0}"
    )]
    Json(#[from] serde_json::Error),
}

/// The conversations of `project`, each read and written to a file of
/// its own, in a folder of this reading's own, for sending.
fn written(project: &Project) -> Result<Written, Unwritten> {
    let folder = staging()?;
    let mut written = Written {
        folder,
        paths: Vec::new(),
        left: Vec::new(),
    };
    write_each(project, &mut written)?;
    if written.paths.is_empty() {
        return Err(Unwritten::NoneRead);
    }
    Ok(written)
}

fn write_each(
    project: &Project,
    written: &mut Written,
) -> Result<(), Unwritten> {
    let read = hamlet_import::read_all(&project.conversations);
    let mut together = 0;
    for (index, (found, imported)) in
        project.conversations.iter().zip(read).enumerate()
    {
        let why = match imported {
            Err(error) => LeftOut::Unread(error),
            Ok(imported) => {
                let bytes = serde_json::to_vec(&imported)?;
                let size = bytes.len() as u64;
                if size > Imported::MOST_BYTES {
                    LeftOut::TooLarge
                } else if together + size > ImportProject::MOST_BYTES
                {
                    LeftOut::PastProject
                } else {
                    together += size;
                    let path = written
                        .folder
                        .0
                        .join(format!("{index}.json"));
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?
                        .write_all(&bytes)?;
                    written.paths.push(path);
                    continue;
                }
            }
        };
        written.left.push(Left {
            title: found.title.clone(),
            why,
        });
    }
    Ok(())
}

/// A new folder for one reading's files, that only this user can
/// open: made anew, never one that was there.
fn staging() -> std::io::Result<Staged> {
    static READINGS: AtomicU64 = AtomicU64::new(0);
    let reading = READINGS.fetch_add(1, Ordering::Relaxed);
    let folder = std::env::temp_dir().join(format!(
        "hamlet-import-{}-{reading}",
        std::process::id()
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&folder)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir(&folder)?;
    Ok(Staged(folder))
}

/// The words for the conversations `left` out of a project.
fn left_words(left: &[Left]) -> String {
    let each = left
        .iter()
        .map(|left| format!("“{}” {}", left.title, left.why))
        .collect::<Vec<_>>()
        .join("; ");
    match left.len() {
        1 => format!(" 1 conversation was left out: {each}."),
        count => {
            format!(" {count} conversations were left out: {each}.")
        }
    }
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
                        imports.unread = Some(format!("{why:#}"));
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
            Err("The reading stopped.".to_owned())
        }
        Ok(read) => read.map_err(|why| why.to_string()),
    };
    imports.reading = None;
    match read {
        Ok(written) => {
            imports.staged = Some(written.folder);
            imports.left = written.left;
            world.trigger(Gather {
                key: KEY,
                paths: written.paths,
            });
        }
        Err(why) => failed(world, why),
    }
}

/// Tells why the project being brought in was not, and goes on to the
/// next.
fn failed(world: &mut World, why: String) {
    let mut imports = world.resource_mut::<Imports>();
    forget_files(&mut imports);
    imports.left.clear();
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
            protocol::ClientMessage::ImportProject(ImportProject {
                files,
                folder,
                id,
                name: name.chars().take(40).collect(),
            })
        };
        let pending = world.resource_mut::<Outbox>().send(message);
        world.resource_mut::<Imports>().pending = Some(pending);
    });
}

fn rejected(rejected: On<FilesRejected>, mut commands: Commands) {
    if rejected.key != KEY {
        return;
    }
    let why = rejected.text.clone();
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
        let left = std::mem::take(&mut imports.left);
        let said = match said {
            Ok(text) if !left.is_empty() => {
                Ok(format!("{text}{}", left_words(&left)))
            }
            said => said,
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
        // What it writes is dropped when it ends, its folder with it,
        // a retry writing to a folder of its own meanwhile.
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
    imports.staged = None;
}

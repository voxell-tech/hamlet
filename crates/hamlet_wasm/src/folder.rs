//! The folder the chief chose for a plugin: listing, reading and
//! writing in it off the frame, each path held inside it, and each
//! write put to the chief unless the folder is a hamlet's set to
//! auto.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_agents_api::hamlet::auto_in;
use hamlet_base::{HomeDir, Tokio};
use hamlet_sdk::capability::{
    Answer, Answered, Capability, Choices, Granted, Petition,
    Verdict, Wanted,
};
use hamlet_sdk::command::Notice;
use hamlet_sdk::plugin::PluginInfo;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    FolderAnswer, FolderFile, FolderOp, FolderResult, Incoming,
    Intent,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;

/// The prefix of the target of `fs.folder` that reads in a folder.
const READ: &str = "read:";
/// The prefix of the target of `fs.folder` that writes in a folder.
const WRITE: &str = "write:";

/// The most of a write's text a petition quotes, in characters.
const QUOTED: usize = 300;

/// What a call in a folder came to off the frame.
enum Came {
    Done(Result<FolderResult, String>),
    /// The write waits for the chief.
    Asking(FolderFile),
}

/// The plugin, the number of its call, its folder and what came of
/// the call.
type Finished = (Entity, u32, PathBuf, Came);

/// The calls in a folder under way.
#[derive(Resource)]
pub struct Folders {
    tx: UnboundedSender<Finished>,
    rx: UnboundedReceiver<Finished>,
    /// The calls each plugin has waiting, a write put to the chief
    /// among them.
    waiting: HashMap<Entity, usize>,
    /// The writes each plugin put to the chief lately.
    asked: Rate,
    /// When each plugin whose last write the chief turned down may
    /// ask again, in seconds by the app's clock.
    resting: HashMap<Entity, f32>,
}

impl Default for Folders {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            tx,
            rx,
            waiting: HashMap::new(),
            asked: Rate::default(),
            resting: HashMap::new(),
        }
    }
}

impl Folders {
    pub fn any(folders: Res<Folders>) -> bool {
        !folders.waiting.is_empty()
    }

    fn done(&mut self, plugin: Entity) {
        if let Some(count) = self.waiting.get_mut(&plugin) {
            *count -= 1;
            if *count == 0 {
                self.waiting.remove(&plugin);
            }
        }
    }
}

/// On the petition that puts a plugin's write to the chief: the
/// plugin, the number of its call, its folder and the file.
#[derive(Component)]
pub struct AskedWrite {
    plugin: Entity,
    id: u32,
    folder: PathBuf,
    file: FolderFile,
}

/// The target of `fs.folder` that reads in `folder`.
pub fn reading(folder: &Path) -> String {
    format!("{READ}{}", folder.display())
}

/// The target of `fs.folder` that writes in `folder`.
pub fn writing(folder: &Path) -> String {
    format!("{WRITE}{}", folder.display())
}

/// The folder the chief chose for a plugin, if it chose one.
pub fn current(choices: &Choices) -> Option<PathBuf> {
    choices.answered_targets().iter().find_map(
        |(capability, target, _)| {
            (*capability == Capability::FsFolder)
                .then(|| target.strip_prefix(READ))
                .flatten()
                .map(PathBuf::from)
        },
    )
}

/// `chosen` as the folder a plugin may work in, made canonical, or
/// why it may not: it is not a folder, or it holds Hamlet's home or
/// is inside it.
pub fn chosen(chosen: &Path, home: &Path) -> Result<PathBuf, String> {
    let real = chosen.canonicalize().map_err(|_| {
        "the folder chosen cannot be read".to_string()
    })?;
    if !real.is_dir() {
        return Err("what was chosen is not a folder".into());
    }
    match beside_home(&real, home) {
        Some(why) => Err(why),
        None => Ok(real),
    }
}

/// Why a plugin may not work in `folder`, if it may not.
fn beside_home(folder: &Path, home: &Path) -> Option<String> {
    let home = home.canonicalize().unwrap_or_else(|_| home.into());
    (folder.starts_with(&home) || home.starts_with(folder)).then(
        || {
            format!(
                "a plugin cannot work in Hamlet's home ({}), or in \
                 a folder that holds it",
                home.display()
            )
        },
    )
}

/// Starts the calls in a folder of the plugins that were stepped.
/// One that is not made is answered at once with why.
pub fn start(
    mut done: MessageReader<Done>,
    plugins: Query<(&Granted, &Choices)>,
    home: Option<Res<HomeDir>>,
    limits: Res<Limits>,
    tokio: Option<Res<Tokio>>,
    mut folders: ResMut<Folders>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        for request in
            intents.iter().filter_map(|intent| match intent {
                Intent::Folder(request) => Some(request),
                _ => None,
            })
        {
            let Ok((granted, choices)) = plugins.get(*plugin) else {
                continue;
            };
            let begun = begin(
                &request.op,
                granted,
                choices,
                home.as_deref(),
                &limits,
                &folders,
                *plugin,
            );
            let (folder, home) = match begun {
                Ok(begun) => begun,
                Err(Refused { why, ask }) => {
                    if let Some(target) = ask {
                        commands.trigger(Wanted {
                            plugin: *plugin,
                            capability: Capability::FsFolder,
                            target: Some(target),
                            reason: String::new(),
                        });
                    }
                    answer(
                        &mut inboxes,
                        *plugin,
                        request.id,
                        Err(why),
                    );
                    continue;
                }
            };
            *folders.waiting.entry(*plugin).or_default() += 1;
            let caps = Caps::of(&limits);
            let (op, tx, id, entity) = (
                request.op.clone(),
                folders.tx.clone(),
                request.id,
                *plugin,
            );
            let work = move || {
                let came = carry_out(op, &folder, &home, caps);
                let _ = tx.send((entity, id, folder, came));
            };
            match tokio.as_deref() {
                Some(tokio) => {
                    tokio.0.spawn_blocking(work);
                }
                None => work(),
            }
        }
    }
}

/// Why a call in a folder is not made, and the target of `fs.folder`
/// the chief is to be asked about, where it is yet to be decided.
struct Refused {
    why: String,
    ask: Option<String>,
}

impl Refused {
    fn because(why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            ask: None,
        }
    }
}

/// The folder and the home a call in a folder is made in, or why it
/// is not made.
fn begin(
    op: &FolderOp,
    granted: &Granted,
    choices: &Choices,
    home: Option<&HomeDir>,
    limits: &Limits,
    folders: &Folders,
    plugin: Entity,
) -> Result<(PathBuf, PathBuf), Refused> {
    let home =
        home.ok_or_else(|| Refused::because("there is no home"))?;
    let folder = current(choices).ok_or_else(|| {
        Refused::because(
            "no folder was chosen; ask the chief for one with \
             pick-folder",
        )
    })?;
    let target = match op {
        FolderOp::Write(_) => writing(&folder),
        FolderOp::List(_) | FolderOp::Read(_) => reading(&folder),
    };
    match granted.verdict_for(Capability::FsFolder, &target) {
        Some(Verdict::Allowed) => {}
        Some(Verdict::Ask) => {
            return Err(Refused {
                why: format!(
                    "fs.folder for `{target}` is yet to be decided"
                ),
                ask: Some(target),
            });
        }
        _ => return Err(Refused::because("fs.folder not granted")),
    }
    let waiting = folders.waiting.get(&plugin).copied().unwrap_or(0);
    if waiting >= limits.folder_calls_in_flight {
        return Err(Refused::because(format!(
            "{waiting} calls in the folder are waiting, the most is \
             {}",
            limits.folder_calls_in_flight
        )));
    }
    Ok((folder, home.0.root.clone()))
}

/// The caps of a folder the chief chose.
#[derive(Clone, Copy)]
pub(crate) struct Caps {
    file_bytes: usize,
    entries: usize,
    path_bytes: usize,
}

impl Caps {
    pub(crate) fn of(limits: &Limits) -> Self {
        Self {
            file_bytes: limits.folder_file_bytes,
            entries: limits.folder_list_entries,
            path_bytes: limits.folder_path_bytes,
        }
    }
}

/// Does `op` in `folder`, off the frame. A write in a folder that is
/// not a hamlet's set to auto, or to a path with a name that starts
/// with `.`, is left for the chief.
fn carry_out(
    op: FolderOp,
    folder: &Path,
    home: &Path,
    caps: Caps,
) -> Came {
    if let Some(why) = beside_home(folder, home) {
        return Came::Done(Err(why));
    }
    Came::Done(match op {
        FolderOp::List(path) => list(folder, &path, caps),
        FolderOp::Read(path) => read(folder, &path, caps),
        FolderOp::Write(file) => {
            match writable(folder, &file, caps) {
                Err(why) => Err(why),
                Ok(at) if dotted(&at) || !auto_in(home, folder) => {
                    return Came::Asking(file);
                }
                Ok(_) => write(folder, &file, caps),
            }
        }
    })
}

/// Whether a name of `rel` starts with `.`: a place tools keep their
/// settings and what they run, as `.vscode` and `.cargo`.
fn dotted(rel: &Path) -> bool {
    rel.components().any(|name| {
        name.as_os_str().to_string_lossy().starts_with('.')
    })
}

/// Where `file` is written in `root`, under it, or why it cannot be,
/// so that the chief is never asked about a write that would fail.
/// No write reaches where git keeps what it runs.
fn writable(
    root: &Path,
    file: &FolderFile,
    caps: Caps,
) -> Result<PathBuf, String> {
    let path = &file.path;
    if file.data.len() > caps.file_bytes {
        return Err(format!(
            "the file has {} bytes, the most is {}",
            file.data.len(),
            caps.file_bytes
        ));
    }
    let (at, there) = inside(root, path, caps)?;
    if there && !at.is_file() {
        return Err(format!("`{path}` is not a file"));
    }
    if !at.parent().is_some_and(Path::is_dir) {
        return Err(format!("the folder of `{path}` is not there"));
    }
    let rel = at.strip_prefix(root).unwrap_or(&at).to_path_buf();
    if hamlet_base::locked(&rel) {
        return Err(format!(
            "`{path}` is where git keeps what it runs, which no \
             plugin may write"
        ));
    }
    Ok(rel)
}

/// `path` in `root`, which is canonical, and whether there is
/// anything there; or why it is not a path inside `root`. It is
/// relative, of names that are neither `.` nor `..`, each folder on
/// the way is a folder and no link, and what it names is a file or a
/// folder, if anything.
fn inside(
    root: &Path,
    path: &str,
    caps: Caps,
) -> Result<(PathBuf, bool), String> {
    let refused = || format!("`{path}` is not a path in the folder");
    if path.len() > caps.path_bytes {
        return Err(format!(
            "the path has {} bytes, the most is {}",
            path.len(),
            caps.path_bytes
        ));
    }
    if path.contains(['\0', '\\']) || path.starts_with('/') {
        return Err(refused());
    }
    let mut at = root.to_path_buf();
    let mut there = true;
    let names = path.split('/').filter(|name| !name.is_empty());
    for name in names {
        if matches!(name, "." | "..") || !there {
            return Err(refused());
        }
        at.push(name);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!("`{path}` goes through a link"));
            }
            Ok(meta) if !meta.is_file() && !meta.is_dir() => {
                return Err(format!(
                    "`{path}` is not a file or a folder"
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {
                there = false
            }
            Err(_) => return Err(refused()),
        }
    }
    // What is there must still be inside once every link is read.
    let real = match there {
        true => at.canonicalize(),
        false => {
            at.parent().unwrap_or(root).canonicalize().map(|parent| {
                parent.join(at.file_name().unwrap_or_default())
            })
        }
    };
    match real {
        Ok(real) if real.starts_with(root) => Ok((real, there)),
        _ => Err(refused()),
    }
}

fn list(
    root: &Path,
    path: &str,
    caps: Caps,
) -> Result<FolderResult, String> {
    let (at, there) = inside(root, path, caps)?;
    if !there || !at.is_dir() {
        return Err(format!("`{path}` is not a folder"));
    }
    let unread = || format!("`{path}` cannot be read");
    let mut names = std::fs::read_dir(&at)
        .map_err(|_| unread())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            let name = entry.file_name().into_string().ok()?;
            match (kind.is_dir(), kind.is_file()) {
                (true, _) => Some(format!("{name}/")),
                (_, true) => Some(name),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names.truncate(caps.entries);
    Ok(FolderResult::Listed(names))
}

fn read(
    root: &Path,
    path: &str,
    caps: Caps,
) -> Result<FolderResult, String> {
    let (at, there) = inside(root, path, caps)?;
    if !there || !at.is_file() {
        return Err(format!("`{path}` is not a file"));
    }
    let too_big = |bytes: u64| {
        format!(
            "the file has {bytes} bytes, the most is {}",
            caps.file_bytes
        )
    };
    let unread = || format!("`{path}` cannot be read");
    let meta = std::fs::metadata(&at).map_err(|_| unread())?;
    if meta.len() > caps.file_bytes as u64 {
        return Err(too_big(meta.len()));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&at)
        .and_then(|file| {
            file.take(caps.file_bytes as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| unread())?;
    if bytes.len() > caps.file_bytes {
        return Err(too_big(bytes.len() as u64));
    }
    Ok(FolderResult::Read(bytes))
}

/// Writes `file` beside its place and renames it over it, so that it
/// is whole or as it was.
fn write(
    root: &Path,
    file: &FolderFile,
    caps: Caps,
) -> Result<FolderResult, String> {
    let path = &file.path;
    writable(root, file, caps)?;
    let (at, _) = inside(root, path, caps)?;
    let (Some(parent), Some(name)) = (at.parent(), at.file_name())
    else {
        return Err(format!("`{path}` is not a path in the folder"));
    };
    let unwritten = || format!("`{path}` cannot be written");
    let part =
        parent.join(format!(".{}.part", name.to_string_lossy()));
    // A part left by a write that stopped, or a link put in its
    // place, is removed and not followed.
    let _ = std::fs::remove_file(&part);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .and_then(|mut out| {
            out.write_all(&file.data)?;
            out.sync_all()
        })
        .and_then(|()| std::fs::rename(&part, &at))
        .map_err(|_| {
            let _ = std::fs::remove_file(&part);
            unwritten()
        })?;
    Ok(FolderResult::Written)
}

/// Gives each plugin the answers to its calls in a folder that came
/// in, and puts a write to the chief where it waits for the chief. A
/// plugin that asks too often, or soon after the chief turned it
/// down, is answered at once.
pub fn finish(
    mut folders: ResMut<Folders>,
    plugins: Query<&PluginInfo>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    while let Ok((plugin, id, folder, came)) = folders.rx.try_recv() {
        let outcome = match came {
            Came::Asking(_)
                if folders
                    .resting
                    .get(&plugin)
                    .is_some_and(|until| now < *until) =>
            {
                Err("the chief turned down the last write; ask \
                     again later"
                    .to_string())
            }
            Came::Asking(_)
                if !folders.asked.admit(
                    plugin,
                    now,
                    limits.folder_asks_per_minute,
                ) =>
            {
                Err(format!(
                    "at most {} writes are put to the chief in a \
                     minute",
                    limits.folder_asks_per_minute
                ))
            }
            Came::Asking(file) => {
                if let Ok(info) = plugins.get(plugin) {
                    commands.spawn((
                        Petition {
                            by: info.name.clone(),
                            text: asking(&info.name, &folder, &file),
                            answers: vec![
                                Answer::Allow,
                                Answer::Deny,
                            ],
                        },
                        AskedWrite {
                            plugin,
                            id,
                            folder,
                            file,
                        },
                    ));
                } else {
                    folders.done(plugin);
                }
                continue;
            }
            Came::Done(outcome) => outcome,
        };
        folders.done(plugin);
        if let (Ok(FolderResult::Written), Ok(info)) =
            (&outcome, plugins.get(plugin))
        {
            tell_written(&mut commands, info, &folder, id);
        }
        answer(&mut inboxes, plugin, id, outcome);
    }
}

/// The question a write puts to the chief, with the start of the text
/// as a block of plain lines that cannot close itself.
fn asking(name: &str, folder: &Path, file: &FolderFile) -> String {
    let words = match std::str::from_utf8(&file.data) {
        Ok(text) => {
            let quoted = text
                .chars()
                .take(QUOTED)
                .collect::<String>()
                .lines()
                .map(|line| {
                    hamlet_base::plain(line).replace('`', "'")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let more = match text.chars().count() > QUOTED {
                true => "\nand more",
                false => "",
            };
            format!(
                "The start of the text, as the plugin gave \
                 it:\n```\n{quoted}\n```{more}"
            )
        }
        Err(_) => "It is not text.".into(),
    };
    format!(
        "{name} asks to write `{}` in {} ({} bytes). Allow \
         it?\n{words}",
        hamlet_base::plain(&file.path).replace('`', "'"),
        folder.display(),
        file.data.len()
    )
}

/// Carries out the chief's answer to a write a plugin asked for, if
/// the plugin still may write in that folder. A Don't allow has the
/// plugin rest before it asks again.
pub fn answered(
    answered: On<Answered>,
    asked: Query<&AskedWrite>,
    plugins: Query<(&Granted, &Choices)>,
    home: Option<Res<HomeDir>>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    tokio: Option<Res<Tokio>>,
    mut folders: ResMut<Folders>,
    mut inboxes: Query<&mut Inbox>,
) {
    let Ok(asked) = asked.get(answered.petition) else {
        return;
    };
    let (plugin, id) = (asked.plugin, asked.id);
    if answered.answer != Answer::Allow {
        let now = time.map_or(0.0, |time| time.elapsed_secs());
        let rest = limits.folder_rest_ms as f32 / 1000.0;
        folders.resting.insert(plugin, now + rest);
    }
    let still =
        plugins.get(plugin).is_ok_and(|(granted, choices)| {
            current(choices).as_ref() == Some(&asked.folder)
                && granted.verdict_for(
                    Capability::FsFolder,
                    &writing(&asked.folder),
                ) == Some(Verdict::Allowed)
        });
    let Some(home) = home
        .filter(|_| answered.answer == Answer::Allow)
        .filter(|_| still)
    else {
        folders.done(plugin);
        let why = match answered.answer == Answer::Allow {
            true => "fs.folder for that folder is no longer granted",
            false => "the chief did not allow the write",
        };
        answer(&mut inboxes, plugin, id, Err(why.to_string()));
        return;
    };
    let (folder, file, home) = (
        asked.folder.clone(),
        asked.file.clone(),
        home.0.root.clone(),
    );
    let (caps, tx) = (Caps::of(&limits), folders.tx.clone());
    let work = move || {
        let written = match beside_home(&folder, &home) {
            Some(why) => Err(why),
            None => write(&folder, &file, caps),
        };
        let _ = tx.send((plugin, id, folder, Came::Done(written)));
    };
    match tokio.as_deref() {
        Some(tokio) => {
            tokio.0.spawn_blocking(work);
        }
        None => work(),
    }
}

fn tell_written(
    commands: &mut Commands,
    info: &PluginInfo,
    folder: &Path,
    id: u32,
) {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    info!(
        "plugin {}: wrote in {} (call {id})",
        info.id,
        folder.display()
    );
    commands.trigger(Notice(format!(
        "{} ({}) wrote in the folder {name}",
        info.name, info.id
    )));
}

fn answer(
    inboxes: &mut Query<&mut Inbox>,
    plugin: Entity,
    id: u32,
    outcome: Result<FolderResult, String>,
) {
    if let Ok(mut inbox) = inboxes.get_mut(plugin) {
        inbox
            .push(Incoming::FolderDone(FolderAnswer { id, outcome }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: Caps = Caps {
        file_bytes: 8,
        entries: 8,
        path_bytes: 64,
    };

    #[test]
    fn a_path_stays_inside_the_folder() {
        let scratch = std::env::temp_dir()
            .join(format!("hamlet_folder_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let root = scratch.join("work");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(scratch.join("secret"), "x").unwrap();
        let root = root.canonicalize().unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            scratch.join("secret"),
            root.join("out"),
        )
        .unwrap();
        let file = |path: &str, data: &str| FolderFile {
            path: path.into(),
            data: data.into(),
        };

        assert!(
            write(&root, &file("notes/a.md", "hi"), CAPS).is_ok()
        );
        assert!(matches!(
            read(&root, "notes/a.md", CAPS),
            Ok(FolderResult::Read(bytes)) if bytes == b"hi"
        ));
        for path in
            ["../secret", "/etc/hosts", "notes/../../secret", "a\\b"]
        {
            assert!(read(&root, path, CAPS).is_err(), "{path}");
        }
        #[cfg(unix)]
        {
            assert!(read(&root, "out", CAPS).is_err());
            assert!(write(&root, &file("out", "y"), CAPS).is_err());
            assert_eq!(
                std::fs::read_to_string(scratch.join("secret"))
                    .unwrap(),
                "x"
            );
        }
        assert!(write(&root, &file("gone/a.md", "y"), CAPS).is_err());
        assert!(
            write(&root, &file("big", "123456789"), CAPS).is_err()
        );
        assert!(matches!(
            list(&root, "", CAPS),
            Ok(FolderResult::Listed(names)) if names == ["notes/"]
        ));
        std::fs::create_dir_all(root.join(".GIT/hooks")).unwrap();
        for path in [".GIT/config", ".GIT/hooks/pre-commit"] {
            assert!(write(&root, &file(path, "y"), CAPS).is_err());
        }
        assert!(chosen(&scratch, &root).is_err());
        assert!(chosen(&root, &scratch).is_err());
        let _ = std::fs::remove_dir_all(&scratch);
    }
}

//! A file or a folder the chief picks for a plugin: the plugin asks,
//! the chief chooses in the system's dialog, and a file is copied
//! into the plugin's folder of files, so that the plugin never learns
//! a path outside it, while a folder becomes the one the plugin works
//! in.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use hamlet_base::{HomeDir, Tokio};
use hamlet_desktop::file::{FileDialog, Pick};
use hamlet_sdk::capability::{
    Capability, Choice, Decide, Forget, Granted, Verdict, Wanted,
};
use hamlet_sdk::plugin::PluginInfo;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::files::{Caps, FILES, Files};
use crate::folder::{self, reading, writing};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent, PickAnswer, PickRequest,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;
use crate::start::Source;

/// The folder of a plugin's files that a file the chief picked is
/// copied into.
const PICKED: &str = "picked";

/// A pick that finished: the plugin, the number of its ask, what was
/// picked and the path, or why there is none. The path of a file is
/// in the plugin's folder of files; that of a folder is the folder's.
type Copied = (Entity, u32, Pick, Result<String, String>);

/// The dialog a plugin asked for, waiting to be opened or open, and
/// the copy of the file chosen in it. One is under way at a time.
#[derive(Resource)]
pub struct Picking {
    /// The plugin, the number of its ask, the dialog's title and
    /// what it picks.
    asked: Option<(Entity, u32, String, Pick)>,
    /// The plugin, the number of the ask a dialog is open for and
    /// what it picks.
    open: Option<(Entity, u32, Pick)>,
    /// The plugin and the number of the ask whose pick is taken in.
    copying: Option<(Entity, u32)>,
    tx: UnboundedSender<Option<PathBuf>>,
    rx: UnboundedReceiver<Option<PathBuf>>,
    copied_tx: UnboundedSender<Copied>,
    copied_rx: UnboundedReceiver<Copied>,
    rate: Rate,
    /// When each plugin whose dialog was closed with nothing may ask
    /// again, in seconds by the app's clock.
    resting: HashMap<Entity, f32>,
}

impl Default for Picking {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        let (copied_tx, copied_rx) = unbounded_channel();
        Self {
            asked: None,
            open: None,
            copying: None,
            tx,
            rx,
            copied_tx,
            copied_rx,
            rate: Rate::default(),
            resting: HashMap::new(),
        }
    }
}

impl Picking {
    /// What the open dialog picks, if a dialog is open for a plugin.
    pub fn open_for(&self) -> Option<Pick> {
        self.open.map(|(.., pick)| pick)
    }

    /// Whether a dialog is open for a plugin.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Closes the open dialog with `path` chosen, or with nothing, as
    /// the system's dialog does.
    pub fn choose(&self, path: Option<PathBuf>) {
        let _ = self.tx.send(path);
    }

    /// Whether a dialog is open or what was chosen in one is taken
    /// in.
    pub fn under_way(&self) -> bool {
        self.open.is_some() || self.copying.is_some()
    }

    fn busy(&self) -> bool {
        self.asked.is_some() || self.under_way()
    }
}

/// Takes the asks for a file or a folder of the plugins that were
/// stepped.
pub fn ask(
    mut done: MessageReader<Done>,
    plugins: Query<(&PluginInfo, &Granted)>,
    dialog: Option<Res<FileDialog>>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut picking: ResMut<Picking>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        for (request, pick) in intents.iter().filter_map(asked) {
            let Ok((info, granted)) = plugins.get(*plugin) else {
                continue;
            };
            let title =
                title(&request.title, limits.pick_title_bytes);
            let resting = picking
                .resting
                .get(plugin)
                .is_some_and(|until| now < *until);
            let capability = match pick {
                Pick::File => Capability::FsPick,
                Pick::Folder => Capability::FsFolder,
            };
            let verdict = match (granted.verdict(capability), pick) {
                // The dialog is the chief's answer for a folder.
                (Some(Verdict::Ask), Pick::Folder) => {
                    Some(Verdict::Allowed)
                }
                (verdict, _) => verdict,
            };
            let refused = match verdict {
                Some(Verdict::Allowed) if dialog.is_none() => {
                    Some("there is no file dialog here".into())
                }
                Some(Verdict::Allowed) if picking.busy() => {
                    Some("a file dialog is already open".into())
                }
                Some(Verdict::Allowed) if resting => Some(
                    "the chief closed the last dialog with nothing; \
                     ask again later"
                        .into(),
                ),
                Some(Verdict::Allowed)
                    if !picking.rate.admit(
                        *plugin,
                        now,
                        limits.picks_per_minute,
                    ) =>
                {
                    Some(format!(
                        "at most {} files are asked for in a minute",
                        limits.picks_per_minute
                    ))
                }
                Some(Verdict::Allowed) => None,
                Some(Verdict::Ask) => {
                    commands.trigger(Wanted {
                        plugin: *plugin,
                        capability,
                        target: None,
                        reason: title.clone(),
                    });
                    Some(format!(
                        "{} is yet to be decided",
                        capability.name()
                    ))
                }
                _ => {
                    Some(format!("{} not granted", capability.name()))
                }
            };
            match refused {
                Some(why) => answer(
                    &mut inboxes,
                    *plugin,
                    request.id,
                    pick,
                    Err(why),
                ),
                None => {
                    picking.asked = Some((
                        *plugin,
                        request.id,
                        format!("{}: {title}", info.name),
                        pick,
                    ));
                }
            }
        }
    }
}

/// The ask of `intent` and what it picks, if it is one.
fn asked(intent: &Intent) -> Option<(&PickRequest, Pick)> {
    match intent {
        Intent::PickFile(request) => Some((request, Pick::File)),
        Intent::PickFolder(request) => Some((request, Pick::Folder)),
        _ => None,
    }
}

/// `text` as one plain line of at most `most` bytes.
fn title(text: &str, most: usize) -> String {
    let mut title = hamlet_base::plain(text);
    title.truncate(title.floor_char_boundary(most));
    title
}

/// Whether a plugin's ask waits for its dialog to be opened.
pub fn waits(picking: Res<Picking>) -> bool {
    picking.asked.is_some()
}

/// Opens the dialog a plugin asked for. The marker keeps this on the
/// main thread, where macOS wants a dialog begun.
pub fn open(
    _main: NonSendMarker,
    dialog: Option<Res<FileDialog>>,
    tokio: Option<Res<Tokio>>,
    mut picking: ResMut<Picking>,
) {
    let (Some(dialog), Some((plugin, id, title, pick))) =
        (dialog, picking.asked.take())
    else {
        return;
    };
    picking.open = Some((plugin, id, pick));
    let tx = picking.tx.clone();
    (dialog.0)(
        title,
        pick,
        tx,
        tokio.as_deref().map(|tokio| &tokio.0),
    );
}

pub fn waiting(picking: Res<Picking>) -> bool {
    picking.under_way()
}

/// Takes in what the chief chose for the plugin that asked for it,
/// on the runtime where there is one: a file is copied into its
/// folder of files, and a folder is checked. The plugin is answered
/// once it is done, and a folder becomes the one it works in. A
/// plugin whose dialog was closed with nothing rests before it may
/// ask again.
pub fn take(
    mut picking: ResMut<Picking>,
    sources: Query<&Source>,
    plugins: Query<&PluginInfo>,
    home: Option<Res<HomeDir>>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    tokio: Option<Res<Tokio>>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    while let Ok(path) = picking.rx.try_recv() {
        let Some((plugin, id, pick)) = picking.open.take() else {
            continue;
        };
        let Some(path) = path else {
            let rest = limits.pick_rest_ms as f32 / 1000.0;
            picking.resting.retain(|_, until| now < *until);
            picking.resting.insert(plugin, now + rest);
            let why = "nothing was chosen".to_string();
            answer(&mut inboxes, plugin, id, pick, Err(why));
            continue;
        };
        let into = match pick {
            Pick::File => sources.get(plugin).ok().map(|source| {
                let folder = source
                    .0
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(FILES);
                Destination::Files(Files::new(
                    folder,
                    Caps::of(&limits),
                ))
            }),
            Pick::Folder => home
                .as_deref()
                .map(|home| Destination::Home(home.0.root.clone())),
        };
        let Some(into) = into else {
            let why = "the plugin is not there".to_string();
            answer(&mut inboxes, plugin, id, pick, Err(why));
            continue;
        };
        let (tx, caps) =
            (picking.copied_tx.clone(), Caps::of(&limits));
        let take_in = move || {
            let taken = match into {
                Destination::Files(files) => {
                    copy(&path, &files, caps)
                }
                Destination::Home(home) => {
                    folder::chosen(&path, &home)
                        .map(|real| real.display().to_string())
                }
            };
            let _ = tx.send((plugin, id, pick, taken));
        };
        picking.copying = Some((plugin, id));
        match tokio.as_deref() {
            Some(tokio) => {
                tokio.0.spawn_blocking(take_in);
            }
            None => take_in(),
        }
    }
    while let Ok((plugin, id, pick, outcome)) =
        picking.copied_rx.try_recv()
    {
        picking.copying = None;
        let outcome = match (pick, outcome, plugins.get(plugin)) {
            (Pick::Folder, Ok(chosen), Ok(info)) => {
                let chosen = PathBuf::from(chosen);
                work_in(&mut commands, &info.id, &chosen);
                info!(
                    "plugin {}: works in {}",
                    info.id,
                    chosen.display()
                );
                Ok(chosen
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default())
            }
            (_, outcome, _) => outcome,
        };
        answer(&mut inboxes, plugin, id, pick, outcome);
    }
}

/// Where what the chief chose is taken in: a file into the plugin's
/// folder of files, and a folder is checked against the home.
enum Destination {
    Files(Files),
    Home(PathBuf),
}

/// Makes `folder` the one folder the plugin of this id works in, with
/// reading and writing in it allowed, as the chief chose it.
fn work_in(commands: &mut Commands, plugin: &str, folder: &Path) {
    commands.trigger(Forget {
        plugin: plugin.into(),
        capability: Capability::FsFolder,
    });
    for target in [reading(folder), writing(folder)] {
        commands.trigger(Decide {
            plugin: plugin.into(),
            capability: Capability::FsFolder,
            target: Some(target),
            choice: Choice::Allow,
            this_run: false,
        });
    }
}

fn answer(
    inboxes: &mut Query<&mut Inbox>,
    plugin: Entity,
    id: u32,
    pick: Pick,
    outcome: Result<String, String>,
) {
    if let Ok(mut inbox) = inboxes.get_mut(plugin) {
        let answer = PickAnswer { id, outcome };
        inbox.push(match pick {
            Pick::File => Incoming::Picked(answer),
            Pick::Folder => Incoming::FolderPicked(answer),
        });
    }
}

/// Copies `file` into `files` under `picked/`, by its name made
/// plain and with a number after it where a file of that name is
/// there. The path in the folder, or why it is not there.
fn copy(
    file: &Path,
    files: &Files,
    caps: Caps,
) -> Result<String, String> {
    let unread = || "the file chosen cannot be read".to_string();
    let meta = std::fs::metadata(file).map_err(|_| unread())?;
    if !meta.is_file() {
        return Err("what was chosen is not a file".into());
    }
    let too_big = |bytes: u64| {
        format!(
            "the file has {bytes} bytes, the most is {}",
            caps.file_bytes
        )
    };
    if meta.len() > caps.file_bytes as u64 {
        return Err(too_big(meta.len()));
    }
    // Read once, as the chief chose it, and no further than the cap
    // in case it grew since.
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .and_then(|opened| {
            opened
                .take(caps.file_bytes as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| unread())?;
    if bytes.len() > caps.file_bytes {
        return Err(too_big(bytes.len() as u64));
    }
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let path = free(files, &name, caps.path_bytes);
    files.write(&path, &bytes)?;
    Ok(path)
}

/// The first path under `picked/` for `name` that no file holds:
/// the name, then the name with `-2`, `-3` and on before its kind.
fn free(files: &Files, name: &str, path_bytes: usize) -> String {
    let held = files.list();
    let room = path_bytes.saturating_sub(PICKED.len() + 1);
    let mut number = 1;
    loop {
        let suffix = match number {
            1 => String::new(),
            n => format!("-{n}"),
        };
        let fitted = fit(name, room.saturating_sub(suffix.len()));
        let named = match fitted.rfind('.') {
            Some(dot) if dot > 0 => {
                format!(
                    "{}{suffix}{}",
                    &fitted[..dot],
                    &fitted[dot..]
                )
            }
            _ => format!("{fitted}{suffix}"),
        };
        let path = format!("{PICKED}/{named}");
        if !held.contains(&path) || number > held.len() {
            return path;
        }
        number += 1;
    }
}

/// `name` in at most `room` bytes of what a path of the folder of
/// files holds, each other character made `_`, with no dot first.
fn fit(name: &str, room: usize) -> String {
    let kept = name
        .chars()
        .map(|c| {
            match c.is_ascii_alphanumeric() || ".-_ ".contains(c) {
                true => c,
                false => '_',
            }
        })
        .collect::<String>();
    // The end, so that the kind of file stays in its name.
    let tail = kept[kept.len().saturating_sub(room)..]
        .trim_start_matches(['.', ' '])
        .trim_end();
    match tail.is_empty() {
        true => "file".into(),
        false => tail.into(),
    }
}

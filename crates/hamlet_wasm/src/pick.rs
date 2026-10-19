//! A file the chief picks for a plugin: the plugin asks, the chief
//! chooses in the system's dialog, and the file is copied into the
//! plugin's folder of files, so that the plugin never learns a path
//! outside it.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use hamlet_base::Tokio;
use hamlet_desktop::file::FileDialog;
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::plugin::PluginInfo;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::files::{Caps, FILES, Files};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent, PickAnswer,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;
use crate::start::Source;

/// The folder of a plugin's files that a file the chief picked is
/// copied into.
const PICKED: &str = "picked";

/// A copy that finished: the plugin, the number of its ask and the
/// path in its folder, or why there is none.
type Copied = (Entity, u32, Result<String, String>);

/// The dialog a plugin asked for, waiting to be opened or open, and
/// the copy of the file chosen in it. One is under way at a time.
#[derive(Resource)]
pub struct Picking {
    /// The plugin, the number of its ask and the dialog's title.
    asked: Option<(Entity, u32, String)>,
    /// The plugin and the number of the ask a dialog is open for.
    open: Option<(Entity, u32)>,
    /// The plugin and the number of the ask whose file is copied.
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
    /// Whether a dialog is open for a plugin.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Closes the open dialog with `file` chosen, or with nothing, as
    /// the system's dialog does.
    pub fn choose(&self, file: Option<PathBuf>) {
        let _ = self.tx.send(file);
    }

    /// Whether a dialog is open or a file chosen in one is copied.
    pub fn under_way(&self) -> bool {
        self.open.is_some() || self.copying.is_some()
    }

    fn busy(&self) -> bool {
        self.asked.is_some() || self.under_way()
    }
}

/// Takes the asks for a file of the plugins that were stepped.
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
        for intent in intents {
            let Intent::PickFile(request) = intent else {
                continue;
            };
            let Ok((info, granted)) = plugins.get(*plugin) else {
                continue;
            };
            let title =
                title(&request.title, limits.pick_title_bytes);
            let resting = picking
                .resting
                .get(plugin)
                .is_some_and(|until| now < *until);
            let refused = match granted.verdict(Capability::FsPick) {
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
                        capability: Capability::FsPick,
                        target: None,
                        reason: title.clone(),
                    });
                    Some("fs.pick is yet to be decided".to_string())
                }
                _ => Some("fs.pick not granted".into()),
            };
            match refused {
                Some(why) => answer(
                    &mut inboxes,
                    *plugin,
                    request.id,
                    Err(why),
                ),
                None => {
                    picking.asked = Some((
                        *plugin,
                        request.id,
                        format!("{}: {title}", info.name),
                    ));
                }
            }
        }
    }
}

/// `text` as one plain line of at most `most` bytes.
fn title(text: &str, most: usize) -> String {
    let mut title = hamlet_base::plain(text);
    title.truncate(title.floor_char_boundary(most));
    title
}

/// Whether a plugin's ask waits for its dialog to be opened.
pub fn asked(picking: Res<Picking>) -> bool {
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
    let (Some(dialog), Some((plugin, id, title))) =
        (dialog, picking.asked.take())
    else {
        return;
    };
    picking.open = Some((plugin, id));
    let tx = picking.tx.clone();
    (dialog.0)(title, tx, tokio.as_deref().map(|tokio| &tokio.0));
}

pub fn waiting(picking: Res<Picking>) -> bool {
    picking.under_way()
}

/// Copies the file the chief chose into the folder of the plugin that
/// asked for it, on the runtime where there is one, and answers the
/// plugin once it is copied. A plugin whose dialog was closed with
/// nothing rests before it may ask again.
pub fn take(
    mut picking: ResMut<Picking>,
    sources: Query<&Source>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    tokio: Option<Res<Tokio>>,
    mut inboxes: Query<&mut Inbox>,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    while let Ok(file) = picking.rx.try_recv() {
        let Some((plugin, id)) = picking.open.take() else {
            continue;
        };
        let folder = sources.get(plugin).map(|source| {
            source.0.parent().unwrap_or(Path::new("")).join(FILES)
        });
        let (file, folder) = match (file, folder) {
            (None, _) => {
                let rest = limits.pick_rest_ms as f32 / 1000.0;
                picking.resting.retain(|_, until| now < *until);
                picking.resting.insert(plugin, now + rest);
                let why = "nothing was chosen".to_string();
                answer(&mut inboxes, plugin, id, Err(why));
                continue;
            }
            (Some(_), Err(_)) => {
                let why = "the plugin is not there".to_string();
                answer(&mut inboxes, plugin, id, Err(why));
                continue;
            }
            (Some(file), Ok(folder)) => (file, folder),
        };
        let caps = Caps::of(&limits);
        let files = Files::new(folder, caps);
        let tx = picking.copied_tx.clone();
        let copy_it = move || {
            let _ = tx.send((plugin, id, copy(&file, &files, caps)));
        };
        picking.copying = Some((plugin, id));
        match tokio.as_deref() {
            Some(tokio) => {
                tokio.0.spawn_blocking(copy_it);
            }
            None => copy_it(),
        }
    }
    while let Ok((plugin, id, outcome)) = picking.copied_rx.try_recv()
    {
        picking.copying = None;
        answer(&mut inboxes, plugin, id, outcome);
    }
}

fn answer(
    inboxes: &mut Query<&mut Inbox>,
    plugin: Entity,
    id: u32,
    outcome: Result<String, String>,
) {
    if let Ok(mut inbox) = inboxes.get_mut(plugin) {
        inbox.push(Incoming::Picked(PickAnswer { id, outcome }));
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

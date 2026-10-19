//! A file the chief picks for a plugin: the plugin asks, the chief
//! chooses in the system's dialog, and the file is copied into the
//! plugin's folder of files, so that the plugin never learns a path
//! outside it.

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

use crate::files::{self, FILES, Files};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent, PickAnswer,
};
use crate::limits::Limits;
use crate::run::Done;
use crate::start::Source;

/// The folder of a plugin's files that a file the chief picked is
/// copied into.
const PICKED: &str = "picked";

/// The dialog a plugin asked for, waiting to be opened or open. One
/// is open at a time.
#[derive(Resource)]
pub struct Picking {
    /// The plugin, the number of its ask and the dialog's title.
    asked: Option<(Entity, u32, String)>,
    /// The plugin and the number of the ask a dialog is open for.
    open: Option<(Entity, u32)>,
    tx: UnboundedSender<Option<PathBuf>>,
    rx: UnboundedReceiver<Option<PathBuf>>,
}

impl Default for Picking {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            asked: None,
            open: None,
            tx,
            rx,
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
}

/// Takes the asks for a file of the plugins that were stepped.
pub fn ask(
    mut done: MessageReader<Done>,
    plugins: Query<(&PluginInfo, &Granted)>,
    dialog: Option<Res<FileDialog>>,
    mut picking: ResMut<Picking>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
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
            let busy =
                picking.asked.is_some() || picking.open.is_some();
            let refused = match granted.verdict(Capability::FsPick) {
                Some(Verdict::Allowed) if dialog.is_none() => {
                    Some("there is no file dialog here".into())
                }
                Some(Verdict::Allowed) if busy => {
                    Some("a file dialog is already open".into())
                }
                Some(Verdict::Allowed) => None,
                Some(Verdict::Ask) => {
                    commands.trigger(Wanted {
                        plugin: *plugin,
                        capability: Capability::FsPick,
                        target: None,
                        reason: request.title.clone(),
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
                        format!("{}: {}", info.name, request.title),
                    ));
                }
            }
        }
    }
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

/// Whether a dialog is open.
pub fn is_open(picking: Res<Picking>) -> bool {
    picking.is_open()
}

/// Copies the file the chief chose into the folder of the plugin that
/// asked for it, and answers the plugin.
pub fn take(
    mut picking: ResMut<Picking>,
    sources: Query<&Source>,
    limits: Res<Limits>,
    mut inboxes: Query<&mut Inbox>,
) {
    while let Ok(file) = picking.rx.try_recv() {
        let Some((plugin, id)) = picking.open.take() else {
            continue;
        };
        let outcome = match (file, sources.get(plugin)) {
            (None, _) => Err("nothing was chosen".into()),
            (Some(_), Err(_)) => {
                Err("the plugin is not there".into())
            }
            (Some(file), Ok(source)) => {
                let folder =
                    source.0.parent().unwrap_or(Path::new(""));
                let files = Files::new(
                    folder.join(FILES),
                    files::Caps::of(&limits),
                );
                copy(&file, &files, &limits)
            }
        };
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
/// plain. The path in the folder, or why it is not there.
fn copy(
    file: &Path,
    files: &Files,
    limits: &Limits,
) -> Result<String, String> {
    let unread = || "the file chosen cannot be read".to_string();
    let meta = std::fs::metadata(file).map_err(|_| unread())?;
    if !meta.is_file() {
        return Err("what was chosen is not a file".into());
    }
    if meta.len() > limits.file_bytes as u64 {
        return Err(format!(
            "the file has {} bytes, the most is {}",
            meta.len(),
            limits.file_bytes
        ));
    }
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let room =
        limits.file_path_bytes.saturating_sub(PICKED.len() + 1);
    let path = format!("{PICKED}/{}", plain(&name, room));
    // Read once, as the chief chose it; the cap was checked first.
    let bytes = std::fs::read(file).map_err(|_| unread())?;
    files.write(&path, &bytes)?;
    Ok(path)
}

/// `name` in at most `room` bytes of what a path of the folder of
/// files holds, each other character made `_`, with no dot first.
fn plain(name: &str, room: usize) -> String {
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

//! A sample plugin that uses what another makes: it holds the notes
//! plugin's note by reference, so it shows each new version as it is
//! saved, asks the notes plugin's `words` service how many words it
//! has, and tells the notes plugin with its event `seen` which
//! version it shows.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    column, heading, label, list, resource,
};
use hamlet_engine_bevy::{
    AssetId, Reply, decode, encode, info, warn,
};

const NOTE: &str = "notes.note";

#[derive(Default)]
struct Board;

impl Plugin for Board {
    fn build(&self, app: &mut App) {
        app.init_resource::<Note>()
            .add_panel("board", "Board", board)
            .add_systems(Declare, find)
            .add_systems(Update, (follow, show).chain());
    }
}

hamlet_engine_bevy::export!(Board);

/// The note held, as the board shows it.
#[derive(Resource, Default)]
struct Note {
    id: Option<AssetId>,
    text: Option<String>,
    version: u64,
    words: Option<u32>,
}

impl Note {
    fn hold(&mut self, id: AssetId, host: &mut impl HostExt) {
        if self.id != Some(id) {
            self.id = Some(id);
            host.hold_asset(id);
        }
    }
}

fn board() -> AnyView {
    column((
        heading("Board"),
        list((
            label(resource::<Note, _>(|note| {
                note.text
                    .clone()
                    .unwrap_or_else(|| "No note yet.".into())
            })),
            label(resource::<Note, _>(|note| match note.text {
                Some(_) => format!("Version {}", note.version),
                None => String::new(),
            })),
            label(resource::<Note, _>(|note| {
                match (&note.text, note.words) {
                    (None, _) => String::new(),
                    (Some(_), Some(1)) => "1 word".into(),
                    (Some(_), Some(words)) => {
                        format!("{words} words")
                    }
                    (Some(_), None) => "Counting...".into(),
                }
            })),
        )),
    ))
    .boxed()
}

/// Hears the notes plugin save, and looks for its note.
fn find(mut commands: Commands) {
    commands.subscribe("hamlet.notes/saved");
    commands.list_assets(NOTE);
}

/// Holds the note the notes plugin saves, and reads each version of
/// it.
fn follow(
    mut saved: MessageReader<EventReceived>,
    mut changed: MessageReader<AssetChanged>,
    mut gone: MessageReader<AssetGone>,
    mut note: ResMut<Note>,
    mut commands: Commands,
) {
    for event in saved.read() {
        if let Ok((id, version)) =
            decode::<(u64, u64)>(&event.payload)
        {
            info(&format!(
                "{} saved note {id} at version {version}",
                event.from
            ));
            note.hold(AssetId(id), &mut commands);
        }
    }
    for change in changed.read() {
        info(&format!(
            "note {} changed to version {}",
            change.asset.0, change.version
        ));
        commands.read_asset(change.asset);
    }
    if gone.read().count() > 0 {
        *note = Note::default();
    }
}

/// Shows the note as it is read, has its words counted, and tells
/// the notes plugin the version shown.
fn show(
    mut replies: MessageReader<Replied>,
    mut note: ResMut<Note>,
    mut commands: Commands,
) {
    for replied in replies.read() {
        match &replied.reply {
            Ok(Reply::Listed(found)) => {
                if let Some(found) = found.first() {
                    note.hold(found.id, &mut commands);
                }
            }
            Ok(Reply::Held(_)) => {
                if let Some(id) = note.id {
                    commands.read_asset(id);
                }
            }
            Ok(Reply::Read(asset)) => {
                let text = String::from_utf8_lossy(&asset.bytes)
                    .into_owned();
                note.version = asset.info.version;
                info(&format!(
                    "shows note {} at version {}",
                    asset.info.id.0, note.version
                ));
                if let Ok(argument) = encode(&text) {
                    commands.call("hamlet.notes/words", &argument);
                }
                if let Ok(payload) = encode(&note.version) {
                    commands.emit("seen", &payload);
                }
                note.text = Some(text);
                note.words = None;
            }
            Ok(Reply::Answered(result)) => {
                note.words = decode::<u32>(result).ok();
                if let Some(words) = note.words {
                    info(&format!("the note has {words} words"));
                }
            }
            Ok(_) => {}
            Err(why) => warn(&why.to_string()),
        }
    }
}

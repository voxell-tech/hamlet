//! A sample plugin that makes things for others: a note, kept as an
//! asset of its type `notes.note` (the bytes are the text), a service
//! `words` that counts the words of a text, and an event `saved` with
//! the note's id and version each time it is saved. It hears the
//! board's `seen`, and shows which version the board saw last.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::serde::{Deserialize, Serialize};
use hamlet_engine_bevy::ui::{
    UiExt, column, field, heading, icon_button, label, resource,
};
use hamlet_engine_bevy::{
    AssetId, Reply, decode, encode, info, warn,
};

const NOTE: &str = "notes.note";

#[derive(Default)]
struct Notes;

impl Plugin for Notes {
    fn build(&self, app: &mut App) {
        app.init_saved_resource::<Note>()
            .add_panel("notes", "Notes", notes)
            .add_service("words", words)
            .add_systems(Declare, find)
            .add_systems(Update, (keep, hear));
    }
}

hamlet_engine_bevy::export!(Notes);

/// The note, as the panel shows it.
#[derive(Resource, Default, Serialize, Deserialize)]
#[serde(crate = "hamlet_engine_bevy::serde")]
struct Note {
    id: Option<AssetId>,
    version: u64,
    /// The text in the field, saved with Save.
    draft: String,
    /// The version the board saw last.
    seen: Option<u64>,
}

impl Note {
    fn saved(
        &mut self,
        id: AssetId,
        version: u64,
        host: &mut impl HostExt,
    ) {
        self.id = Some(id);
        self.version = version;
        info(&format!("saved note {} at version {version}", id.0));
        if let Ok(payload) = encode(&(id.0, version)) {
            host.emit("saved", &payload);
        }
    }

    /// Saves the draft: the note made, or changed.
    fn save(&self, host: &mut impl HostExt) {
        match self.id {
            Some(id) => {
                host.update_asset(id, self.draft.as_bytes());
            }
            None => {
                host.create_asset(
                    NOTE,
                    "Note",
                    self.draft.as_bytes(),
                );
            }
        }
    }
}

fn notes() -> AnyView {
    column((
        heading("Notes"),
        field(resource::<Note, _>(|note| note.draft.clone()))
            .placeholder("The note")
            .on_commit(|world, text| {
                world.resource_mut::<Note>().draft = text;
            }),
        icon_button("check", "Save").on_click(|world| {
            world.resource_scope(|world, note: Mut<Note>| {
                note.save(world);
            });
        }),
        label(resource::<Note, _>(|note| {
            format!("Version {}", note.version)
        })),
        label(resource::<Note, _>(|note| match note.seen {
            Some(version) => {
                format!("The board saw version {version}.")
            }
            None => "The board has not seen it yet.".into(),
        })),
    ))
    .boxed()
}

/// Looks for the note made before, and hears the board.
fn find(mut commands: Commands) {
    commands.list_assets(NOTE);
    commands.subscribe("hamlet.board/seen");
}

/// Keeps the note as the asset store answers.
fn keep(
    mut replies: MessageReader<Replied>,
    mut note: ResMut<Note>,
    mut commands: Commands,
) {
    for replied in replies.read() {
        match &replied.reply {
            Ok(Reply::Listed(found)) => match found.first() {
                Some(found) => {
                    note.id = Some(found.id);
                    note.version = found.version;
                    commands.read_asset(found.id);
                }
                None => {
                    note.draft = "A first note".into();
                    note.save(&mut commands);
                }
            },
            Ok(Reply::Read(asset)) => {
                note.draft = String::from_utf8_lossy(&asset.bytes)
                    .into_owned();
                note.version = asset.info.version;
            }
            Ok(Reply::Created(id)) => {
                note.saved(*id, 1, &mut commands)
            }
            Ok(Reply::Updated(version)) => {
                if let Some(id) = note.id {
                    note.saved(id, *version, &mut commands);
                }
            }
            Ok(_) => {}
            Err(why) => warn(&why.to_string()),
        }
    }
}

/// Shows the version the board saw last.
fn hear(
    mut events: MessageReader<EventReceived>,
    mut note: ResMut<Note>,
) {
    for event in events.read() {
        if let Ok(version) = decode::<u64>(&event.payload) {
            note.seen = Some(version);
        }
    }
}

/// Counts the words of a text.
fn words(call: In<ServiceCall>) -> Result<Vec<u8>, String> {
    let text = decode::<String>(&call.argument)
        .map_err(|why| why.to_string())?;
    let words = text.split_whitespace().count() as u32;
    info(&format!("counted {words} words for {}", call.caller));
    encode(&words).map_err(|why| why.to_string())
}

//! A sample plugin that makes things for others: a note, kept as an
//! asset of its type `notes.note` (the bytes are the text), a service
//! `words` that counts the words of a text, and an event `saved` with
//! the note's id and version each time it is saved. It hears the
//! board's `seen`, and shows which version the board saw last.

use hamlet_engine_sdk::ui::{
    AnyView, UiExt, ViewExt, column, field, heading, icon_button,
    label, resource,
};
use hamlet_engine_sdk::{
    AssetId, Entry, Message, Plugin, Reply, World, assets, decode,
    emit, encode, info, subscribe, warn,
};

const NOTE: &str = "notes.note";

#[derive(Default)]
struct Notes;

/// The note, as the panel shows it.
#[derive(Default)]
struct Note {
    id: Option<AssetId>,
    version: u64,
    /// The text in the field, saved with Save.
    draft: String,
    /// The version the board saw last.
    seen: Option<u64>,
}

impl Note {
    fn saved(&mut self, id: AssetId, version: u64) {
        self.id = Some(id);
        self.version = version;
        info(&format!("saved note {} at version {version}", id.0));
        if let Ok(payload) = encode(&(id.0, version)) {
            emit("saved", &payload);
        }
    }

    /// Saves the draft: the note made, or changed.
    fn save(&self) {
        match self.id {
            Some(id) => {
                assets::update(id, self.draft.as_bytes());
            }
            None => {
                assets::create(NOTE, "Note", self.draft.as_bytes());
            }
        }
    }
}

impl Plugin for Notes {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("notes", "Notes");
        assets::list(NOTE);
        subscribe("hamlet.board/seen");
    }

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        world.init_resource::<Note>();
        column((
            heading("Notes"),
            field(resource::<Note, _>(|note| note.draft.clone()))
                .placeholder("The note")
                .on_commit(|world, text| {
                    world.resource_mut::<Note>().draft = text;
                }),
            icon_button("check", "Save").on_click(|world| {
                world.resource::<Note>().save();
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

    fn receive(&mut self, message: Message, world: &mut World) {
        let note = world.resource_or_init::<Note>();
        match message {
            Message::Reply { reply, .. } => match reply {
                Ok(Reply::Listed(found)) => match found.first() {
                    Some(found) => {
                        note.id = Some(found.id);
                        note.version = found.version;
                        assets::read(found.id);
                    }
                    None => {
                        note.draft = "A first note".into();
                        note.save();
                    }
                },
                Ok(Reply::Read(asset)) => {
                    note.draft =
                        String::from_utf8_lossy(&asset.bytes)
                            .into_owned();
                    note.version = asset.info.version;
                }
                Ok(Reply::Created(id)) => note.saved(id, 1),
                Ok(Reply::Updated(version)) => {
                    if let Some(id) = note.id {
                        note.saved(id, version);
                    }
                }
                Ok(_) => {}
                Err(why) => warn(&why.to_string()),
            },
            Message::Event { payload, .. } => {
                if let Ok(version) = decode::<u64>(&payload) {
                    note.seen = Some(version);
                }
            }
            _ => {}
        }
    }

    fn serve(
        &mut self,
        service: &str,
        caller: &str,
        argument: &[u8],
        _: &mut World,
    ) -> Result<Vec<u8>, String> {
        if service != "words" {
            return Err(format!("there is no service {service}"));
        }
        let text = decode::<String>(argument)
            .map_err(|why| why.to_string())?;
        let words = text.split_whitespace().count() as u32;
        info(&format!("counted {words} words for {caller}"));
        encode(&words).map_err(|why| why.to_string())
    }

    fn save(&self, world: &World) -> Vec<u8> {
        let Some(note) = world.get::<Note>() else {
            return Vec::new();
        };
        let state = (
            note.id.map(|id| id.0),
            note.version,
            &note.draft,
            note.seen,
        );
        encode(&state).unwrap_or_default()
    }

    fn restore(&mut self, state: &[u8], world: &mut World) {
        if let Ok((id, version, draft, seen)) =
            decode::<(Option<u64>, u64, String, Option<u64>)>(state)
        {
            world.insert(Note {
                id: id.map(AssetId),
                version,
                draft,
                seen,
            });
        }
    }
}

hamlet_engine_sdk::export!(Notes);

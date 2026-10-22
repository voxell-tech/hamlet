//! A sample plugin that makes things for others: a note, kept as an
//! asset of its type `notes.note` (the bytes are the text), a service
//! `words` that counts the words of a text, and an event `saved` with
//! the note's id and version each time it is saved. It hears the
//! board's `seen`, and shows which version the board saw last.

use hamlet_engine_sdk::{
    Action, AssetId, Entry, Message, Plugin, Reply, Ui, WidgetEvent,
    assets, decode, emit, encode, info, subscribe, warn,
};

const NOTE: &str = "notes.note";

#[derive(Default)]
struct Notes {
    note: Option<AssetId>,
    version: u64,
    /// The text in the field, saved with Save.
    draft: String,
    /// The version the board saw last.
    seen: Option<u64>,
}

impl Notes {
    fn saved(&mut self, id: AssetId, version: u64) {
        self.note = Some(id);
        self.version = version;
        info(&format!("saved note {} at version {version}", id.0));
        if let Ok(payload) = encode(&(id.0, version)) {
            emit("saved", &payload);
        }
    }
}

impl Plugin for Notes {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("notes", "Notes");
        assets::list(NOTE);
        subscribe("hamlet.board/seen");
    }

    fn draw(&self, _: &str, ui: &mut Ui) {
        let seen = match self.seen {
            Some(version) => {
                format!("The board saw version {version}.")
            }
            None => "The board has not seen it yet.".into(),
        };
        ui.heading("title", "Notes")
            .field("text", &self.draft, "The note")
            .button("save", "Save")
            .text("version", format!("Version {}", self.version))
            .text("seen", seen);
    }

    fn event(&mut self, event: WidgetEvent) {
        match (event.widget.0.as_str(), event.action) {
            ("text", Action::Text(text)) => self.draft = text,
            ("save", Action::Click) => match self.note {
                Some(id) => {
                    assets::update(id, self.draft.as_bytes());
                }
                None => {
                    assets::create(
                        NOTE,
                        "Note",
                        self.draft.as_bytes(),
                    );
                }
            },
            _ => {}
        }
    }

    fn receive(&mut self, message: Message) {
        match message {
            Message::Reply { reply, .. } => match reply {
                Ok(Reply::Listed(found)) => match found.first() {
                    Some(note) => {
                        self.note = Some(note.id);
                        self.version = note.version;
                        assets::read(note.id);
                    }
                    None => {
                        self.draft = "A first note".into();
                        assets::create(
                            NOTE,
                            "Note",
                            self.draft.as_bytes(),
                        );
                    }
                },
                Ok(Reply::Read(note)) => {
                    self.draft = String::from_utf8_lossy(&note.bytes)
                        .into_owned();
                    self.version = note.info.version;
                }
                Ok(Reply::Created(id)) => self.saved(id, 1),
                Ok(Reply::Updated(version)) => {
                    if let Some(id) = self.note {
                        self.saved(id, version);
                    }
                }
                Ok(_) => {}
                Err(why) => warn(&why.to_string()),
            },
            Message::Event { payload, .. } => {
                if let Ok(version) = decode::<u64>(&payload) {
                    self.seen = Some(version);
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

    fn save(&self) -> Vec<u8> {
        let state = (
            self.note.map(|id| id.0),
            self.version,
            &self.draft,
            self.seen,
        );
        encode(&state).unwrap_or_default()
    }

    fn restore(&mut self, state: &[u8]) {
        if let Ok((note, version, draft, seen)) =
            decode::<(Option<u64>, u64, String, Option<u64>)>(state)
        {
            *self = Self {
                note: note.map(AssetId),
                version,
                draft,
                seen,
            };
        }
    }
}

hamlet_engine_sdk::export!(Notes);

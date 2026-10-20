//! A sample plugin that uses what another makes: it holds the notes
//! plugin's note by reference, so it shows each new version as it is
//! saved, asks the notes plugin's `words` service how many words it
//! has, and tells the notes plugin with its event `seen` which
//! version it shows.

use hamlet_plugin::{
    AssetId, Entry, Message, Plugin, Reply, Ui, assets, call, decode,
    emit, encode, info, subscribe, warn,
};

const NOTE: &str = "notes.note";

#[derive(Default)]
struct Board {
    /// The note held.
    note: Option<AssetId>,
    text: Option<String>,
    version: u64,
    words: Option<u32>,
}

impl Board {
    fn hold(&mut self, id: AssetId) {
        if self.note != Some(id) {
            self.note = Some(id);
            assets::hold(id);
        }
    }
}

impl Plugin for Board {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("board", "Board");
        subscribe("hamlet.notes/saved");
        assets::list(NOTE);
    }

    fn draw(&self, _: &str, ui: &mut Ui) {
        ui.heading("title", "Board");
        match &self.text {
            Some(text) => {
                let words = match self.words {
                    Some(1) => "1 word".to_string(),
                    Some(words) => format!("{words} words"),
                    None => "Counting...".into(),
                };
                ui.text("note", text.clone())
                    .text(
                        "version",
                        format!("Version {}", self.version),
                    )
                    .text("words", words);
            }
            None => {
                ui.text("note", "No note yet.");
            }
        }
    }

    fn receive(&mut self, message: Message) {
        match message {
            Message::Reply { reply, .. } => match reply {
                Ok(Reply::Listed(found)) => {
                    if let Some(note) = found.first() {
                        self.hold(note.id);
                    }
                }
                Ok(Reply::Held(_)) => {
                    if let Some(id) = self.note {
                        assets::read(id);
                    }
                }
                Ok(Reply::Read(note)) => {
                    let text = String::from_utf8_lossy(&note.bytes)
                        .into_owned();
                    self.version = note.info.version;
                    info(&format!(
                        "shows note {} at version {}",
                        note.info.id.0, self.version
                    ));
                    if let Ok(argument) = encode(&text) {
                        call("hamlet.notes/words", &argument);
                    }
                    if let Ok(payload) = encode(&self.version) {
                        emit("seen", &payload);
                    }
                    self.text = Some(text);
                    self.words = None;
                }
                Ok(Reply::Answered(result)) => {
                    self.words = decode::<u32>(&result).ok();
                    if let Some(words) = self.words {
                        info(&format!("the note has {words} words"));
                    }
                }
                Ok(_) => {}
                Err(why) => warn(&why.to_string()),
            },
            Message::Changed { asset, version } => {
                info(&format!(
                    "note {} changed to version {version}",
                    asset.0
                ));
                assets::read(asset);
            }
            Message::Gone(_) => *self = Self::default(),
            Message::Event { from, payload, .. } => {
                if let Ok((id, version)) =
                    decode::<(u64, u64)>(&payload)
                {
                    info(&format!(
                        "{from} saved note {id} at version {version}"
                    ));
                    self.hold(AssetId(id));
                }
            }
        }
    }
}

hamlet_plugin::export!(Board);

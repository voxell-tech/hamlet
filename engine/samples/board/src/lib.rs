//! A sample plugin that uses what another makes: it holds the notes
//! plugin's note by reference, so it shows each new version as it is
//! saved, asks the notes plugin's `words` service how many words it
//! has, and tells the notes plugin with its event `seen` which
//! version it shows.

use hamlet_engine_sdk::ui::{
    AnyView, ViewExt, column, heading, label, list, resource,
};
use hamlet_engine_sdk::{
    AssetId, Entry, Message, Plugin, Reply, World, assets, call,
    decode, emit, encode, info, subscribe, warn,
};

const NOTE: &str = "notes.note";

#[derive(Default)]
struct Board;

/// The note held, as the board shows it.
#[derive(Default)]
struct Note {
    id: Option<AssetId>,
    text: Option<String>,
    version: u64,
    words: Option<u32>,
}

impl Note {
    fn hold(&mut self, id: AssetId) {
        if self.id != Some(id) {
            self.id = Some(id);
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

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        world.init_resource::<Note>();
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

    fn receive(&mut self, message: Message, world: &mut World) {
        let note = world.resource_or_init::<Note>();
        match message {
            Message::Reply { reply, .. } => match reply {
                Ok(Reply::Listed(found)) => {
                    if let Some(found) = found.first() {
                        note.hold(found.id);
                    }
                }
                Ok(Reply::Held(_)) => {
                    if let Some(id) = note.id {
                        assets::read(id);
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
                        call("hamlet.notes/words", &argument);
                    }
                    if let Ok(payload) = encode(&note.version) {
                        emit("seen", &payload);
                    }
                    note.text = Some(text);
                    note.words = None;
                }
                Ok(Reply::Answered(result)) => {
                    note.words = decode::<u32>(&result).ok();
                    if let Some(words) = note.words {
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
            Message::Gone(_) => *note = Note::default(),
            Message::Event { from, payload, .. } => {
                if let Ok((id, version)) =
                    decode::<(u64, u64)>(&payload)
                {
                    info(&format!(
                        "{from} saved note {id} at version {version}"
                    ));
                    note.hold(AssetId(id));
                }
            }
        }
    }
}

hamlet_engine_sdk::export!(Board);

//! What an event does: the button runs a command of Notes, the
//! reply and the events of Notes and of the host change the model.

use hamlet_guest::{Incoming, Intent, calls, events};

use crate::model::{Model, Why};

/// The plugin whose command, events and collection are used.
pub const NOTES: &str = "notes";

/// Carries out one event. `out` gets what the plugin sends to others.
pub fn handle(
    incoming: Incoming,
    model: &mut Model,
    out: &mut Vec<Intent>,
) {
    if let Some((Why::AddNote, outcome)) =
        model.calls.route(&incoming)
    {
        model.line = match calls::returned::<u64>(&outcome) {
            Ok(id) => format!("Notes saved it as note {id}."),
            Err(why) => format!("Notes did not take it: {why}."),
        };
        return;
    }
    match incoming {
        Incoming::Click(press) if press.id == "send" => {
            send(model, out)
        }
        Incoming::Event(event) => {
            match (event.sender.as_str(), event.topic.as_str()) {
                (NOTES, "note-saved") => model.saved += 1,
                ("host", "selection-changed") => {
                    model.selected = events::text(&event);
                }
                _ => {}
            }
        }
        Incoming::Granted(name) if name.starts_with("plugin.use") => {
            model.line =
                "You allowed it. Press the button again.".into();
        }
        Incoming::Denied(name) if name.starts_with("plugin.use") => {
            model.line = "Not allowed to use Notes.".into();
        }
        _ => {}
    }
}

fn send(model: &mut Model, out: &mut Vec<Intent>) {
    if model.selected.is_empty() {
        model.line = "Select an agent in the world first.".into();
        return;
    }
    let name = model.selected.clone();
    match model.calls.call(NOTES, "add", &name, Why::AddNote) {
        Ok((_, intent)) => {
            out.push(intent);
            model.line = "Sent. Waiting for Notes.".into();
        }
        Err(why) => model.line = why,
    }
}

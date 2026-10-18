//! What an event does: the button runs a command of Notes, the
//! reply and the events of Notes and of the host change the model.

use hamlet_guest::{
    CommandCall, Incoming, Intent, calls, events, subject,
};

use crate::model::{Model, Why};

/// The plugin whose command, events and collection are used.
pub const NOTES: &str = "notes";

/// The command that sends the name of the thing it is run on to
/// Notes. The context menu of an agent runs it.
pub const CLIP: &str = "clip";

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
            if model.selected.is_empty() {
                model.line =
                    "Select an agent in the world first.".into();
            } else {
                let name = model.selected.clone();
                clip(&name, model, out);
            }
        }
        Incoming::Command(call) if call.command == CLIP => {
            answer(&call, model, out)
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

/// Runs the command a context menu item chose, on the thing it was
/// chosen on, and answers it.
fn answer(
    call: &CommandCall,
    model: &mut Model,
    out: &mut Vec<Intent>,
) {
    let outcome = subject::of(call).map(|thing| {
        clip(&thing.name, model, out);
    });
    if let Err(why) = &outcome {
        model.line = why.clone();
    }
    out.push(calls::reply(
        call,
        outcome.as_ref().map_err(String::clone),
    ));
}

/// Sends `name` to Notes, to be added as a note.
fn clip(name: &str, model: &mut Model, out: &mut Vec<Intent>) {
    match model.calls.call(NOTES, "add", &name, Why::AddNote) {
        Ok((_, intent)) => {
            out.push(intent);
            model.line = "Sent. Waiting for Notes.".into();
        }
        Err(why) => model.line = why,
    }
}

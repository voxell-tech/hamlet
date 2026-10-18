//! A panel with a button, a count kept in state and one read that
//! needs the chief's leave.

use hamlet_guest::ui::{Node, View, button, column, heading, note, row};
use hamlet_guest::{
    Incoming, Intent, Level, Plugin, RequestResult, State,
};
use serde::{Deserialize, Serialize};

const PANEL: &str = "main";

/// What the plugin keeps while the host runs. The host holds it across
/// a reload; anything that must outlive Hamlet goes in
/// `hamlet_guest::storage`.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    looks: u32,
}

struct Main;

impl Plugin for Main {
    fn entry() {
        hamlet_guest::panel(PANEL, "{{TITLE}}", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut intents = Vec::new();
        for incoming in inbox {
            match incoming {
                // The button counts a look and draws the panel again.
                Incoming::Click(press) if press.id == "look" => {
                    let mut model = State::<Model>::load();
                    model.looks += 1;
                    if let Err(why) = State::save(&model) {
                        hamlet_guest::log(Level::Warn, &why);
                    }
                    intents.push(Intent::Redraw(press.panel));
                }
                // The chief answered a request that was pending.
                Incoming::Granted(capability) => {
                    hamlet_guest::log(
                        Level::Info,
                        &format!("allowed to use {capability}"),
                    );
                    intents.push(Intent::Redraw(PANEL.into()));
                }
                Incoming::Denied(capability) => {
                    hamlet_guest::log(
                        Level::Warn,
                        &format!("not allowed to use {capability}"),
                    );
                    intents.push(Intent::Redraw(PANEL.into()));
                }
                _ => {}
            }
        }
        intents
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        column([
            heading("{{TITLE}}"),
            row([button("Look").id("look")]),
            note(&format!("Looked {} times.", model.looks)).id("looks"),
            agents(),
        ])
        .into()
    }
}

/// How many agents live here, or why that is not shown.
fn agents() -> Node {
    let reason = "To count the agents the panel is about.";
    match hamlet_guest::request("read.agents", reason) {
        RequestResult::Granted => match hamlet_guest::agents() {
            Ok(list) => note(&format!("{} agents live here.", list.len()))
                .id("count"),
            Err(why) => note(&format!("Not shown: {why}.")).id("count"),
        },
        RequestResult::Pending => {
            note("Waiting for the chief to answer.").id("count")
        }
        RequestResult::Denied => {
            note("The chief said no, so there is no count.").id("count")
        }
    }
}

hamlet_guest::export!(Main);

//! A panel with a button and one read that needs the chief's leave.

use hamlet_guest::ui::{Node, View, button, column, heading, note, row};
use hamlet_guest::{Incoming, Intent, Level, Plugin, RequestResult};

const PANEL: &str = "main";

struct Main;

impl Plugin for Main {
    fn entry() {
        hamlet_guest::panel(PANEL, "{{TITLE}}", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut intents = Vec::new();
        for incoming in inbox {
            match incoming {
                // The button asks for the panel to be drawn again.
                Incoming::Click(press) if press.id == "look" => {
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
        column([
            heading("{{TITLE}}"),
            row([button("Look").id("look")]),
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

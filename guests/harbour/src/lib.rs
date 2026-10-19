//! A plugin that calls the typed interface `tide-table` of the
//! plugins that implement it: it lists them, asks one for the height
//! of the tide at a port and shows what came back, or why nothing
//! did. A button sends a number where the port goes, which the host
//! refuses before the call is made.

use hamlet_guest::invoke::{self, Invokes};
use hamlet_guest::ui::{
    View, button, column, heading, note, row, text_field,
};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

const PANEL: &str = "main";

const TABLE: &str = "tide-table";

/// The port asked about until another is typed.
const PORT: &str = "Kettlebay";

/// What the panel shows, which the host keeps across a reload.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    /// The port to ask about.
    port: String,
    /// The plugins that implement the table, when last looked.
    found: Vec<String>,
    /// What came of the last call.
    answer: String,
    /// The calls waiting, each with the port it asks about.
    calls: Invokes<String>,
}

struct Harbour;

impl Plugin for Harbour {
    fn entry() {
        hamlet_guest::panel(PANEL, "HARBOUR", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut out = Vec::new();
        for incoming in inbox {
            handle(incoming, &mut model, &mut out);
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        // An answer is not an event of the panel.
        out.push(Intent::Redraw(PANEL.into()));
        out
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let port = match model.port.is_empty() {
            true => PORT,
            false => &model.port,
        };
        let found = match model.found.is_empty() {
            true => "Implementers: none found yet.".to_string(),
            false => {
                format!("Implementers: {}", model.found.join(", "))
            }
        };
        let asks = model.found.iter().map(|id| {
            button(&format!("Ask {id}")).id(&format!("ask.{id}"))
        });
        column([
            heading("HARBOUR"),
            note("Asks the plugins that keep a tide-table."),
            row([text_field(port).id("port")]),
            row([
                button("Find").id("find"),
                button("Send a number").id("number"),
            ]),
            note(&found).id("found"),
            row(asks),
            note(&model.answer).id("answer"),
        ])
        .into()
    }
}

fn handle(
    incoming: Incoming,
    model: &mut Model,
    out: &mut Vec<Intent>,
) {
    if let Some((port, outcome)) = model.calls.route(&incoming) {
        model.answer = match invoke::returned::<Option<f64>>(&outcome)
        {
            Ok(Some(metres)) => format!("{port}: {metres} metres"),
            Ok(None) => format!("{port}: no table"),
            Err(why) => why,
        };
        return;
    }
    match incoming {
        Incoming::TextChanged(typed) if typed.id == "port" => {
            model.port = typed.text;
        }
        Incoming::Click(press) if press.id == "find" => {
            match invoke::implementers(TABLE) {
                Ok(found) => model.found = found,
                Err(why) => model.answer = why,
            }
        }
        Incoming::Click(press) => {
            let port = match model.port.is_empty() {
                true => PORT.to_string(),
                false => model.port.clone(),
            };
            let called = match press.id.strip_prefix("ask.") {
                Some(plugin) => model.calls.call(
                    plugin,
                    (TABLE, "height"),
                    &(&port,),
                    port.clone(),
                ),
                None if press.id == "number" => {
                    let plugin =
                        model.found.first().map_or("tides", |id| id);
                    model.calls.call(
                        plugin,
                        (TABLE, "height"),
                        &(7u32,),
                        "7".into(),
                    )
                }
                None => return,
            };
            match called {
                Ok(intent) => out.push(intent),
                Err(why) => model.answer = why,
            }
        }
        _ => {}
    }
}

hamlet_guest::export!(Harbour);

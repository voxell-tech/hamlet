//! A plugin that gives the agents a tool, `height`, which looks up
//! the tide of a port in a table made up for the example. Its panel
//! counts the calls. What the tool returns is shown to the agent as
//! text from this plugin, so it is never read as an instruction.

use hamlet_guest::tool::{self, flag, text};
use hamlet_guest::ui::{View, column, heading, note};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

/// The ports and the height of their high tide, in metres.
const PORTS: [(&str, f64); 2] = [("Dover", 6.1), ("Hull", 7.4)];

const FEET_IN_A_METRE: f64 = 3.28084;

/// What the host holds across a reload.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    calls: u32,
}

struct Tides;

impl Plugin for Tides {
    fn entry() {
        hamlet_guest::panel("tides", "TIDES", "book");
        tool::declare(
            "height",
            "The height of the high tide at a port.",
            &[
                text(
                    "port",
                    "The port, such as Dover or Hull.",
                    true,
                ),
                flag("feet", "Answer in feet, not metres.", false),
            ],
        );
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut intents = Vec::new();
        for incoming in inbox {
            let Incoming::Command(call) = incoming else {
                continue;
            };
            if !tool::from_agent(&call) || call.command != "height" {
                continue;
            }
            model.calls += 1;
            let answer =
                tool::args(&call).and_then(|args| height(&args));
            intents.push(tool::reply(&call, answer));
            intents.push(Intent::Redraw("tides".into()));
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        intents
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        column([
            heading("TIDES"),
            note("Gives the agents one tool, height."),
            note(&format!("Calls: {}", model.calls)),
        ])
        .into()
    }
}

/// The words that answer a call of `height`.
fn height(args: &tool::Args) -> Result<String, String> {
    let port = args.text("port").ok_or("no port was given")?;
    let (name, metres) = PORTS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(port))
        .ok_or_else(|| format!("no table for {port}"))?;
    Ok(match args.flag("feet") {
        Some(true) => format!(
            "High tide at {name}: {:.0} feet",
            metres * FEET_IN_A_METRE
        ),
        _ => format!("High tide at {name}: {metres} metres"),
    })
}

hamlet_guest::export!(Tides);

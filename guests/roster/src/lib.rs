//! A panel that lists the agents. Refresh reads them again, and a
//! checkbox shows or hides a line of help.

use hamlet_guest::ui::{
    Node, View, button, checkbox, column, divider, heading, icon,
    label, note, row, scroll,
};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

const PANEL: &str = "roster";

/// What the panel shows, which the host keeps across a reload.
#[derive(Serialize, Deserialize)]
struct Model {
    refreshes: u32,
    hint: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            refreshes: 0,
            hint: true,
        }
    }
}

struct Roster;

impl Plugin for Roster {
    fn entry() {
        hamlet_guest::panel(PANEL, "ROSTER", "robot");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut changed = false;
        for incoming in inbox {
            match incoming {
                Incoming::Click(press) if press.id == "refresh" => {
                    model.refreshes += 1;
                    changed = true;
                }
                Incoming::Toggled(toggle) if toggle.id == "hint" => {
                    model.hint = toggle.on;
                    changed = true;
                }
                _ => {}
            }
        }
        if changed && let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        Vec::new()
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let mut rows = vec![
            row([icon("robot"), heading("EVERYONE IN THE REALM")]),
            row([button("Refresh").id("refresh")]),
            label(&format!("Refreshed {} times", model.refreshes))
                .id("count"),
            checkbox("Show the hint", model.hint).id("hint"),
        ];
        if model.hint {
            rows.push(
                note("Refresh reads the agents again.").id("help"),
            );
        }
        rows.push(divider());
        rows.push(agents());
        column(rows).into()
    }
}

/// The agents, or why they are not shown.
fn agents() -> Node {
    match hamlet_guest::agents() {
        Ok(agents) if agents.is_empty() => {
            note("Nobody lives here yet.").id("nobody")
        }
        Ok(agents) => scroll(agents.iter().map(|agent| {
            row([label(&agent.name), note(&agent.hamlet)])
                .id(&format!("{}/{}", agent.hamlet, agent.name))
        }))
        .id("agents"),
        Err(why) => note(&format!("Not shown: {why}.")).id("denied"),
    }
}

hamlet_guest::export!(Roster);

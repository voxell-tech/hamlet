//! A panel that lists the agents. Refresh reads them again, and a
//! checkbox shows or hides a line of help.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use hamlet_guest::ui::{
    Node, button, checkbox, column, divider, heading, icon, label,
    note, row, scroll,
};
use hamlet_guest::{Incoming, Intent, Plugin};

const PANEL: &str = "roster";

static REFRESHES: AtomicU32 = AtomicU32::new(0);
static HINT: AtomicBool = AtomicBool::new(true);

struct Roster;

impl Plugin for Roster {
    fn entry() {
        hamlet_guest::panel(PANEL, "ROSTER", "robot");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        for incoming in inbox {
            match incoming {
                Incoming::Click(press) if press.id == "refresh" => {
                    REFRESHES.fetch_add(1, Ordering::Relaxed);
                }
                Incoming::Toggled(toggle) if toggle.id == "hint" => {
                    HINT.store(toggle.on, Ordering::Relaxed);
                }
                _ => {}
            }
        }
        Vec::new()
    }

    fn panel_view(_panel: &str) -> Node {
        let refreshed = REFRESHES.load(Ordering::Relaxed);
        let mut rows = vec![
            row([icon("robot"), heading("EVERYONE IN THE REALM")]),
            row([button("Refresh").id("refresh")]),
            label(&format!("Refreshed {refreshed} times")).id("count"),
            checkbox("Show the hint", HINT.load(Ordering::Relaxed))
                .id("hint"),
        ];
        if HINT.load(Ordering::Relaxed) {
            rows.push(
                note("Refresh reads the agents again.").id("help"),
            );
        }
        rows.push(divider());
        rows.push(agents());
        column(rows)
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

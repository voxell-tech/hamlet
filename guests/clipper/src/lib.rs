//! Clipper: a button that sends the selected agent's name to Notes,
//! the titles of the notes it reads from Notes, and a count of the
//! notes Notes says it saved.

mod model;
mod update;
mod view;

use hamlet_guest::ui::View;
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};

use crate::model::Model;

const PANEL: &str = "main";

struct Clipper;

impl Plugin for Clipper {
    fn entry() {
        hamlet_guest::panel(PANEL, "CLIPPER", "notes");
        hamlet_guest::command(
            update::CLIP,
            "Clip name to Notes",
            None,
        );
        hamlet_guest::context_item(
            "agent",
            200,
            "Clip name to Notes",
            update::CLIP,
        );
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let events = inbox
            .into_iter()
            .filter(|incoming| !matches!(incoming, Incoming::Tick(_)))
            .collect::<Vec<_>>();
        if events.is_empty() {
            return Vec::new();
        }
        let mut model = State::<Model>::load();
        let mut out = Vec::new();
        for incoming in events {
            update::handle(incoming, &mut model, &mut out);
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        out.push(Intent::Redraw(PANEL.into()));
        out
    }

    fn panel_view(_panel: &str) -> View {
        view::panel(&State::<Model>::load()).into()
    }
}

hamlet_guest::export!(Clipper);

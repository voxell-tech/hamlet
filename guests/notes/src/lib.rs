//! Notes: a list of notes kept in the plugin's own storage, each
//! edited in a text area or read as markdown.

mod model;
mod storage;
mod update;
mod view;

use hamlet_guest::ui::View;
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};

use crate::model::Session;

const PANEL: &str = "main";

struct Notes;

impl Plugin for Notes {
    fn entry() {
        hamlet_guest::panel(PANEL, "NOTES", "notes");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let events = inbox
            .into_iter()
            .filter(|incoming| !matches!(incoming, Incoming::Tick(_)))
            .collect::<Vec<_>>();
        if events.is_empty() {
            return Vec::new();
        }
        let mut session = State::<Session>::load();
        let mut index = storage::index();
        for incoming in events {
            update::handle(incoming, &mut session, &mut index);
        }
        if let Err(why) = State::save(&session) {
            hamlet_guest::log(Level::Warn, &why);
        }
        vec![Intent::Redraw(PANEL.into())]
    }

    fn panel_view(_panel: &str) -> View {
        let session = State::<Session>::load();
        view::panel(&session, &storage::index()).into()
    }
}

hamlet_guest::export!(Notes);

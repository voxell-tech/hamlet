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
/// The page of the Settings.
const SETTINGS: &str = "settings";
/// The tab of the Inspector's page of an agent.
const MENTIONS: &str = "mentions";

struct Notes;

impl Plugin for Notes {
    fn entry() {
        hamlet_guest::panel(PANEL, "NOTES", "notes");
        hamlet_guest::panel(SETTINGS, "NOTES SETTINGS", "sliders");
        hamlet_guest::panel(MENTIONS, "NOTES OF AN AGENT", "book");
        hamlet_guest::command(update::ADD, "Add a note", None);
        hamlet_guest::command(update::NEW, "New note", Some("plus"));
        hamlet_guest::menu_item("hamlet", 1, "New note", update::NEW);
        hamlet_guest::key("Cmd+Shift+N", update::NEW);
        hamlet_guest::settings_page("NOTES", "notes", SETTINGS);
        hamlet_guest::inspector_section("agent", "NOTES", MENTIONS);
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
        let mut out = Vec::new();
        for incoming in events {
            update::handle(
                incoming,
                &mut session,
                &mut index,
                &mut out,
            );
        }
        if let Err(why) = State::save(&session) {
            hamlet_guest::log(Level::Warn, &why);
        }
        out.extend(
            [PANEL, SETTINGS, MENTIONS]
                .map(|panel| Intent::Redraw(panel.into())),
        );
        out
    }

    fn panel_view(panel: &str) -> View {
        match panel {
            SETTINGS => view::settings(&storage::prefs()).into(),
            MENTIONS => {
                let session = State::<Session>::load();
                view::mentions(&session, &storage::index()).into()
            }
            _ => {
                let session = State::<Session>::load();
                view::panel(&session, &storage::index()).into()
            }
        }
    }
}

hamlet_guest::export!(Notes);

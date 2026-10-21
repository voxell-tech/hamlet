//! The realm behind the dock: the endless land, a town for every
//! hamlet with a house and a robot for every agent, the roads between
//! them, and the chief's Hearth, drawn from the agents service's
//! roster. Clicking a robot, a house or a common selects it in the
//! window's focus and brings the Inspector forward.

mod bridge;
mod camera;
mod chief;
mod flora;
mod frame;
mod ground;
mod house;
mod keeper;
mod land;
mod lantern;
mod layout;
mod mesh;
mod moving;
mod noise;
mod palette;
mod pick;
mod realm;
mod road;
mod shift;
mod signs;
mod sky;
mod society;
mod stock;
mod tint;
mod town;
mod tuning;
mod upload;
mod yard;

use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Selected,
};
use hamlet_engine_sdk::scene::{SceneInput, animate};
use hamlet_engine_sdk::ui::open;
use hamlet_engine_sdk::{
    Entry, Message, Plugin, Reply, World, agents, decode, error,
    subscribe, warn,
};

use self::realm::Realm;
use self::sky::Hour;

/// The Inspector, brought forward when something is chosen here.
const INSPECTOR: &str = "hamlet.inspector/inspector";

/// The Inspector's event that has the realm take a hamlet's town in
/// hand, its payload the hamlet's name on disk.
const CARRY: &str = "hamlet.inspector/carry";

/// The longest frame the realm moves by, in seconds, so a frame that
/// came late does not throw things far.
const LONGEST_FRAME: f32 = 0.1;

#[derive(Default)]
struct RealmPlugin {
    /// The realm, once the first answer of the agents service came.
    realm: Option<Realm>,
    /// Why the realm cannot be drawn, once it could not.
    broken: bool,
}

impl Plugin for RealmPlugin {
    fn declare(&self, entry: &mut Entry) {
        let commands = [
            ("reset_camera", "Reset Camera", "View", None),
            ("focus", "Focus on Selection", "View", Some("F")),
            ("follow", "Follow Selection", "View", Some("Shift+F")),
            ("sky_clock", "Sky Follows the Clock", "View", None),
            ("sky_day", "Sky at Day", "View", None),
            ("sky_night", "Sky at Night", "View", None),
        ];
        for (name, label, menu, keys) in commands {
            entry.command(name, label).menu_entry(menu, name);
            if let Some(keys) = keys {
                entry.shortcut(keys, name);
            }
        }
        for request in [
            AgentsRequest::Watch,
            AgentsRequest::Roster,
            AgentsRequest::Focus,
        ] {
            agents(&request);
        }
        subscribe(CARRY);
    }

    fn run(&mut self, name: &str, _: &mut World) {
        let Some(realm) = &mut self.realm else {
            return;
        };
        match name {
            "reset_camera" => realm.reset_camera(),
            "focus" => realm.focus_on_selected(false),
            "follow" => realm.focus_on_selected(true),
            "sky_clock" => realm.set_hour(Hour::Clock),
            "sky_day" => realm.set_hour(Hour::Day),
            "sky_night" => realm.set_hour(Hour::Night),
            _ => {}
        }
    }

    fn receive(&mut self, message: Message, _: &mut World) {
        let reply = match message {
            Message::Reply { reply, .. } => reply,
            Message::Event { payload, .. } => {
                match decode::<String>(&payload) {
                    Ok(hamlet) => {
                        if let Some(realm) = self.realm() {
                            realm.carry(&hamlet);
                        }
                    }
                    Err(why) => warn(&format!("carry: {why}")),
                }
                return;
            }
            _ => return,
        };
        let reply = match reply {
            Ok(Reply::Agents(reply)) => reply,
            Ok(_) => return,
            Err(why) => {
                warn(&format!("the agents: {why}"));
                return;
            }
        };
        let Some(realm) = self.realm() else {
            return;
        };
        match reply {
            AgentsReply::Roster(roster)
            | AgentsReply::Event(AgentsEvent::Roster(roster)) => {
                realm.follow(&roster);
            }
            AgentsReply::Focus(focus)
            | AgentsReply::Event(AgentsEvent::Focus(focus)) => {
                realm.focus(&focus);
            }
            _ => {}
        }
    }

    fn scene(&mut self, input: SceneInput, _: &mut World) {
        if let Some(realm) = &mut self.realm {
            realm.input(&input);
        }
    }

    fn tick(&mut self, micros: u32, _: &mut World) {
        let Some(realm) = &mut self.realm else {
            return;
        };
        let dt = (micros as f32 / 1_000_000.0).min(LONGEST_FRAME);
        realm.tick(dt);
        if let Some(selected) = realm.choice() {
            // The bare ground only clears what was selected.
            let something = selected != Selected::Nothing;
            agents(&AgentsRequest::Select(selected));
            if something {
                open(INSPECTOR);
            }
        }
        if let Some((hamlet, text)) = realm.set_down() {
            agents(&AgentsRequest::Decree { hamlet, text });
        }
    }
}

impl RealmPlugin {
    /// The realm, made the first time it is asked for.
    fn realm(&mut self) -> Option<&mut Realm> {
        if self.realm.is_none() && !self.broken {
            match Realm::make() {
                Ok(realm) => {
                    self.realm = Some(realm);
                    animate(true);
                }
                Err(why) => {
                    error(&format!(
                        "the realm cannot be drawn: {why}"
                    ));
                    self.broken = true;
                }
            }
        }
        self.realm.as_mut()
    }
}

hamlet_engine_sdk::export!(RealmPlugin);

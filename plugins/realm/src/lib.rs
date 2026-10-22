//! The realm in a panel of its own: the endless land, a town for
//! every hamlet with a house and a robot for every agent, the roads
//! between them, and the chief's Hearth, drawn from the agents
//! service's roster. Clicking a robot, a house or a common selects it
//! in the window's focus and brings the Inspector forward.

mod bridge;
mod build;
mod folk;
mod places;
mod realm;
mod society;
mod tuning;
mod view;
mod world;

use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Selected,
};
use hamlet_engine_sdk::scene::{SceneInput, animate, viewport};
use hamlet_engine_sdk::ui::{
    AnyView, FrameProps, Space, Tone, UiExt, ViewExt, column, label,
    open, resource,
};
use hamlet_engine_sdk::{
    Entry, Message, Plugin, Reply, World, agents, decode, error,
    subscribe, warn,
};

use crate::realm::Realm;
use crate::world::sky::Hour;

/// The name of the panel the realm is drawn in.
const PANEL: &str = "realm";

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
    /// True once the realm could not be made.
    broken: bool,
}

impl Plugin for RealmPlugin {
    fn declare(&self, entry: &mut Entry) {
        entry
            .panel_with_icon(PANEL, "Realm", "globe")
            .padding(Space::None);
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

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        if self.realm(world).is_none() {
            return column((label(
                "The realm cannot be drawn: the log says why",
            )
            .tone(Tone::Dim),))
            .padding(Space::Double)
            .boxed();
        }
        viewport::<Realm>(|realm| Some(realm.lens()))
            .pointer(resource::<Realm, _>(Realm::pointer))
            .boxed()
    }

    fn run(&mut self, name: &str, world: &mut World) {
        let Some(realm) = made(world) else {
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

    fn receive(&mut self, message: Message, world: &mut World) {
        let reply = match message {
            Message::Reply { reply, .. } => reply,
            Message::Event { payload, .. } => {
                match decode::<String>(&payload) {
                    Ok(hamlet) => {
                        if let Some(realm) = self.realm(world) {
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
        let Some(realm) = self.realm(world) else {
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

    fn scene(&mut self, input: SceneInput, world: &mut World) {
        if let Some(realm) = made(world) {
            realm.input(&input);
        }
    }

    fn tick(&mut self, micros: u32, world: &mut World) {
        let Some(realm) = made(world) else {
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
    fn realm<'a>(
        &mut self,
        world: &'a mut World,
    ) -> Option<&'a mut Realm> {
        if world.get::<Realm>().is_none() && !self.broken {
            match Realm::make() {
                Ok(realm) => {
                    world.insert(realm);
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
        made(world)
    }
}

/// The realm, once it is made.
fn made(world: &mut World) -> Option<&mut Realm> {
    world.get::<Realm>()?;
    Some(world.resource_mut::<Realm>())
}

hamlet_engine_sdk::export!(RealmPlugin);

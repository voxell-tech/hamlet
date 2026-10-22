//! The realm in a panel of its own: the endless land, a town for
//! every hamlet with a house and a robot for every agent, the roads
//! between them, and the chief's Hearth, drawn from the agents
//! service's roster. Clicking a robot, a house or a common selects it
//! in the window's focus and brings the Inspector forward. Clicking a
//! stall of the Agent Bazaar sends `stall` with the stall's short
//! name, and a `traded` of `hamlet.bazaar` with one has the stall
//! show the sale. Agents and hamlets wear the looks the bazaar keeps
//! in the store (docs/looks.md), and its `emote` has an agent's robot
//! play its clip. Its service `diorama` hands over the town of a
//! hamlet template, for the Agent Bazaar to show on a plinth.

mod bridge;
mod build;
mod diorama;
mod folk;
mod places;
mod realm;
mod society;
mod tuning;
mod view;
mod wardrobe;
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
    Entry, Message, Plugin, Reply, World, agents, assets, decode,
    emit, encode, error, subscribe, warn,
};
use looks::{FILE, PLAY, WORN};

use crate::diorama::Dioramas;
use crate::realm::Realm;
use crate::wardrobe::closet::{Ask, Closet};
use crate::world::sky::Hour;

/// The name of the panel the realm is drawn in.
const PANEL: &str = "realm";

/// The Inspector, brought forward when something is chosen here.
const INSPECTOR: &str = "hamlet.inspector/inspector";

/// The Inspector's event that has the realm take a hamlet's town in
/// hand, its payload the hamlet's name on disk.
const CARRY: &str = "hamlet.inspector/carry";

/// The realm's event that a stall of the bazaar was clicked, its
/// payload the stall's short name.
const STALL: &str = "stall";

/// The bazaar's event that the chief bought something at a stall, its
/// payload the stall's short name.
const TRADED: &str = "hamlet.bazaar/traded";

/// The service that hands over the town of a hamlet template.
const DIORAMA: &str = "diorama";

/// The longest frame the realm moves by, in seconds, so a frame that
/// came late does not throw things far.
const LONGEST_FRAME: f32 = 0.1;

#[derive(Default)]
struct RealmPlugin {
    /// True once the realm could not be made.
    broken: bool,
    dioramas: Dioramas,
    /// The reading of what is worn, and the seconds it has run.
    closet: Closet,
    clock: f32,
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
            ("bazaar", "Go to the Bazaar", "View", None),
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
        subscribe(TRADED);
        subscribe(PLAY);
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
            "bazaar" => realm.go_to_bazaar(),
            _ => {}
        }
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        let reply = match message {
            Message::Reply { ticket, reply } => {
                if self.closet.owns(ticket) {
                    self.closet.answer(ticket, reply);
                    return;
                }
                reply
            }
            Message::Changed { asset, .. } => {
                self.closet.changed(asset);
                return;
            }
            Message::Gone(asset) => {
                self.closet.gone(asset);
                return;
            }
            Message::Event { event, payload, .. } => {
                let event = event.as_str();
                match decode::<String>(&payload) {
                    Ok(said) => {
                        if let Some(realm) = self.realm(world) {
                            match event {
                                TRADED => realm.trade(&said),
                                PLAY => realm.gesture(&said),
                                _ => realm.carry(&said),
                            }
                        }
                    }
                    Err(why) => warn(&format!("{event}: {why}")),
                }
                return;
            }
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

    fn serve(
        &mut self,
        service: &str,
        _: &str,
        argument: &[u8],
        _: &mut World,
    ) -> Result<Vec<u8>, String> {
        match service {
            DIORAMA => self.dioramas.serve(argument),
            _ => Err(format!("there is no service {service}")),
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
        self.clock += dt;
        for ask in self.closet.asks(&realm.closet(), self.clock) {
            let ticket = match &ask {
                Ask::ListWorn => assets::list(WORN),
                Ask::ListFiles => assets::list(FILE),
                Ask::Hold(id) => assets::hold(*id),
                Ask::ReadWorn(id) | Ask::ReadPart { id, .. } => {
                    assets::read(*id)
                }
            };
            self.closet.sent(ticket, ask);
        }
        for note in self.closet.notes() {
            warn(&format!("looks: {note}"));
        }
        if let Some(delivery) = self.closet.take() {
            realm.wear(delivery);
        }
        if let Some(selected) = realm.choice() {
            // The bare ground only clears what was selected.
            let something = selected != Selected::Nothing;
            agents(&AgentsRequest::Select(selected));
            if something {
                open(INSPECTOR);
            }
        }
        if let Some(stall) = realm.take_stall() {
            match encode(&stall.short().to_owned()) {
                Ok(payload) => {
                    emit(STALL, &payload);
                }
                Err(why) => warn(&format!("{STALL}: {why}")),
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

//! What a right click on the realm asks: the menu of a thing or of
//! the bare ground, the question before a deed that ends an agent or
//! a hamlet, and the form a hamlet is founded with.

mod deed;
mod found;
mod menu;
mod wardrobe;

use hamlet_engine_bevy::api::agents::{
    AgentsReply, AgentsRequest, Gift, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::open;
use hamlet_engine_bevy::{Reply, Ticket};

use crate::ask::deed::{Asking, Deed};
use crate::ask::found::Forming;
use crate::ask::menu::Menus;
use crate::ask::wardrobe::Dressing;
use crate::realm::{
    Hear, INSPECTOR, Things, focus_on_thing, pick_up, steer,
};
use crate::society::Place;
use crate::sound::{Cue, Sound};
use crate::view::pick::{Picking, Thing};

/// The agents' chat, brought forward to talk to an agent.
const CHAT: &str = "hamlet.agents/chat";

/// The widest a question or the form is drawn, in pixels.
const QUESTION_WIDTH: f32 = 300.0;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Offering>()
        .init_resource::<found::Founding>()
        .add_systems(
            Update,
            (ask, offered, close).chain().in_set(Hear).after(steer),
        );
}

/// The cards that ask something of the person.
type Asked = Or<(With<Asking>, With<Forming>, With<Dressing>)>;

/// The thing asked about, until the skills its agent could be given
/// are told.
#[derive(Resource, Default)]
struct Offering(Option<(Thing, Ticket)>);

/// The realm's methods on the [`World`], for a menu item to run.
pub trait AskWorldExt {
    /// Opens the conversation with the agent `name`.
    fn talk_to(&mut self, name: &str);

    /// Selects `selected` and brings the Inspector forward.
    fn show(&mut self, selected: Selected);

    /// Gives the chief's command `text` to the hamlet `hamlet`.
    fn decree(&mut self, hamlet: &str, text: String);

    /// Brings `thing` to the middle of the view, and stays on it if
    /// `stay`.
    fn focus(&mut self, thing: Thing, stay: bool);

    /// Takes `place` in hand, to be set down with a click on the
    /// ground.
    fn carry(&mut self, place: Place);

    /// Asks before `deed` is done, with the names of the agents at
    /// work that it cuts short.
    fn question(&mut self, deed: Deed, working: Vec<String>);

    /// Shows the form a hamlet is founded with.
    fn found_hamlet(&mut self);

    /// Shows the card the chief's look is chosen on, unless it is
    /// shown already.
    fn change_look(&mut self);
}

impl AskWorldExt for World {
    fn talk_to(&mut self, name: &str) {
        self.agents(&AgentsRequest::Select(Selected::Agent(
            name.to_owned(),
        )));
        open(CHAT);
    }

    fn show(&mut self, selected: Selected) {
        self.agents(&AgentsRequest::Select(selected));
        open(INSPECTOR);
    }

    fn decree(&mut self, hamlet: &str, text: String) {
        self.agents(&AgentsRequest::Decree {
            hamlet: hamlet.to_owned(),
            text,
        });
    }

    fn focus(&mut self, thing: Thing, stay: bool) {
        let _ = self
            .run_system_cached_with(focus_on_thing, (thing, stay));
    }

    fn carry(&mut self, place: Place) {
        let _ = self.run_system_cached_with(pick_up, place);
    }

    fn question(&mut self, deed: Deed, working: Vec<String>) {
        self.spawn((
            Card::new(CardLook::Card, QUESTION_WIDTH, deed::view),
            CardPlace::Here,
            Asking { deed, working },
        ));
    }

    fn found_hamlet(&mut self) {
        self.spawn((
            Card::new(CardLook::Card, QUESTION_WIDTH, found::view),
            CardPlace::Here,
            Forming,
        ));
    }

    fn change_look(&mut self) {
        let shown = self
            .query_filtered::<(), With<Dressing>>()
            .iter(self)
            .next()
            .is_some();
        if !shown {
            self.spawn((
                Card::new(
                    CardLook::Card,
                    QUESTION_WIDTH,
                    wardrobe::view,
                ),
                CardPlace::Here,
                Dressing,
            ));
        }
    }
}

/// Opens the menu of what the person asked about with the right
/// button. A resident's menu waits for the skills it could be given.
fn ask(
    things: Things,
    mut picking: ResMut<Picking>,
    mut offering: ResMut<Offering>,
    mut commands: Commands,
    mut cues: MessageWriter<Cue>,
) {
    let Some(asked) = picking.take_ask() else {
        return;
    };
    let Some(thing) = asked else {
        commands.open_menu(Menus::ground());
        return;
    };
    let resident = match thing {
        Thing::Robot(agent) | Thing::House(agent) => things
            .bridge
            .agent(agent)
            .filter(|agent| !agent.mayor && !agent.gone),
        _ => None,
    };
    match resident {
        Some(agent) => {
            let ticket = commands
                .agents(&AgentsRequest::Offers(agent.name.clone()));
            offering.0 = Some((thing, ticket));
        }
        None => {
            let menus = Menus::new(&things.bridge, &picking, &[]);
            commands.open_menu(menus.of(thing));
            cues.write_batch(menu_cue(&things, thing));
        }
    }
}

/// Opens the menu of the resident asked about, given the skills it
/// could be given.
fn offered(
    mut replies: MessageReader<Replied>,
    things: Things,
    picking: Res<Picking>,
    mut offering: ResMut<Offering>,
    mut commands: Commands,
    mut cues: MessageWriter<Cue>,
) {
    for replied in replies.read() {
        let Some((thing, ticket)) = offering.0 else {
            return;
        };
        if replied.ticket != ticket {
            continue;
        }
        offering.0 = None;
        let gifts: &[Gift] = match &replied.reply {
            Ok(Reply::Agents(AgentsReply::Offers(gifts))) => gifts,
            _ => &[],
        };
        let menus = Menus::new(&things.bridge, &picking, gifts);
        commands.open_menu(menus.of(thing));
        cues.write_batch(menu_cue(&things, thing));
    }
}

/// The sound of a menu opening over `thing`.
fn menu_cue(things: &Things, thing: Thing) -> Option<Cue> {
    let at = things.place(thing)?;
    Some(Cue {
        sound: Sound::Menu,
        at: things.land.on_ground(at),
        turn: thing.key(),
    })
}

/// Takes down a question or a form that was closed without an
/// answer.
fn close(
    mut closed: MessageReader<CardClosed>,
    cards: Query<(), Asked>,
    mut commands: Commands,
) {
    for CardClosed(card) in closed.read() {
        if cards.contains(*card) {
            commands.entity(*card).despawn();
        }
    }
}

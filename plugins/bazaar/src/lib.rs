//! The Agent Bazaar: six stalls, each a tab of its panel, where the
//! chief adds models from OpenRouter and the endpoints, takes skills
//! from skills.sh, buys and sells agents, tools and hamlets at the
//! bazaar's server, hamlet-server, and buys the official looks agents
//! and hamlets wear there; where that cannot be reached, the stalls
//! of what chiefs sell show samples, and hamlets are founded from
//! their templates.

mod covers;
mod diorama;
mod errand;
mod hamlets;
mod listings;
mod market;
mod models;
mod picture;
mod samples;
mod shelf;
mod skills;
mod view;
mod wardrobe;

use agents_ui::{Heard, start};
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::open;
use hamlet_engine_bevy::{decode, encode, warn};
use serde::{Deserialize, Serialize};

use crate::listings::{AgentWares, HamletWares, ToolWares};
use crate::models::Ware;
use crate::samples::agents::Hireling;
use crate::samples::hamlets::Template;
use crate::samples::plugins::Contraption;
use crate::shelf::{Shelf, Stock};
use crate::skills::skills_sh::Scroll;
use crate::view::Shown;

/// The bazaar's panel.
const PANEL: &str = "bazaar";

/// The event the realm sends when a stall of its market is clicked,
/// with the stall's name.
const STALL_CLICKED: &str = "hamlet.realm/stall";

/// The event sent after a purchase, with the stall's name.
const TRADED: &str = "traded";

/// The commands that show a stall besides the Models', with their
/// labels.
const BROWSE: [(&str, &str, Stall); 4] = [
    ("agents", "Browse the Agents", Stall::Community),
    ("plugins", "Browse the Plugins", Stall::Plugins),
    ("skills", "Browse the Skills", Stall::Skills),
    ("hamlets", "Browse the Hamlets", Stall::Hamlets),
];

#[derive(Default)]
struct Bazaar;

impl Plugin for Bazaar {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            agents_ui::plugin,
            diorama::plugin,
            errand::plugin,
            models::plugin,
            skills::plugin,
            samples::plugin,
            hamlets::plugin,
            market::plugin,
            covers::plugin,
            wardrobe::plugin,
            view::plugin,
        ))
        .init_saved_resource::<StallShown>()
        .add_panel(PANEL, "Agent Bazaar", view::bazaar)
        .panel_icon("coins")
        .add_command(
            "open",
            "Open the Agent Bazaar",
            |world: &mut World| {
                open(PANEL);
                let stall = world.resource::<StallShown>().0;
                world.stocked(stall);
            },
        )
        .add_command(
            "models",
            "Browse the Models",
            |world: &mut World| {
                models::browse(world);
                world.show(Stall::Models);
            },
        )
        .add_command(
            "looks",
            "Browse the Looks",
            |world: &mut World| {
                world.resource_mut::<wardrobe::Fitting>().on = false;
                world.show(Stall::Assets);
            },
        )
        .add_command(
            "sell",
            "Sell at the Agent Bazaar",
            |world: &mut World| {
                let stall = match world.resource::<StallShown>().0 {
                    stall @ (Stall::Community
                    | Stall::Plugins
                    | Stall::Hamlets) => stall,
                    _ => Stall::Community,
                };
                world.show(stall);
                view::sell(world, stall);
            },
        )
        .add_menu_entry("Hamlet", "open")
        .add_menu_entry("Hamlet", "models")
        .add_menu_entry("Hamlet", "hamlets")
        .add_menu_entry("Hamlet", "looks")
        .add_menu_entry("Hamlet", "sell")
        .add_subscription(STALL_CLICKED)
        .add_subscription(looks::DRESS)
        .add_systems(
            Declare,
            start(&[
                AgentsRequest::Settings,
                AgentsRequest::Catalogue,
                AgentsRequest::Skills,
                AgentsRequest::Roster,
                AgentsRequest::Limits,
            ]),
        )
        .add_systems(Update, receive);
        for (name, label, stall) in BROWSE {
            app.add_command(name, label, move |world: &mut World| {
                world.show(stall);
            });
        }
    }
}

hamlet_engine_bevy::export!(Bazaar);

/// A stall of the bazaar.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub(crate) enum Stall {
    #[default]
    Models,
    Community,
    Plugins,
    Skills,
    Hamlets,
    /// The official looks agents and hamlets wear.
    Assets,
}

impl Stall {
    pub const ALL: [Stall; 6] = [
        Stall::Models,
        Stall::Community,
        Stall::Plugins,
        Stall::Skills,
        Stall::Hamlets,
        Stall::Assets,
    ];

    /// Its name in the events about it.
    pub fn name(self) -> &'static str {
        match self {
            Stall::Models => "models",
            Stall::Community => "community",
            Stall::Plugins => "plugins",
            Stall::Skills => "skills",
            Stall::Hamlets => "hamlets",
            Stall::Assets => "assets",
        }
    }
}

/// The stall the panel shows, kept across a reload.
#[derive(Resource, Default, Serialize, Deserialize)]
pub(crate) struct StallShown(pub Stall);

/// The bazaar's methods on [`World`].
pub(crate) trait BazaarWorldExt {
    /// Shows `stall` in the panel and brings the panel to the front.
    fn show(&mut self, stall: Stall);

    /// Asks for what `stall` sells the first time it is shown, and
    /// for the page open there what that page shows.
    fn stocked(&mut self, stall: Stall);

    /// Tells whoever listens that something was bought at `stall`.
    fn traded(&mut self, stall: Stall);
}

impl BazaarWorldExt for World {
    fn show(&mut self, stall: Stall) {
        if self.resource::<StallShown>().0 != stall {
            self.resource_mut::<StallShown>().0 = stall;
        }
        self.stocked(stall);
        open(PANEL);
    }

    fn stocked(&mut self, stall: Stall) {
        match stall {
            Stall::Models => {
                if !self.resource::<Stock<Ware>>().asked {
                    models::fetch_wares(self);
                }
                reopen::<Ware>(self);
            }
            Stall::Skills => {
                if !self.resource::<Stock<Scroll>>().asked {
                    skills::fetch_board(self);
                }
                reopen::<Scroll>(self);
            }
            Stall::Community => {
                market::stocked(self);
                reopen::<Hireling>(self);
                reopen::<AgentWares>(self);
            }
            Stall::Plugins => {
                market::stocked(self);
                reopen::<Contraption>(self);
                reopen::<ToolWares>(self);
            }
            Stall::Hamlets => {
                market::stocked(self);
                reopen::<Template>(self);
                reopen::<HamletWares>(self);
            }
            Stall::Assets => wardrobe::stocked(self),
        }
    }

    fn traded(&mut self, stall: Stall) {
        if let Ok(payload) = encode(&stall.name().to_string()) {
            self.emit(TRADED, &payload);
        }
    }
}

/// Asks again for what the page open at the stall of `L` shows, as
/// after a reload.
fn reopen<L: Shown>(world: &mut World) {
    if let Some(id) = world.resource::<Shelf<L>>().open.clone() {
        L::opened(world, &id);
    }
}

/// Takes in what the agents service says, the stalls clicked in the
/// realm, and the looks the Inspector asks to show.
fn receive(
    world: &mut World,
    mut heard: Local<MessageCursor<Heard>>,
    mut events: Local<MessageCursor<EventReceived>>,
) {
    let told = heard
        .read(world.resource::<Messages<Heard>>())
        .cloned()
        .collect::<Vec<_>>();
    for told in told {
        models::heard(world, &told);
        skills::heard(world, &told);
        hamlets::heard(world, &told);
        market::heard(world, &told);
    }
    let events = events
        .read(world.resource::<Messages<EventReceived>>())
        .cloned()
        .collect::<Vec<_>>();
    for event in events
        .iter()
        .filter(|event| event.event.as_str() == looks::DRESS)
    {
        match decode::<looks::Dress>(&event.payload) {
            Ok(dress) => wardrobe::dress(world, dress),
            Err(why) => warn(&format!("looks to show: {why}")),
        }
    }
    let clicked = events
        .iter()
        .filter(|event| event.event.as_str() == STALL_CLICKED)
        .map(|event| decode::<String>(&event.payload))
        .collect::<Vec<_>>();
    for name in clicked {
        let stall = name.ok().and_then(|name| {
            Stall::ALL.into_iter().find(|stall| stall.name() == name)
        });
        match stall {
            Some(stall) => world.show(stall),
            None => warn("a stall of no name the bazaar knows"),
        }
    }
}

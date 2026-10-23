//! The card that follows the pointer over the realm and says what
//! it rests on: how an agent is doing, a hamlet, the chief's place,
//! a workbench or its clockwork.

use bevy_ecs::system::SystemParam;
use hamlet_engine_bevy::api::agents::RosterHamlet;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    Face, Size, Tone, ViewExt, column, label, resource,
};

use crate::bridge::{Bridge, Mode};
use crate::cards::WordsExt;
use crate::folk::chief::{Company, Keeping};
use crate::folk::keeper::Robots;
use crate::realm::{ChiefFocus, Clock};
use crate::society::TownId;
use crate::tuning::WorldTuning;
use crate::view::camera::Following;
use crate::view::pick::{Picking, Thing};
use crate::world::land::Land;

/// The widest the card is drawn, in pixels.
const WIDEST: f32 = 280.0;

/// The most of an agent's purpose the card shows, in letters.
const PURPOSE_SHOWN: usize = 96;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Hover>()
        .init_resource::<Resting>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            describe.run_if(resource_exists::<Land>),
        );
}

/// The words of the card.
#[derive(Resource, Clone, PartialEq)]
struct Hover {
    title: String,
    state: String,
    tone: Tone,
    purpose: String,
    account: String,
    hint: String,
}

impl Default for Hover {
    fn default() -> Self {
        Self {
            title: String::new(),
            state: String::new(),
            tone: Tone::Body,
            purpose: String::new(),
            account: String::new(),
            hint: String::new(),
        }
    }
}

/// On the card that follows the pointer.
#[derive(Component)]
struct HoverCard;

/// The thing the pointer rests on, and the time it came to rest.
#[derive(Resource, Default)]
struct Resting {
    on: Option<Thing>,
    since: f32,
}

fn spawn(mut commands: Commands) {
    commands.spawn((
        Card::new(CardLook::Card, WIDEST, view),
        CardPlace::Hidden,
        HoverCard,
    ));
}

/// A line of the card that takes no room while it is empty.
fn line(read: fn(&Hover) -> &String, tone: Tone) -> AnyView {
    label(resource::<Hover, _>(move |hover| read(hover).clone()))
        .size(Size::Small)
        .tone(tone)
        .boxed()
}

fn view(_: Entity) -> AnyView {
    column((
        label(resource::<Hover, _>(|hover| hover.title.clone()))
            .bold(true)
            .face(Face::Display),
        label(resource::<Hover, _>(|hover| hover.state.clone()))
            .size(Size::Small)
            .tone(resource::<Hover, _>(|hover| hover.tone)),
        line(|hover| &hover.purpose, Tone::Body),
        line(|hover| &hover.account, Tone::Dim),
        line(|hover| &hover.hint, Tone::Dim),
    ))
    .boxed()
}

/// What the card is made from.
#[derive(SystemParam)]
struct Facts<'w> {
    bridge: Res<'w, Bridge>,
    robots: Res<'w, Robots>,
    following: Res<'w, Following>,
    keeping: Res<'w, Keeping>,
    focus: Res<'w, ChiefFocus>,
    clock: Res<'w, Clock>,
}

impl Facts<'_> {
    /// The card of `thing`, if it has one.
    fn card(&self, thing: Thing) -> Option<Hover> {
        match thing {
            Thing::Robot(_) | Thing::House(_) => self.of_agent(thing),
            Thing::Common(town) => self.of_hamlet(town),
            Thing::Clockwork(town) => self.of_clockwork(town),
            Thing::Workbench(town) => self.of_workbench(town),
            Thing::Mascot | Thing::Hearth => {
                Some(self.of_chief(thing == Thing::Hearth))
            }
            Thing::Library => None,
        }
    }

    /// The card of a whole hamlet, over its common.
    fn of_hamlet(&self, town: TownId) -> Option<Hover> {
        let hamlet = self.bridge.record(town)?;
        let (state, tone) = standing(hamlet);
        Some(Hover {
            title: hamlet.title.to_uppercase(),
            state,
            tone,
            purpose: format!("Mayor: {}", hamlet.mayor),
            account: hamlet.account.clone(),
            hint: "Click its hall to talk to the mayor".to_owned(),
        })
    }

    /// The card of a hamlet's clockwork.
    fn of_clockwork(&self, town: TownId) -> Option<Hover> {
        let hamlet = self.bridge.record(town)?;
        let (state, tone, purpose, hint) =
            match self.bridge.mode(town) {
                None => (
                    "No folder",
                    Tone::Dim,
                    "It turns by itself in auto, once this hamlet \
                     works on a folder."
                        .to_owned(),
                    "Click for the hamlet's page",
                ),
                Some(Mode::Manual) => (
                    "Manual",
                    Tone::Body,
                    hamlet.mode.clone(),
                    "Click to switch to auto",
                ),
                Some(Mode::Auto) => (
                    "Auto",
                    Tone::Accent,
                    hamlet.mode.clone(),
                    "Click to switch to manual",
                ),
            };
        Some(Hover {
            title: format!("CLOCKWORK OF {}", hamlet.title)
                .to_uppercase(),
            state: state.to_owned(),
            tone,
            purpose,
            hint: hint.to_owned(),
            ..Hover::default()
        })
    }

    /// The card of a hamlet's workbench.
    fn of_workbench(&self, town: TownId) -> Option<Hover> {
        let hamlet = self.bridge.record(town)?;
        let (state, tone, purpose) = match &hamlet.folder {
            Some(folder) => (
                folder.clone(),
                Tone::Body,
                "The folder this hamlet works on.",
            ),
            None => (
                "No folder".to_owned(),
                Tone::Dim,
                "Its work is laid out when this hamlet works on a \
                 folder.",
            ),
        };
        Some(Hover {
            title: format!("WORKBENCH OF {}", hamlet.title)
                .to_uppercase(),
            state,
            tone,
            purpose: purpose.to_owned(),
            hint: "Click for the hamlet's page".to_owned(),
            ..Hover::default()
        })
    }

    /// The card of the chief's mascot, or of its place.
    fn of_chief(&self, place: bool) -> Hover {
        let name = self.focus.chief.to_uppercase();
        let with = match self.keeping.with {
            Some(Company::Agent(agent)) => {
                self.bridge.named(agent).map(|(name, _)| name.clone())
            }
            _ => None,
        };
        let (state, tone) = match with {
            Some(agent) => (format!("With {agent}"), Tone::Accent),
            None => ("At home".to_owned(), Tone::Dim),
        };
        Hover {
            title: if place {
                format!("{name}'S HEARTH")
            } else {
                name
            },
            state,
            tone,
            purpose: if place {
                "Where the chief lives.".to_owned()
            } else {
                "The chief, who goes by Wickstep.".to_owned()
            },
            hint: "Click to change the look".to_owned(),
            ..Hover::default()
        }
    }

    /// The card of an agent, or of what is left of it.
    fn of_agent(&self, thing: Thing) -> Option<Hover> {
        let (Thing::Robot(id) | Thing::House(id)) = thing else {
            return None;
        };
        let agent = self.bridge.agent(id)?;
        let state = if let Some(host) = &agent.merged_into {
            (format!("Merged into {host}"), Tone::Dim)
        } else if agent.gone {
            ("Removed".to_owned(), Tone::Critical)
        } else if agent.waiting > 0 {
            (
                format!("Waits for you ({})", agent.waiting),
                Tone::Critical,
            )
        } else {
            let tone =
                if agent.busy { Tone::Accent } else { Tone::Dim };
            (agent.state.clone(), tone)
        };
        let followed = self.following.0 == Some(thing);
        let robot = matches!(thing, Thing::Robot(_));
        let proud = self
            .robots
            .robot(id)
            .is_some_and(|robot| robot.mood.proud(self.clock.now));
        let hint = if agent.gone {
            "Right-click to restore"
        } else if followed {
            "Followed. Esc lets go"
        } else if robot && proud {
            "Its work is done: click to cheer it"
        } else if robot {
            "Click to talk, drag to pick up, right-click for more"
        } else {
            "Click to talk, F to focus, right-click for more"
        };
        Some(Hover {
            title: agent.name.to_uppercase(),
            state: state.0,
            tone: state.1,
            purpose: agent.purpose.short(PURPOSE_SHOWN),
            account: agent.account.clone(),
            hint: hint.to_owned(),
        })
    }
}

/// The state of a hamlet in a word, as its record counts its agents.
fn standing(hamlet: &RosterHamlet) -> (String, Tone) {
    let count = match hamlet.living {
        1 => "1 agent".to_owned(),
        n => format!("{n} agents"),
    };
    match (hamlet.waiting, hamlet.working) {
        (0, 0) => (format!("{count}, all at rest"), Tone::Dim),
        (0, n) => (format!("{count}, {n} at work"), Tone::Accent),
        (w, _) => {
            (format!("{count}, {w} waiting for you"), Tone::Critical)
        }
    }
}

/// Fills the card from what the pointer has rested on for a while,
/// and shows it beside the pointer. There is no card while a menu is
/// open over the realm.
fn describe(
    picking: Res<Picking>,
    tuning: Res<WorldTuning>,
    menu: Res<MenuShown>,
    facts: Facts,
    mut resting: ResMut<Resting>,
    mut hover: ResMut<Hover>,
    mut cards: Query<&mut CardPlace, With<HoverCard>>,
) {
    let now = facts.clock.now;
    let on = picking.hovered();
    if resting.on != on {
        resting.on = on;
        resting.since = now;
    }
    let rested = now - resting.since >= tuning.hover_wait;
    let card = on
        .filter(|_| rested && !menu.0)
        .and_then(|thing| facts.card(thing));
    let place = match card {
        Some(card) => {
            if *hover != card {
                *hover = card;
            }
            CardPlace::Pointer
        }
        None => CardPlace::Hidden,
    };
    for mut at in &mut cards {
        at.set_if_neq(place);
    }
}

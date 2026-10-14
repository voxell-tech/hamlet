//! The card that follows the pointer over the world and says how
//! the agent under it is doing.

use bevy::app::{App, Update};
use bevy::color::Alpha;
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Has, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{
    Commands, Local, Query, Res, ResMut, SystemParam,
};
use bevy::math::Vec2;
use bevy::picking::Pickable;
use bevy::text::TextColor;
use bevy::time::Time;
use bevy::ui::{
    BackgroundColor, BorderColor, ComputedNode, Node, OverrideClip,
    PositionType, UiRect, UiTransform, Val, px,
};
use bevy::window::{PrimaryWindow, Window};
use bevy_fynix::resource;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, overlay,
};
use hamlet_core::agents::{
    Account, Activity, Busy, Citizen, Left, LivesIn, Mayor,
    MergedInto, Purpose, Roster, Treasury, Works,
};
use hamlet_core::oversight::Waiting;
use hamlet_rules::work::Mode;
use hamlet_world::chief::{Beside, Look};
use hamlet_world::hand::Proud;
use hamlet_world::pick::{Following, Hovered, Thing};

use crate::feel::FeelSettings;
use crate::inspector::now;
use crate::overview::standing;
use crate::platform::{DisplayText, show};
use crate::roll::Roll;
use crate::theme::Themed;
use crate::world_menu::Open;
use crate::{AnyView, Cx, View};

/// The layer of the card in the window's stack: over the panels,
/// under the menus.
const CARD_Z: i32 = 950;

/// The most of an agent's purpose the card shows, in letters.
pub(crate) const PURPOSE_SHOWN: usize = 96;

/// The state of an agent, for its card.
type Doing<'a> = (
    &'a Name,
    (Has<Left>, Has<Busy>, Has<Proud>),
    Option<&'a MergedInto>,
    Option<&'a Activity>,
    Option<&'a Purpose>,
    Option<&'a Account>,
);

/// An agent that lives in a hamlet, that it is at work and that it is
/// the mayor.
type Living<'a> = (&'a Name, &'a LivesIn, Has<Busy>, Has<Mayor>);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Card>()
        .init_resource::<Resting>()
        .add_systems(Update, (describe, place).chain());
}

/// The words of the card. It keeps them after `shown` is false, to go
/// out with them.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Card {
    shown: bool,
    title: String,
    state: String,
    tone: Tone,
    purpose: String,
    account: String,
    hint: String,
}

impl Card {
    pub(crate) fn shown(&self) -> bool {
        self.shown
    }
}

/// The card's node.
#[derive(Component)]
pub(crate) struct CardBox;

/// Words cut to `most` letters at a word's end.
pub(crate) fn short(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(most)
        .map_or(text.len(), |(at, _)| at);
    let kept = &text[..cut];
    let kept = kept.rsplit_once(' ').map_or(kept, |(kept, _)| kept);
    format!("{kept}...")
}

/// The queries a card is made from.
#[derive(SystemParam)]
pub(crate) struct Told<'w, 's> {
    roster: Roster<'w, 's>,
    names: Query<'w, 's, &'static Name>,
    agents: Query<'w, 's, Doing<'static>, With<Citizen>>,
    living: Query<
        'w,
        's,
        Living<'static>,
        (With<Citizen>, Without<Left>),
    >,
    waits: Query<'w, 's, &'static Waiting>,
    spent: Query<'w, 's, &'static Treasury>,
    works: Query<'w, 's, &'static Works>,
    chiefs: Query<'w, 's, (Ref<'static, Look>, Ref<'static, Beside>)>,
    following: Res<'w, Following>,
}

impl Told<'_, '_> {
    fn name(&self, of: Entity) -> String {
        self.names
            .get(of)
            .map(|name| name.as_str().to_string())
            .unwrap_or_default()
    }

    /// The agent a thing stands for, and that the thing is its ruin.
    fn agent_of(&self, thing: Thing) -> Option<(Entity, bool)> {
        match thing {
            Thing::Keeper | Thing::Cottage => {
                self.roster.mayor().map(|mayor| (mayor, false))
            }
            Thing::Resident(agent) | Thing::ResidentHouse(agent) => {
                Some((agent, false))
            }
            Thing::Ruin(agent) => Some((agent, true)),
            Thing::Common(_)
            | Thing::Chief(_)
            | Thing::ChiefPlace(_)
            | Thing::Library
            | Thing::Clockwork(_)
            | Thing::Workbench(_) => None,
        }
    }

    /// The agent of `thing` has work to be cheered for.
    fn proud(&self, thing: Thing) -> bool {
        self.agent_of(thing)
            .and_then(|(agent, _)| self.agents.get(agent).ok())
            .is_some_and(|(_, (_, _, proud), ..)| proud)
    }

    /// The look or the company of the chief of `thing` changed.
    fn chief_moved(&self, thing: Thing) -> bool {
        let (Thing::Chief(chief) | Thing::ChiefPlace(chief)) = thing
        else {
            return false;
        };
        self.chiefs.get(chief).is_ok_and(|(look, beside)| {
            look.is_changed() || beside.is_changed()
        })
    }

    /// The card of a whole hamlet, over its common.
    fn of_hamlet(&self, hamlet: Entity) -> Card {
        let people = self
            .living
            .iter()
            .filter(|(_, lives, ..)| lives.0 == hamlet)
            .map(|(name, _, busy, mayor)| {
                (name.as_str(), busy, mayor)
            })
            .collect::<Vec<_>>();
        let at_work =
            people.iter().filter(|(_, busy, _)| *busy).count();
        let waiting = self
            .waits
            .iter()
            .filter(|w| people.iter().any(|(n, ..)| *n == w.by))
            .count();
        let (_, state, tone) =
            standing(people.len(), at_work, waiting);
        let mayor = people
            .iter()
            .find(|(_, _, mayor)| *mayor)
            .map(|(name, ..)| format!("Mayor: {name}"))
            .unwrap_or_default();
        Card {
            shown: true,
            title: self.name(hamlet).to_uppercase(),
            state,
            tone,
            purpose: mayor,
            account: self
                .spent
                .get(hamlet)
                .map(|t| t.0.summary())
                .unwrap_or_default(),
            hint: "Click its hall to talk to the mayor".to_string(),
        }
    }

    /// The card of a hamlet's clockwork.
    fn of_clockwork(&self, hamlet: Entity) -> Card {
        let mode = self
            .works
            .get(hamlet)
            .ok()
            .filter(|works| works.0.folder.is_some())
            .map(|works| works.0.mode);
        let (state, tone, purpose, hint) = match mode {
            None => (
                "No folder",
                Tone::Dim,
                "It turns by itself in auto, once this hamlet works \
                 on a folder.",
                "Click for the hamlet's page",
            ),
            Some(Mode::Manual) => (
                "Manual",
                Tone::Body,
                Mode::Manual.says(),
                "Click to switch to auto",
            ),
            Some(Mode::Auto) => (
                "Auto",
                Tone::Accent,
                Mode::Auto.says(),
                "Click to switch to manual",
            ),
        };
        Card {
            shown: true,
            title: format!("CLOCKWORK OF {}", self.name(hamlet))
                .to_uppercase(),
            state: state.to_string(),
            tone,
            purpose: purpose.to_string(),
            account: String::new(),
            hint: hint.to_string(),
        }
    }

    /// The card of a hamlet's workbench.
    fn of_workbench(&self, hamlet: Entity) -> Card {
        let folder = self
            .works
            .get(hamlet)
            .ok()
            .and_then(|works| works.0.folder.as_deref());
        let (state, tone, purpose) = match folder {
            Some(folder) => (
                folder.display().to_string(),
                Tone::Body,
                "The folder this hamlet works on.",
            ),
            None => (
                "No folder".to_string(),
                Tone::Dim,
                "Its work is laid out when this hamlet works on a \
                 folder.",
            ),
        };
        Card {
            shown: true,
            title: format!("WORKBENCH OF {}", self.name(hamlet))
                .to_uppercase(),
            state,
            tone,
            purpose: purpose.to_string(),
            account: String::new(),
            hint: "Click for the hamlet's page".to_string(),
        }
    }

    /// The card of a chief's mascot, or of its place.
    fn of_chief(&self, chief: Entity, place: bool) -> Option<Card> {
        let name =
            self.names.get(chief).ok()?.as_str().to_uppercase();
        let (look, beside) = self.chiefs.get(chief).ok()?;
        let with =
            beside.0.and_then(|agent| self.names.get(agent).ok());
        let (state, tone) = match with {
            Some(agent) => (format!("With {agent}"), Tone::Accent),
            None => ("At home".to_string(), Tone::Dim),
        };
        Some(Card {
            shown: true,
            title: if place {
                format!("{name}'S HEARTH")
            } else {
                name
            },
            state,
            tone,
            purpose: if place {
                "Where the chief lives.".to_string()
            } else {
                "The chief, who goes by Wickstep.".to_string()
            },
            account: look.words(),
            hint: "Click to change the look".to_string(),
        })
    }

    /// The card of an agent, or of what is left of it.
    fn of_agent(
        &self,
        agent: Entity,
        ruin: bool,
        hovered: Thing,
    ) -> Option<Card> {
        let (
            name,
            (gone, busy, proud),
            into,
            activity,
            purpose,
            account,
        ) = self.agents.get(agent).ok()?;
        let name = name.as_str();
        let waiting =
            self.waits.iter().filter(|w| w.by == name).count();
        let (state, tone) = match into {
            Some(host) if gone => (
                format!("Merged into {}", self.name(host.0)),
                Tone::Dim,
            ),
            _ if gone || ruin => {
                ("Removed".to_string(), Tone::Critical)
            }
            _ if waiting > 0 => {
                (format!("Waits for you ({waiting})"), Tone::Critical)
            }
            _ => {
                let tone =
                    if busy { Tone::Accent } else { Tone::Dim };
                (
                    now(busy, activity.copied().unwrap_or_default()),
                    tone,
                )
            }
        };
        let followed = self.following.0 == Some(hovered);
        let robot =
            matches!(hovered, Thing::Keeper | Thing::Resident(_));
        Some(Card {
            shown: true,
            title: name.to_uppercase(),
            state,
            tone,
            purpose: purpose
                .map(|p| short(&p.0, PURPOSE_SHOWN))
                .unwrap_or_default(),
            account: account
                .map(|a| a.0.summary())
                .unwrap_or_default(),
            hint: match (gone || ruin, followed) {
                (true, _) => "Right-click to restore".to_string(),
                (false, true) => "Followed. Esc lets go".to_string(),
                (false, false) if robot && proud => {
                    "Its work is done: click to cheer it".to_string()
                }
                (false, false) if robot => "Click to talk, drag to \
                                            pick up, right-click \
                                            for more"
                    .to_string(),
                (false, false) => "Click to talk, F to focus, \
                                   right-click for more"
                    .to_string(),
            },
        })
    }

    /// The card of `thing`, if it has one.
    fn card(&self, thing: Thing) -> Option<Card> {
        match thing {
            Thing::Common(hamlet) => Some(self.of_hamlet(hamlet)),
            Thing::Clockwork(hamlet) => {
                Some(self.of_clockwork(hamlet))
            }
            Thing::Workbench(hamlet) => {
                Some(self.of_workbench(hamlet))
            }
            Thing::Chief(chief) => self.of_chief(chief, false),
            Thing::ChiefPlace(chief) => self.of_chief(chief, true),
            Thing::Keeper
            | Thing::Cottage
            | Thing::Resident(_)
            | Thing::ResidentHouse(_)
            | Thing::Ruin(_) => {
                let (agent, ruin) = self.agent_of(thing)?;
                self.of_agent(agent, ruin, thing)
            }
            Thing::Library => None,
        }
    }
}

/// The thing the pointer rests on, and the time it came to rest.
#[derive(Resource, Default)]
pub(crate) struct Resting {
    on: Option<Thing>,
    since: f32,
}

/// Fills the card from what the pointer has rested on for a while,
/// and again when that changed. There is no card while a menu is
/// open over the world.
#[allow(clippy::too_many_arguments)]
pub(crate) fn describe(
    hovered: Res<Hovered>,
    time: Res<Time>,
    feel: Res<FeelSettings>,
    open: Res<Open>,
    roll: Res<Roll>,
    told: Told,
    mut resting: ResMut<Resting>,
    mut card: ResMut<Card>,
    mut made: Local<Option<(Thing, bool)>>,
) {
    let clock = time.elapsed_secs();
    if resting.on != hovered.0 {
        resting.on = hovered.0;
        resting.since = clock;
    }
    let rested = clock - resting.since >= feel.hover_wait;
    let menu = *open != Open::Nothing;
    let Some(thing) = hovered.0.filter(|_| rested && !menu) else {
        *made = None;
        // It goes out with the words it had.
        if card.shown {
            card.shown = false;
        }
        return;
    };
    let now = Some((thing, told.proud(thing)));
    let moved = *made != now
        || roll.is_changed()
        || told.following.is_changed()
        || told.chief_moved(thing);
    if !moved {
        return;
    }
    *made = now;
    match told.card(thing) {
        Some(next) => {
            if *card != next {
                *card = next;
            }
        }
        None => {
            if card.shown {
                card.shown = false;
            }
        }
    }
}

/// Fades the card in with a small growth when it has something to
/// say and out again after, keeps it by the pointer and inside the
/// window while it is shown, and out of the pointer's way.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn place(
    mut commands: Commands,
    theme: Themed,
    card: Res<Card>,
    time: Res<Time>,
    set: Res<FeelSettings>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut boxes: Query<
        (
            Entity,
            &mut Node,
            &ComputedNode,
            &mut BackgroundColor,
            &mut BorderColor,
            Has<Pickable>,
            Option<&mut UiTransform>,
        ),
        With<CardBox>,
    >,
    under: Query<&Children>,
    mut inks: Query<&mut TextColor>,
    mut shown: Local<f32>,
) {
    let Some(window) = window.iter().next() else {
        return;
    };
    let pointer = window.cursor_position();
    let wanted = card.shown && pointer.is_some();
    let step = time.delta_secs()
        / if wanted { set.hover_in } else { set.hover_out };
    *shown = if wanted {
        (*shown + step).min(1.0)
    } else {
        (*shown - step).max(0.0)
    };
    let eased = *shown * *shown * (3.0 - 2.0 * *shown);
    for (
        entity,
        mut node,
        computed,
        mut fill,
        mut edge,
        passes,
        grown,
    ) in &mut boxes
    {
        // The pointer must reach the world through the card, or the
        // card would end the very hover that shows it.
        if !passes {
            commands.entity(entity).try_insert(Pickable::IGNORE);
            for child in under.iter_descendants(entity) {
                commands.entity(child).try_insert(Pickable::IGNORE);
            }
        }
        show(&mut node, *shown > 0.0);
        // It fades, and grows the last little way to its size.
        let pose = UiTransform::from_scale(Vec2::splat(
            set.hover_from + (1.0 - set.hover_from) * eased,
        ));
        match grown {
            Some(mut grown) if *grown != pose => *grown = pose,
            Some(_) => {}
            None => {
                commands.entity(entity).try_insert(pose);
            }
        }
        if fill.0.alpha() != eased {
            fill.0.set_alpha(eased);
            *edge = BorderColor::all(
                theme.palette.brass_5.with_alpha(eased),
            );
        }
        for child in under.iter_descendants(entity) {
            if let Ok(mut ink) = inks.get_mut(child)
                && ink.0.alpha() != eased
            {
                ink.0.set_alpha(eased);
            }
        }
        // It stays where it was while it goes.
        let Some(at) = pointer.filter(|_| wanted) else {
            continue;
        };
        let full = computed.size() * computed.inverse_scale_factor();
        let room = (window.width(), window.height());
        // Below and right of the pointer, or above and left where
        // the window ends.
        let left = if at.x + 18.0 + full.x > room.0 {
            at.x - 12.0 - full.x
        } else {
            at.x + 18.0
        };
        let top = if at.y + 20.0 + full.y > room.1 {
            at.y - 12.0 - full.y
        } else {
            at.y + 20.0
        };
        let (left, top) = (px(left.max(4.0)), px(top.max(4.0)));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
}

/// A line of the card that takes no room while it is empty.
fn line(read: fn(&Card) -> &String, tone: Tone) -> impl View {
    label(resource::<Card, _>(move |card| read(card).clone()))
        .size(12.0)
        .tone(tone)
}

/// The card, on the window's top layer. It is built once.
pub fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let shadow = cx.theme().shadow();
        let card = column((
            label(resource::<Card, _>(|card| card.title.clone()))
                .size(13.0)
                .bold(true)
                .wrap(false)
                .tagged(DisplayText),
            label(resource::<Card, _>(|card| card.state.clone()))
                .size(12.0)
                .tone(resource::<Card, _>(|card| card.tone)),
            line(|card| &card.purpose, Tone::Body),
            line(|card| &card.account, Tone::Dim),
            line(|card| &card.hint, Tone::Dim),
        ))
        .gap(3.0)
        .padding(UiRect::axes(px(12.0), px(9.0)))
        .max_width(px(280.0))
        .border(1.0)
        .border_color(cx.theme().palette.brass_5)
        .radius(cx.theme().space.soft)
        .fill(cx.theme().palette.card)
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(0.0),
            top: px(0.0),
            right: Val::Auto,
            bottom: Val::Auto,
        })
        .z(Some(CARD_Z))
        .with((OverrideClip, CardBox, shadow));
        cx.build(overlay((card,)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_purpose_is_cut_at_a_word() {
        assert_eq!(short("Keeps the list.", 40), "Keeps the list.");
        assert_eq!(
            short("Reads every paper it is given", 12),
            "Reads every..."
        );
    }
}

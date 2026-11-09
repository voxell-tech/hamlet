//! The popup an agent's model is chosen in: the chief's favourites
//! and the providers down its side, the models of the one chosen
//! with a search over every provider, the efforts the model can
//! think at, and its fast tier when it has one. It hangs over the
//! chip in the composer that opens it.

use std::cmp::Reverse;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonState;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::math::Rect;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, Invoke, Layer, Mods, Scope,
    ScopeId, ScopeSpec, ShortcutAppExt,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, menu_item, overlay, popup, row,
};
use bevy_fynix::{
    Focused, ScopedExt, ViewExt, derived, every_frame, keyed, style,
};
use hamlet_client::realm::{
    Account, Bylaws, Fast, Kind, LivesIn, OwnEffort, Settings,
    name_of,
};
use hamlet_protocol::{Effort, client as protocol};
use hamlet_ui::styles::{
    ICON, description, hairline, note, radio, rail_item, small_rules,
    upright,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::{AnyView, Cx, View, with_cx};
use serde::{Deserialize, Serialize};

use super::{Open, close};
use crate::menu::rect_of;
use crate::models::{
    Detail, ModelRow, Shelf, legend, mark, model_name, model_row,
    model_title, over_rows, provider_name, provider_of, tag,
    title_of,
};

/// The width of the providers down the side.
const RAIL: f32 = 108.0;

/// The least width of the search and its models, which take what the
/// popup has over the rest.
const LIST: f32 = 230.0;

/// The width of the efforts and the tier.
const LEVELS: f32 = 150.0;

/// The room between the popup's parts, and about them.
const GAP: f32 = 10.0;
const PADDING: f32 = 14.0;

/// The popup's height, whatever it shows, so nothing in it moves
/// under the pointer as it changes.
const TALL: f32 = 354.0;

/// The scope of the popup's keys, over the desks' own.
const MODELS: ScopeId = ScopeId("models");

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Picking>()
        .init_resource::<Favorites>()
        .init_resource::<LastSide>()
        .add_scope(ScopeSpec {
            id: MODELS,
            label: "Models",
            layer: Layer::Modal,
            active: None,
        })
        .add_observer(leave_search)
        .add_systems(Update, take_focus);
    for (id, label, key, run) in PICKED_BY_DIGIT {
        app.add_command(
            CommandSpec {
                id: CommandId(id),
                label,
                scope: MODELS,
                run,
                enabled: |_| true,
                repeat: false,
            },
            &[Chord {
                key,
                mods: Mods::PRIMARY,
            }],
        );
    }
    for (id, label, key, run) in STEPS {
        app.add_command(
            CommandSpec {
                id: CommandId(id),
                label,
                scope: MODELS,
                run,
                enabled: |_| true,
                repeat: false,
            },
            &[Chord::key(key)],
        );
    }
}

/// The keys that pick the first nine favourites, with the primary
/// key, whatever the list shows.
const PICKED_BY_DIGIT: [(
    &str,
    &str,
    KeyCode,
    fn(&mut World, Invoke),
); 9] = [
    (
        "models.pick_1",
        "Favourite 1",
        KeyCode::Digit1,
        pick_at::<0>,
    ),
    (
        "models.pick_2",
        "Favourite 2",
        KeyCode::Digit2,
        pick_at::<1>,
    ),
    (
        "models.pick_3",
        "Favourite 3",
        KeyCode::Digit3,
        pick_at::<2>,
    ),
    (
        "models.pick_4",
        "Favourite 4",
        KeyCode::Digit4,
        pick_at::<3>,
    ),
    (
        "models.pick_5",
        "Favourite 5",
        KeyCode::Digit5,
        pick_at::<4>,
    ),
    (
        "models.pick_6",
        "Favourite 6",
        KeyCode::Digit6,
        pick_at::<5>,
    ),
    (
        "models.pick_7",
        "Favourite 7",
        KeyCode::Digit7,
        pick_at::<6>,
    ),
    (
        "models.pick_8",
        "Favourite 8",
        KeyCode::Digit8,
        pick_at::<7>,
    ),
    (
        "models.pick_9",
        "Favourite 9",
        KeyCode::Digit9,
        pick_at::<8>,
    ),
];

/// The arrows that move the focus between the popup's parts.
const STEPS: [(&str, &str, KeyCode, fn(&mut World, Invoke)); 2] = [
    (
        "models.left",
        "The part to the left",
        KeyCode::ArrowLeft,
        |world, _| step(world, -1),
    ),
    (
        "models.right",
        "The part to the right",
        KeyCode::ArrowRight,
        |world, _| step(world, 1),
    ),
];

/// The models the chief starred, by name, kept with the view.
#[derive(
    Resource, Default, Clone, PartialEq, Debug, Serialize, Deserialize,
)]
pub struct Favorites(pub Vec<String>);

/// The side of the popup the chief chose last, kept with the view:
/// `favorites` or a provider's id; empty before any.
#[derive(
    Resource, Default, Clone, PartialEq, Debug, Serialize, Deserialize,
)]
pub struct LastSide(pub String);

/// What the popup hangs from: the composer that holds the model
/// chip, which it stands over as wide, or the chip alone outside one.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Hook {
    Chip,
    Composer,
}

/// The side of the popup chosen.
#[derive(Clone, PartialEq, Debug)]
enum Side {
    Favorites,
    Provider(String),
}

impl Side {
    /// The side `kept` names, as [`LastSide`] keeps it.
    fn of(kept: &str) -> Option<Self> {
        Some(match kept {
            "" => return None,
            "favorites" => Self::Favorites,
            provider => Self::Provider(provider.into()),
        })
    }

    fn kept(&self) -> String {
        match self {
            Self::Favorites => "favorites".into(),
            Self::Provider(provider) => provider.clone(),
        }
    }
}

/// The parts of the popup the arrows move between, left to right.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Rail,
    Search,
    Levels,
}

const PARTS: [Part; 3] = [Part::Rail, Part::Search, Part::Levels];

/// On the efforts: the model they are of.
#[derive(Component)]
struct LevelsOf(String);

/// On the provider and the effort chosen, which take the focus when
/// their part does.
#[derive(Component)]
struct Chosen;

/// Where the focus is to go once it is there to take it.
#[derive(Clone, PartialEq, Debug)]
enum Want {
    Part(Part),
    /// The efforts of the model, once they are shown.
    Levels(String),
}

/// What is typed and chosen in the popup.
#[derive(Resource, Clone, PartialEq)]
pub(crate) struct Picking {
    search: String,
    side: Side,
    focus: Option<Want>,
}

impl Default for Picking {
    fn default() -> Self {
        Self {
            search: String::new(),
            side: Side::Favorites,
            focus: None,
        }
    }
}

/// Opens the popup of `agent`'s model, its search in focus, on the
/// side chosen last while it still has models, else on the
/// favourites when there are any and else on the provider of the
/// model it runs.
pub fn open(world: &mut World, agent: Entity) {
    let last = world
        .get_resource::<LastSide>()
        .and_then(|last| Side::of(&last.0))
        .filter(|side| match side {
            Side::Favorites => !favourites(world).is_empty(),
            Side::Provider(id) => {
                providers(world).iter().any(|(at, _)| at == id)
            }
        });
    let side = if let Some(last) = last {
        last
    } else if world.resource::<Favorites>().0.is_empty() {
        let runs = now(world, agent).model;
        let providers = providers(world);
        providers
            .iter()
            .find(|(id, _)| id == provider_of(&runs))
            .or(providers.first())
            .map_or(Side::Favorites, |(id, _)| {
                Side::Provider(id.clone())
            })
    } else {
        Side::Favorites
    };
    *world.resource_mut::<Picking>() = Picking {
        search: String::new(),
        side,
        focus: Some(Want::Part(Part::Search)),
    };
    *world.resource_mut::<Open>() = Open::Model(agent);
}

/// Types `text` into the search of the popup open.
pub fn search(world: &mut World, text: &str) {
    world.resource_mut::<Picking>().search = text.to_string();
}

/// Every effort, weakest first.
const EFFORTS: [Effort; 7] = [
    Effort::Off,
    Effort::Minimal,
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::XHigh,
    Effort::Max,
];

/// An effort as a command names it.
const fn spelled(effort: Effort) -> &'static str {
    match effort {
        Effort::Off => "off",
        Effort::Minimal => "minimal",
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
    }
}

/// An effort in the chief's words.
const fn words(effort: Effort) -> &'static str {
    match effort {
        Effort::Off => "Off",
        Effort::Minimal => "Minimal",
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
        Effort::XHigh => "Extra High",
        Effort::Max => "Max",
    }
}

/// How `agent` is named in a chief's command.
fn address(world: &World, agent: Entity) -> String {
    match world.get::<Kind>(agent).map(|kind| kind.0) {
        Some(protocol::AgentKind::Hand) => "hand".into(),
        _ => name_of(world, agent),
    }
}

/// What `agent` runs now, and what it falls back to.
#[derive(Clone, PartialEq)]
struct Now {
    /// The model it runs, its own or the one it falls back to.
    model: String,
    /// The model it falls back to without one of its own.
    fallback: String,
    effort: Option<Effort>,
    /// The effort it falls back to without one of its own.
    realm_effort: Option<Effort>,
    fast: bool,
}

impl Now {
    /// The effort it thinks at, and the one it falls back to, for the
    /// model `card`.
    fn efforts(
        &self,
        card: Option<&protocol::Listed>,
    ) -> (Option<Effort>, Option<Effort>) {
        let fallback = self
            .realm_effort
            .or(card.and_then(|card| card.default_effort));
        (self.effort.or(fallback), fallback)
    }
}

fn now(world: &World, agent: Entity) -> Now {
    let settings = world.get_resource::<Settings>();
    let realm_model = settings
        .map(|settings| settings.0.model.clone())
        .unwrap_or_default();
    let fallback = world
        .get::<LivesIn>(agent)
        .and_then(|lives| world.get::<Bylaws>(lives.0))
        .and_then(|limits| limits.0.model.clone())
        .unwrap_or(realm_model);
    Now {
        model: world
            .get::<Account>(agent)
            .map(|account| account.model.clone())
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| fallback.clone()),
        fallback,
        effort: world.get::<OwnEffort>(agent).map(|own| own.0),
        realm_effort: settings.and_then(|settings| settings.0.effort),
        fast: world.get::<Fast>(agent).is_some(),
    }
}

/// The model `name` of the catalogue.
fn card(world: &World, name: &str) -> Option<protocol::Listed> {
    world
        .get_resource::<Shelf>()?
        .models
        .iter()
        .find(|model| model.name == name)
        .cloned()
}

/// Each provider of the catalogue, by its id and its name.
fn providers(world: &World) -> Vec<(String, String)> {
    let mut ids = world
        .get_resource::<Shelf>()
        .into_iter()
        .flat_map(|shelf| shelf.models.iter())
        .map(|model| provider_of(&model.name).to_string())
        .filter(|provider| !provider.is_empty())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let name = provider_name(world, &id);
            (id, name)
        })
        .collect()
}

/// How well `model` answers every word of `search`, best lowest; none
/// when a word is in neither its names nor its provider's.
fn rank(world: &World, model: &str, search: &str) -> Option<usize> {
    let names = [
        model_name(model).to_lowercase(),
        model_title(world, model).to_lowercase(),
    ];
    let whole = model.to_lowercase();
    let provider =
        provider_name(world, provider_of(model)).to_lowercase();
    // How well `word` answers `name`, best lowest.
    let answers = |name: &String, word: &str| {
        let starts_a_part = name
            .split(['-', '/', '.', '_', ' ', ':'])
            .any(|part| part.starts_with(word));
        if name == word {
            Some(0)
        } else if name.starts_with(word) {
            Some(1)
        } else if starts_a_part {
            Some(2)
        } else if name.contains(word) {
            Some(3)
        } else {
            None
        }
    };
    search.split_whitespace().try_fold(0, |sum, word| {
        let score = names
            .iter()
            .filter_map(|name| answers(name, word))
            .min()
            .or_else(|| {
                (whole.contains(word) || provider.contains(word))
                    .then_some(4)
            })?;
        Some(sum + score)
    })
}

/// The favourites the catalogue lists, in the order they were
/// starred.
fn favourites(world: &World) -> Vec<String> {
    let listed = |name: &&String| {
        world.get_resource::<Shelf>().is_some_and(|shelf| {
            shelf.models.iter().any(|model| model.name == **name)
        })
    };
    world
        .resource::<Favorites>()
        .0
        .iter()
        .filter(listed)
        .cloned()
        .collect()
}

/// The models shown: those found by the search across every
/// provider, best first, or else those of the side chosen.
fn listed(world: &World) -> Vec<String> {
    let picking = world.resource::<Picking>();
    let names = world
        .get_resource::<Shelf>()
        .into_iter()
        .flat_map(|shelf| shelf.models.iter())
        .map(|model| model.name.clone());
    let search = picking.search.trim().to_lowercase();
    if !search.is_empty() {
        let mut found = names
            .filter_map(|name| {
                Some((rank(world, &name, &search)?, name))
            })
            .collect::<Vec<_>>();
        found.sort_by_key(|(score, name)| {
            (*score, model_name(name).len())
        });
        return found.into_iter().map(|(_, name)| name).collect();
    }
    match &picking.side {
        Side::Favorites => favourites(world),
        // Its newest first.
        Side::Provider(id) => {
            let mut models = names
                .filter(|name| provider_of(name) == id)
                .collect::<Vec<_>>();
            models.sort_by_cached_key(|name| {
                Reverse(version(&model_title(world, name)))
            });
            models
        }
    }
}

/// The version in a model's title, as `[5, 5]` in `Opus 5.5`; none
/// for a title with none, which sorts last.
fn version(title: &str) -> Option<Vec<u32>> {
    title
        .split(|c: char| c.is_whitespace() || c == '-')
        .rev()
        .find_map(|word| {
            word.split('.')
                .map(|part| {
                    part.parse::<u32>().ok().filter(|n| *n < 1000)
                })
                .collect::<Option<Vec<_>>>()
        })
}

/// Gives `agent` the model `model`, or no model of its own for the
/// one it falls back to, and sends the focus to its efforts.
fn choose(world: &mut World, agent: Entity, model: &str) {
    let to = address(world, agent);
    let given = if model == now(world, agent).fallback {
        "none"
    } else {
        model
    };
    crate::chat::command(world, format!("/model {to} {given}"));
    world.resource_mut::<Picking>().focus =
        Some(Want::Levels(model.to_string()));
}

fn pick_at<const N: usize>(world: &mut World, _: Invoke) {
    let Open::Model(agent) = *world.resource::<Open>() else {
        return;
    };
    if let Some(name) = favourites(world).get(N) {
        let name = name.clone();
        choose(world, agent, &name);
    }
}

fn set_effort(
    world: &mut World,
    agent: Entity,
    effort: Option<Effort>,
) {
    let to = address(world, agent);
    let effort = effort.map_or("none", spelled);
    crate::chat::command(world, format!("/effort {to} {effort}"));
}

/// The part the focus is in, the search when it is in none.
fn focused_part(world: &mut World) -> Part {
    let Some(mut at) = world.resource::<InputFocus>().get() else {
        return Part::Search;
    };
    loop {
        if let Some(part) = world.get::<Part>(at) {
            return *part;
        }
        match world.get::<ChildOf>(at) {
            Some(parent) => at = parent.parent(),
            None => return Part::Search,
        }
    }
}

/// Asks for the focus in the part `by` places along from its own.
fn step(world: &mut World, by: isize) {
    let from = focused_part(world);
    let at = PARTS.iter().position(|part| *part == from).unwrap_or(1);
    let to = at.saturating_add_signed(by).min(PARTS.len() - 1);
    world.resource_mut::<Picking>().focus =
        Some(Want::Part(PARTS[to]));
}

/// Leaves the search for the providers with Left at its start, and
/// for the efforts with Right at its end. The field keeps the arrows
/// anywhere else.
fn leave_search(
    key: On<FocusedInput<KeyboardInput>>,
    texts: Query<&EditableText>,
    parents: Query<&ChildOf>,
    parts: Query<&Part>,
    mut picking: ResMut<Picking>,
) {
    let field = key.focused_entity;
    if key.event_target() != field
        || key.input.state != ButtonState::Pressed
    {
        return;
    }
    let in_search = parents
        .iter_ancestors(field)
        .any(|up| parts.get(up) == Ok(&Part::Search));
    let Ok(text) = texts.get(field) else {
        return;
    };
    if !in_search {
        return;
    }
    let caret = text.editor().raw_selection().text_range();
    let to = match key.input.key_code {
        KeyCode::ArrowLeft if caret == (0..0) => Part::Rail,
        KeyCode::ArrowRight
            if caret.start == text.value().to_string().len()
                && caret.is_empty() =>
        {
            Part::Levels
        }
        _ => return,
    };
    picking.focus = Some(Want::Part(to));
}

/// Gives the focus where it was asked for once that is shown: to the
/// field of the search, or to the chosen row of the providers or
/// efforts, else their first.
fn take_focus(world: &mut World) {
    let Some(want) = world.resource::<Picking>().focus.clone() else {
        return;
    };
    let root = match &want {
        Want::Part(part) => world
            .query::<(Entity, &Part)>()
            .iter(world)
            .find(|(_, at)| *at == part)
            .map(|(entity, _)| entity),
        Want::Levels(model) => world
            .query::<(Entity, &LevelsOf)>()
            .iter(world)
            .find(|(_, of)| of.0 == *model)
            .map(|(entity, _)| entity),
    };
    let Some(root) = root else {
        // A part not shown once the popup is, such as the efforts of
        // a model that does not reason, is not waited for.
        let shown =
            world.query::<&Part>().iter(world).next().is_some();
        if shown && matches!(want, Want::Part(_)) {
            world.resource_mut::<Picking>().focus = None;
        }
        return;
    };
    let mut under = vec![root];
    let mut first = None;
    let mut chosen = None;
    while let Some(node) = under.pop() {
        let world = &*world;
        if world.get::<Chosen>(node).is_some() {
            chosen = Some(node);
            break;
        }
        if first.is_none()
            && (world.get::<EditableText>(node).is_some()
                || world.get::<TabIndex>(node).is_some())
        {
            first = Some(node);
        }
        if let Some(children) = world.get::<Children>(node) {
            under.extend(children.iter().rev());
        }
    }
    let Some(node) = chosen.or(first) else {
        return;
    };
    world
        .resource_mut::<InputFocus>()
        .set(node, FocusCause::Navigated);
    world.resource_mut::<Picking>().focus = None;
}

/// The favourites over the providers of the catalogue, each by its
/// mark and its name, the one chosen lit. Faded, none of them lit,
/// and kept in place while a search runs over all of them, so the
/// list beside it does not move.
fn rail() -> impl View {
    keyed(
        derived(|world: &World| {
            let picking = world.resource::<Picking>();
            let searching = !picking.search.trim().is_empty();
            (providers(world), picking.side.clone(), searching)
        })
        .when(every_frame()),
        |(providers, on, searching)| {
            let searching = *searching;
            // Faded while a search runs over every provider.
            let shown = if searching { 0.45 } else { 1.0 };
            let tab = |side: Side, face: AnyView, name: String| {
                let chosen = *on == side;
                let item = rail_item(
                    face,
                    name,
                    chosen && !searching,
                    shown,
                )
                .on_activate(move |world| {
                    let mut picking = world.resource_mut::<Picking>();
                    picking.side = side.clone();
                    picking.focus = Some(Want::Part(Part::Rail));
                    world
                        .resource_mut::<LastSide>()
                        .set_if_neq(LastSide(side.kept()));
                });
                if chosen {
                    item.tagged(Chosen).boxed()
                } else {
                    item.boxed()
                }
            };
            let mut tabs = vec![tab(
                Side::Favorites,
                glyph(Glyph::Star, ICON).boxed(),
                "Favourites".into(),
            )];
            tabs.push(
                with_cx(|cx| hairline(cx.theme().palette.gold_6))
                    .boxed(),
            );
            for (id, name) in providers {
                tabs.push(tab(
                    Side::Provider(id.clone()),
                    mark(id, ICON),
                    name.clone(),
                ));
            }
            row((
                column(tabs)
                    .gap(2.0)
                    .width(px(RAIL))
                    .tagged(Part::Rail),
                upright(),
            ))
            .gap(GAP)
            .align(AlignItems::Stretch)
            .boxed()
        },
    )
    .within(row(()))
}

/// A star that adds `name` to the favourites, or takes it off:
/// filled while it is the favourite at `at`, else hollow and faint.
/// It is never gold, the check's colour for the model in use.
fn star(name: String, at: Option<usize>) -> AnyView {
    let starred = at.is_some();
    let face = if starred {
        glyph(Glyph::Starred, ICON).tone(Tone::Body).opacity(1.0)
    } else {
        glyph(Glyph::Star, ICON).tone(Tone::Dim).opacity(0.5)
    };
    let said = match at.and_then(chord) {
        Some(chord) => {
            format!("Take off the favourites. {chord} picks it.")
        }
        None if starred => "Take off the favourites".to_string(),
        None => "Add to the favourites".to_string(),
    };
    button(face)
        .padding(UiRect::all(px(2.0)))
        .rules(bevy_fynix::views::tint)
        .tooltip(move || note(said.clone()))
        .on_activate(move |world| {
            let mut favorites = world.resource_mut::<Favorites>();
            match favorites.0.iter().position(|kept| *kept == name) {
                Some(at) => {
                    favorites.0.remove(at);
                }
                None => favorites.0.push(name.clone()),
            }
        })
        .boxed()
}

/// The keys that pick the favourite at `at`.
fn chord(at: usize) -> Option<Chord> {
    let (_, _, key, _) = PICKED_BY_DIGIT.get(at)?;
    Some(Chord {
        key: *key,
        mods: Mods::PRIMARY,
    })
}

/// The key that picks the favourite at `at`, as a small cap.
fn key_cap(at: usize) -> AnyView {
    let Some(chord) = chord(at) else {
        return frame().boxed();
    };
    with_cx(move |cx| {
        let theme = cx.theme();
        column((label(chord.to_string())
            .tone(Tone::Faint)
            .wrap(false)
            .rules(small_rules),))
        .padding(UiRect::axes(px(4.0), px(1.0)))
        .border(theme.space.border)
        .border_color(theme.palette.gold_6)
        .radius(3.0)
    })
    .boxed()
}

/// What a list with no models in it says, and what to do about it.
fn nothing(title: &'static str, then: &'static str) -> AnyView {
    column((
        label(title).tone(Tone::Dim).bold(true).wrap(false),
        description(then),
    ))
    .gap(4.0)
    .padding(UiRect::axes(px(8.0), px(12.0)))
    .boxed()
}

/// The search and the models it finds, each favourite with the key
/// that picks it.
fn models(agent: Entity, search: String) -> impl View {
    let search = crate::search::search(
        "Search models",
        search,
        |world, text| {
            let mut picking = world.resource_mut::<Picking>();
            if picking.search != text {
                picking.search = text;
            }
        },
    );
    let list = keyed(
        derived(move |world: &World| {
            let picking = world.resource::<Picking>();
            let searching = !picking.search.trim().is_empty();
            (
                listed(world),
                now(world, agent),
                favourites(world),
                (!searching).then(|| picking.side.clone()),
            )
        })
        .when(every_frame()),
        move |(models, now, favorites, side)| {
            if models.is_empty() {
                return match side {
                    None => nothing(
                        "No models found",
                        "Try another name, or part of one.",
                    ),
                    Some(Side::Favorites) => nothing(
                        "No favourites yet",
                        "Star a model to keep it here.",
                    ),
                    Some(Side::Provider(_)) => nothing(
                        "No models here",
                        "This provider lists no models yet.",
                    ),
                };
            }
            // A search runs over every provider, whichever is
            // chosen down the side.
            let over = match side {
                None => {
                    over_rows("From every provider", Detail::Source)
                }
                Some(_) => frame().boxed(),
            };
            let rows = models
                .iter()
                .map(|name| {
                    let at = favorites.iter().position(|f| f == name);
                    // Only among the favourites does every row have
                    // its key.
                    let cap = match (side, at) {
                        (Some(Side::Favorites), Some(at)) => {
                            key_cap(at)
                        }
                        _ => frame().boxed(),
                    };
                    let end = row((cap, star(name.clone(), at)))
                        .gap(6.0)
                        .align(AlignItems::Center)
                        .boxed();
                    let picked = name.clone();
                    model_row(
                        ModelRow {
                            name: name.clone(),
                            picked: now.model == *name,
                            tag: (now.fallback == *name)
                                .then_some("Default"),
                            end: Some(end),
                            detail: Detail::Source,
                        },
                        move |world| choose(world, agent, &picked),
                    )
                })
                .collect::<Vec<_>>();
            column((
                legend(Detail::Source),
                over,
                column(rows).gap(2.0),
            ))
            .gap(4.0)
            .width(percent(100.0))
            .boxed()
        },
    )
    .within(column(()).width(percent(100.0)));
    column((
        column((search,)).tagged(Part::Search),
        hamlet_ui::styles::scroll_area((list,)),
    ))
    .gap(8.0)
    .width(px(LIST))
    .grow(1.0)
    .min_width(px(LIST))
}

/// The efforts the model can think at, under a head naming it, the
/// one it falls back to tagged.
fn reasoning(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let now = now(world, agent);
            let card = card(world, &now.model);
            (now, card)
        })
        .when(every_frame()),
        move |(now, card)| {
            let head = column((
                label("Reasoning")
                    .tone(Tone::Faint)
                    .bold(true)
                    .wrap(false)
                    .rules(small_rules),
                one_line(label(format!(
                    "for {}",
                    card.as_ref().map_or_else(
                        || model_name(&now.model).to_string(),
                        title_of,
                    )
                )))
                .rules(hamlet_ui::styles::note_rules),
            ))
            .gap(1.0)
            .padding(UiRect::axes(px(8.0), px(4.0)));
            let efforts = match card {
                Some(card) if !card.efforts.is_empty() => {
                    card.efforts.clone()
                }
                _ => EFFORTS.to_vec(),
            };
            if efforts == [Effort::Off] {
                return column((
                    head,
                    column((description(
                        "This model does not reason.",
                    ),))
                    .padding(UiRect::axes(px(8.0), px(4.0))),
                ))
                .gap(4.0)
                .boxed();
            }
            let (on, fallback) = now.efforts(card.as_ref());
            let levels = efforts
                .into_iter()
                .map(|effort| {
                    let is_fallback = fallback == Some(effort);
                    let default = if is_fallback {
                        tag("Default")
                    } else {
                        frame().boxed()
                    };
                    let item = menu_item(
                        row((
                            radio(on == Some(effort)),
                            label(words(effort)).wrap(false),
                            default,
                        ))
                        .gap(8.0)
                        .align(AlignItems::Center)
                        .width(percent(100.0)),
                    )
                    .rules(level_look)
                    .on_activate(move |world| {
                        let chosen = (!is_fallback).then_some(effort);
                        set_effort(world, agent, chosen);
                    });
                    if on == Some(effort) {
                        item.tagged(Chosen).boxed()
                    } else {
                        item.boxed()
                    }
                })
                .collect::<Vec<_>>();
            column((
                head,
                column(levels)
                    .gap(2.0)
                    .tagged(Part::Levels)
                    .tagged(LevelsOf(now.model.clone())),
            ))
            .gap(4.0)
            .boxed()
        },
    )
    .within(column(()))
}

/// The look of an effort: lit under the pointer or the focus.
fn level_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .when::<Focused>(|s| s.fill(|t| t.palette.button_hover))
        .apply(cx);
}

/// Standard or fast, under the efforts, for a model whose provider
/// has a fast tier; nothing for one without.
fn tier(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let now = now(world, agent);
            let offered =
                card(world, &now.model).is_some_and(|card| card.fast);
            (offered, now.fast)
        })
        .when(every_frame()),
        move |(offered, fast)| {
            if !*offered {
                return frame().boxed();
            }
            let fast = *fast;
            column((
                with_cx(|cx| hairline(cx.theme().palette.gold_6)),
                column((label("Speed")
                    .tone(Tone::Faint)
                    .bold(true)
                    .wrap(false)
                    .rules(small_rules),))
                .padding(UiRect::axes(px(8.0), px(0.0))),
                column((
                    labelled_switch(
                        "Fast",
                        fast,
                        move |world, on| {
                            let to = address(world, agent);
                            let on = if on { "on" } else { "off" };
                            crate::chat::command(
                                world,
                                format!("/fast {to} {on}"),
                            );
                        },
                    ),
                    description(
                        "Quicker replies, at a higher price.",
                    ),
                ))
                .gap(4.0)
                .padding(UiRect::horizontal(px(8.0))),
            ))
            .gap(8.0)
            .boxed()
        },
    )
    .within(column(()))
}

/// Above the composer first, as it is at the foot of the chat, lined
/// up with its left edge, then its right.
fn above() -> Vec<PopoverPlacement> {
    use PopoverAlign::{End, Start};
    use PopoverSide::{Bottom, Top};
    [(Top, Start), (Top, End), (Bottom, Start), (Bottom, End)]
        .into_iter()
        .map(|(side, align)| PopoverPlacement {
            side,
            align,
            gap: 6.0,
        })
        .collect()
}

/// What the popup hangs from: the composer around the model chip
/// shown, in whole pixels so the popup is not built again for a
/// fraction.
fn hook(world: &World) -> Option<Rect> {
    let mut hooks = world.try_query::<(Entity, &Hook)>()?;
    let (chip, rect) = hooks
        .iter(world)
        .filter(|(_, hook)| **hook == Hook::Chip)
        .find_map(|(chip, _)| {
            let rect = rect_of(world, chip)?;
            (rect.width() > 0.0).then_some((chip, rect))
        })?;
    let rect = std::iter::successors(Some(chip), |at| {
        world.get::<ChildOf>(*at).map(ChildOf::parent)
    })
    .find(|at| world.get::<Hook>(*at) == Some(&Hook::Composer))
    .and_then(|bar| rect_of(world, bar))
    .unwrap_or(rect);
    Some(Rect::new(
        rect.min.x.round(),
        rect.min.y.round(),
        rect.max.x.round(),
        rect.max.y.round(),
    ))
}

/// The popup of `agent`'s model, hung over the composer once its chip
/// is laid out: as wide as the composer where that leaves the list
/// its room.
pub(super) fn picker(agent: Entity) -> impl View {
    keyed(derived(hook).when(every_frame()), move |hook| {
        let Some(hook) = *hook else {
            return frame().boxed();
        };
        let fixed = RAIL + LEVELS + 2.0 + 5.0 * GAP + 2.0 * PADDING;
        let wide = hook.width().max(fixed + LIST);
        with_cx(move |cx| {
            let search =
                cx.world.resource::<Picking>().search.clone();
            let body = row((
                rail(),
                models(agent, search),
                upright(),
                column((reasoning(agent), tier(agent)))
                    .gap(GAP)
                    .width(px(LEVELS))
                    .shrink(0.0),
            ))
            .gap(GAP)
            .align(AlignItems::Stretch)
            .height(percent(100.0))
            .tagged(Scope(MODELS));
            popup(hook, (body,))
                .placements(above())
                .on_dismiss(|world, _| close(world))
                .width(px(wide))
                .height(px(TALL))
                .padding(UiRect::all(px(PADDING)))
        })
        .boxed()
    })
    .within(overlay(()))
}

/// The model `agent` runs as the composer's chip shows it: its
/// provider's mark, its name, the effort it thinks at and a bolt on
/// its fast tier. Its tooltip names them in words.
pub(crate) fn chip(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let now = now(world, agent);
            let card = card(world, &now.model);
            let reasons = card
                .as_ref()
                .is_none_or(|card| card.efforts != [Effort::Off]);
            let effort =
                now.efforts(card.as_ref()).0.filter(|_| reasons);
            let title = model_title(world, &now.model);
            (now, effort, title)
        })
        .when(every_frame()),
        |(now, effort, title)| {
            let name = if now.model.is_empty() {
                "Default model".to_string()
            } else {
                title.clone()
            };
            let mut parts = vec![
                mark(provider_of(&now.model), ICON),
                label(name).wrap(false).boxed(),
            ];
            if let Some(effort) = effort {
                parts.push(
                    label(format!("· {}", words(*effort)))
                        .wrap(false)
                        .tone(Tone::Dim)
                        .boxed(),
                );
            }
            if now.fast {
                parts.push(
                    glyph(Glyph::Fast, 12.0)
                        .tone(Tone::Accent)
                        .boxed(),
                );
            }
            let mut said = vec![if now.model.is_empty() {
                "The default model".to_string()
            } else {
                now.model.clone()
            }];
            if let Some(effort) = effort {
                said.push(format!("{} reasoning", words(*effort)));
            }
            if now.fast {
                said.push("fast".into());
            }
            let said = said.join(" · ");
            row(parts)
                .gap(6.0)
                .align(AlignItems::Center)
                .tooltip(move || note(said.clone()))
                .boxed()
        },
    )
    .within(row(()))
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::input::keyboard::Key;
    use bevy::input_focus::InputDispatchPlugin;
    use bevy::math::Vec2;
    use bevy::window::{PrimaryWindow, Window};
    use hamlet_client::outbox::Outbox;
    use hamlet_client::realm;

    use super::*;
    use crate::testing::{panel_app, press};

    /// The part the focus is in, and whether the focus is anywhere.
    fn focus_in(app: &mut App) -> Option<Part> {
        let world = app.world_mut();
        world.resource::<InputFocus>().get()?;
        Some(focused_part(world))
    }

    fn arrow(app: &mut App, window: Entity, key: KeyCode) {
        let logical_key = match key {
            KeyCode::ArrowLeft => Key::ArrowLeft,
            _ => Key::ArrowRight,
        };
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Unidentified(
                bevy::input::keyboard::NativeKey::Unidentified,
            ),
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }

    #[test]
    fn arrows_move_between_the_parts_and_a_pick_moves_to_its_efforts()
    {
        let (mut app, ada) = picking();
        app.add_plugins((
            bevy::input::InputPlugin,
            InputDispatchPlugin,
        ));
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        assert_eq!(
            focus_in(&mut app),
            Some(Part::Search),
            "on opening"
        );
        arrow(&mut app, window, KeyCode::ArrowRight);
        assert_eq!(focus_in(&mut app), Some(Part::Levels));
        arrow(&mut app, window, KeyCode::ArrowLeft);
        assert_eq!(focus_in(&mut app), Some(Part::Search));
        arrow(&mut app, window, KeyCode::ArrowLeft);
        assert_eq!(focus_in(&mut app), Some(Part::Rail));
        press(&mut app, "wren-large");
        // The server gives her the model.
        app.world_mut().get_mut::<Account>(ada).unwrap().model =
            "lantern::wren-large".into();
        app.update();
        app.update();
        let world = app.world_mut();
        let focused = world.resource::<InputFocus>().get().unwrap();
        let levels = std::iter::successors(Some(focused), |at| {
            world.get::<ChildOf>(*at).map(ChildOf::parent)
        })
        .find_map(|at| world.get::<LevelsOf>(at))
        .map(|of| of.0.clone());
        assert_eq!(levels.as_deref(), Some("lantern::wren-large"));
    }

    /// The popup of `home#0`'s model open over a catalogue of two
    /// models, and her.
    fn picking() -> (App, Entity) {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .add_observer(crate::chat::decreed)
            .init_resource::<Shelf>()
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>();
        let listed = |name: &str| protocol::Listed {
            context: None,
            default_effort: Some(Effort::Medium),
            efforts: vec![Effort::Low, Effort::Medium, Effort::High],
            fast: false,
            name: name.into(),
            note: String::new(),
            price: None,
            priced: protocol::ListedPriced::Unknown,
            title: String::new(),
        };
        app.world_mut().resource_mut::<Shelf>().models = vec![
            listed("lantern::wren-large"),
            listed("lantern::wren-mini"),
        ];
        let ada = realm::named(app.world_mut(), "home#0").unwrap();
        // The chip it hangs from, laid out.
        app.world_mut().spawn((
            Hook::Chip,
            bevy::ui::ComputedNode {
                size: Vec2::new(120.0, 24.0),
                ..bevy::ui::ComputedNode::DEFAULT
            },
            bevy::ui::UiGlobalTransform::default(),
        ));
        open(app.world_mut(), ada);
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            super::super::view(),
        );
        app.update();
        (app, ada)
    }

    #[test]
    fn a_model_and_an_effort_picked_are_the_chiefs_commands() {
        let (mut app, _) = picking();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        press(&mut app, "wren-large");
        press(&mut app, "High");
        let said = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                protocol::ClientMessage::Say(say) => Some(say.text),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            said,
            [
                "/model home#0 lantern::wren-large",
                "/effort home#0 high"
            ]
        );
    }

    #[test]
    fn a_favourites_key_picks_it_whatever_the_search_shows() {
        let (mut app, _) = picking();
        app.world_mut().resource_mut::<Favorites>().0 =
            vec!["lantern::wren-large".into()];
        search(app.world_mut(), "wren");
        app.update();
        assert_eq!(listed(app.world())[0], "lantern::wren-mini");
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        pick_at::<0>(app.world_mut(), Invoke::default());
        let said = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                protocol::ClientMessage::Say(say) => Some(say.text),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(said, ["/model home#0 lantern::wren-large"]);
    }
}

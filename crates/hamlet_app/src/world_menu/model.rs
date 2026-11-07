//! The popup an agent's model is chosen in: the chief's favourites
//! and the providers down its side, the models of the one chosen
//! with a search over every provider, the efforts the model can
//! think at, and its fast tier when it has one. Its chip in the
//! composer opens it.

use bevy::app::{App, Update};
use bevy::color::Color;
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
use bevy::math::{Rect, Vec2};
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, Invoke, Layer, Mods, Scope,
    ScopeId, ScopeSpec, ShortcutAppExt,
};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, menu_item, popup, row,
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
    ICON, choices, description, hairline, note, small_rules,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};
use serde::{Deserialize, Serialize};

use super::{Open, close};
use crate::models::{
    ModelRow, Shelf, mark, model_name, model_row, provider_name,
    provider_of,
};

/// The width of the providers down the side.
const RAIL: f32 = 150.0;

/// The width of the search and its models.
const LIST: f32 = 340.0;

/// The width of the efforts.
const LEVELS: f32 = 200.0;

/// The popup's height, whatever it shows, so nothing in it moves
/// under the pointer as it changes.
const TALL: f32 = 400.0;

/// The scope of the popup's keys, over the desks' own.
const MODELS: ScopeId = ScopeId("models");

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Picking>()
        .init_resource::<Favorites>()
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

/// The keys that pick the first nine models shown, with the primary
/// key.
const PICKED_BY_DIGIT: [(
    &str,
    &str,
    KeyCode,
    fn(&mut World, Invoke),
); 9] = [
    ("models.pick_1", "Model 1", KeyCode::Digit1, pick_at::<0>),
    ("models.pick_2", "Model 2", KeyCode::Digit2, pick_at::<1>),
    ("models.pick_3", "Model 3", KeyCode::Digit3, pick_at::<2>),
    ("models.pick_4", "Model 4", KeyCode::Digit4, pick_at::<3>),
    ("models.pick_5", "Model 5", KeyCode::Digit5, pick_at::<4>),
    ("models.pick_6", "Model 6", KeyCode::Digit6, pick_at::<5>),
    ("models.pick_7", "Model 7", KeyCode::Digit7, pick_at::<6>),
    ("models.pick_8", "Model 8", KeyCode::Digit8, pick_at::<7>),
    ("models.pick_9", "Model 9", KeyCode::Digit9, pick_at::<8>),
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

/// The side of the popup chosen.
#[derive(Clone, PartialEq, Debug)]
enum Side {
    Favorites,
    Provider(String),
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

/// Opens the popup of `agent`'s model at `at`, its search in focus,
/// on the favourites when there are any and else on the provider of
/// the model it runs.
pub fn open(world: &mut World, agent: Entity, at: Vec2) {
    let side = if world.resource::<Favorites>().0.is_empty() {
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
    *world.resource_mut::<Open>() = Open::Model(agent, at);
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

/// How many of the chip's four bars an effort fills.
const fn bars_of(effort: Effort) -> usize {
    match effort {
        Effort::Off => 0,
        Effort::Minimal | Effort::Low => 1,
        Effort::Medium => 2,
        Effort::High => 3,
        Effort::XHigh | Effort::Max => 4,
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
/// when a word is in neither its name nor its provider's.
fn rank(world: &World, model: &str, search: &str) -> Option<usize> {
    let name = model_name(model).to_lowercase();
    let whole = model.to_lowercase();
    let provider =
        provider_name(world, provider_of(model)).to_lowercase();
    search.split_whitespace().try_fold(0, |sum, word| {
        let starts_a_part = name
            .split(['-', '/', '.', '_', ' ', ':'])
            .any(|part| part.starts_with(word));
        let score = if name == word {
            0
        } else if name.starts_with(word) {
            1
        } else if starts_a_part {
            2
        } else if name.contains(word) {
            3
        } else if whole.contains(word) || provider.contains(word) {
            4
        } else {
            return None;
        };
        Some(sum + score)
    })
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
        Side::Favorites => {
            let known = names.collect::<Vec<_>>();
            world
                .resource::<Favorites>()
                .0
                .iter()
                .filter(|name| known.contains(name))
                .cloned()
                .collect()
        }
        Side::Provider(id) => {
            names.filter(|name| provider_of(name) == id).collect()
        }
    }
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
    let Open::Model(agent, _) = *world.resource::<Open>() else {
        return;
    };
    if let Some(name) = listed(world).get(N) {
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
        // A part not shown once the popup is, such as the providers
        // hidden by a search, is not waited for.
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

/// The look of a provider down the side: its name small and bold,
/// lit under the pointer or the focus, brighter while chosen.
fn rail_look(chosen: bool) -> impl FnOnce(&mut Cx) + Send + Sync {
    let tone = if chosen { Tone::Body } else { Tone::Faint };
    style::<HamletTheme>()
        .label(|l, t| l.size(t.text.small).bold(true))
        .tone(tone)
        .frame(|f, t| f.fill(Color::NONE).radius(t.space.round))
        .hovered(|s| {
            s.fill(|t| t.palette.button_hover).tone(Tone::Body)
        })
        .when::<Focused>(|s| {
            s.fill(|t| t.palette.button_hover).tone(Tone::Body)
        })
        .transition(Motion::Interact)
        .bundle()
}

/// The favourites over the providers of the catalogue, each by its
/// mark and its name, a bar of the accent at the one chosen. Out of
/// the way while a search runs over all of them.
fn rail() -> impl View {
    keyed(
        derived(|world: &World| {
            let picking = world.resource::<Picking>();
            let searching = !picking.search.trim().is_empty();
            (providers(world), picking.side.clone(), searching)
        })
        .when(every_frame()),
        |(providers, on, searching)| {
            if *searching {
                return frame().boxed();
            }
            let tab = |side: Side, face: AnyView, name: String| {
                let chosen = *on == side;
                let bar = with_cx(move |cx| {
                    let fill = if chosen {
                        cx.theme().palette.accent
                    } else {
                        Color::NONE
                    };
                    frame()
                        .width(px(3.0))
                        .height(px(16.0))
                        .radius(1.5)
                        .fill(fill)
                });
                let item = menu_item(
                    row((bar, face, one_line(label(name))))
                        .gap(8.0)
                        .align(AlignItems::Center)
                        .width(percent(100.0)),
                )
                .padding(UiRect::right(px(8.0)))
                .rules(rail_look(chosen))
                .on_activate(move |world| {
                    let mut picking = world.resource_mut::<Picking>();
                    picking.side = side.clone();
                    picking.focus = Some(Want::Part(Part::Rail));
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
            .gap(10.0)
            .align(AlignItems::Stretch)
            .boxed()
        },
    )
    .within(row(()))
}

/// A star that adds `name` to the favourites, or takes it off. Shown
/// only on the row under the pointer, unless starred.
fn star(name: String, starred: bool) -> AnyView {
    let face = if starred {
        glyph(Glyph::Starred, ICON).tone(Tone::Accent).opacity(1.0)
    } else {
        glyph(Glyph::Star, ICON)
    };
    button(face)
        .padding(UiRect::all(px(2.0)))
        .rules(bevy_fynix::views::tint)
        .tooltip(move || {
            note(if starred {
                "Take off the favourites"
            } else {
                "Add to the favourites"
            })
        })
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

/// The key that picks the row at `at`, as a small cap.
fn key_cap(at: usize) -> AnyView {
    let Some((_, _, key, _)) = PICKED_BY_DIGIT.get(at) else {
        return frame().boxed();
    };
    let chord = Chord {
        key: *key,
        mods: Mods::PRIMARY,
    };
    with_cx(move |cx| {
        let theme = cx.theme();
        column((label(chord.to_string())
            .size(11.0)
            .tone(Tone::Faint)
            .wrap(false),))
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

/// The search and the models it finds, the first nine with their
/// keys.
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
                world.resource::<Favorites>().0.clone(),
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
            let rows = models
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    let starred = favorites.contains(name);
                    let end = row((
                        key_cap(at),
                        star(name.clone(), starred),
                    ))
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
                        },
                        move |world| choose(world, agent, &picked),
                    )
                })
                .collect::<Vec<_>>();
            column(rows).gap(2.0).width(percent(100.0)).boxed()
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
    .min_width(px(0.0))
}

/// A ring, filled while it is the one chosen, as a radio button's.
fn radio(on: bool) -> impl View {
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let dot = frame()
            .width(px(6.0))
            .height(px(6.0))
            .radius(3.0)
            .fill(if on { palette.accent } else { Color::NONE });
        column((dot,))
            .width(px(14.0))
            .height(px(14.0))
            .radius(7.0)
            .border(1.5)
            .border_color(if on {
                palette.accent
            } else {
                palette.gold_5
            })
            .align(AlignItems::Center)
            .justify(bevy::ui::JustifyContent::Center)
    })
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
            let head = row((
                label("Reasoning")
                    .tone(Tone::Faint)
                    .bold(true)
                    .wrap(false)
                    .rules(small_rules),
                frame().grow(1.0),
                one_line(label(model_name(&now.model).to_string()))
                    .rules(hamlet_ui::styles::note_rules),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
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
                    let tag = if is_fallback {
                        label("Default")
                            .tone(Tone::Faint)
                            .wrap(false)
                            .rules(small_rules)
                            .boxed()
                    } else {
                        frame().boxed()
                    };
                    let item = menu_item(
                        row((
                            radio(on == Some(effort)),
                            label(words(effort)).wrap(false),
                            frame().grow(1.0),
                            tag,
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
    .within(column(()).width(px(LEVELS)))
}

/// The look of an effort: lit under the pointer or the focus.
fn level_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .when::<Focused>(|s| s.fill(|t| t.palette.button_hover))
        .apply(cx);
}

/// Standard or fast, under the rest, for a model whose provider has
/// a fast tier; nothing for one without.
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
                row((
                    label("Speed")
                        .tone(Tone::Faint)
                        .bold(true)
                        .wrap(false)
                        .rules(small_rules),
                    choices(
                        ["Standard", "Fast"],
                        usize::from(fast),
                        move |world, at| {
                            let to = address(world, agent);
                            let on =
                                if at == 1 { "on" } else { "off" };
                            crate::chat::command(
                                world,
                                format!("/fast {to} {on}"),
                            );
                        },
                    ),
                ))
                .gap(10.0)
                .align(AlignItems::Center),
            ))
            .gap(8.0)
            .boxed()
        },
    )
    .within(column(()))
}

/// A line between the popup's parts, top to bottom.
fn upright() -> impl View {
    with_cx(|cx| {
        frame().width(px(1.0)).fill(cx.theme().palette.gold_6)
    })
}

/// Above the anchor first, as the chip that opens it is at the foot
/// of the composer, lined up with its left edge, then its right.
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

/// The popup of `agent`'s model at `at`.
pub(super) fn picker(agent: Entity, at: Vec2) -> impl View {
    with_cx(move |cx| {
        let padding = cx.theme().space.overlay_padding;
        let search = cx.world.resource::<Picking>().search.clone();
        let body = row((
            rail(),
            models(agent, search),
            upright(),
            reasoning(agent),
        ))
        .gap(10.0)
        .align(AlignItems::Stretch)
        .grow(1.0)
        .min_height(px(0.0));
        popup(
            Rect::from_corners(at, at),
            (column((body, tier(agent)))
                .gap(10.0)
                .height(percent(100.0))
                .tagged(Scope(MODELS)),),
        )
        .placements(above())
        .on_dismiss(|world, _| close(world))
        .width(px(RAIL + LIST + LEVELS + 4.0 * 10.0 + 2.0 * padding))
        .height(px(TALL))
        .padding(UiRect::all(px(padding)))
    })
}

/// The model `agent` runs as the composer's chip shows it: its
/// provider's mark, its name, the effort it thinks at as bars and a
/// bolt on its fast tier. Its tooltip names them in words.
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
            (now, effort)
        })
        .when(every_frame()),
        |(now, effort)| {
            let name = if now.model.is_empty() {
                "Default model".to_string()
            } else {
                model_name(&now.model).to_string()
            };
            let mut parts = vec![
                mark(provider_of(&now.model), ICON),
                label(name).wrap(false).boxed(),
            ];
            if let Some(effort) = effort {
                parts.push(bars(bars_of(*effort)).boxed());
            }
            if now.fast {
                parts.push(
                    glyph(Glyph::Bolt, 12.0)
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

/// Four rising bars, the first `filled` of them lit.
fn bars(filled: usize) -> impl View {
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let bars = (0..4)
            .map(|at| {
                frame()
                    .width(px(3.0))
                    .height(px(4.0 + 3.0 * at as f32))
                    .radius(1.0)
                    .fill(if at < filled {
                        palette.accent
                    } else {
                        palette.gold_6
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        row(bars).gap(2.0).align(AlignItems::FlexEnd)
    })
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::input::keyboard::Key;
    use bevy::input_focus::InputDispatchPlugin;
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

    /// The popup of `home#0`'s model open over a catalogue of one
    /// model, and her.
    fn picking() -> (App, Entity) {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .add_observer(crate::chat::decreed)
            .init_resource::<Shelf>()
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>();
        app.world_mut().resource_mut::<Shelf>().models =
            vec![protocol::Listed {
                context: None,
                default_effort: Some(Effort::Medium),
                efforts: vec![
                    Effort::Low,
                    Effort::Medium,
                    Effort::High,
                ],
                fast: false,
                name: "lantern::wren-large".into(),
                note: String::new(),
                price: None,
                priced: protocol::ListedPriced::Unknown,
            }];
        let ada = realm::named(app.world_mut(), "home#0").unwrap();
        open(app.world_mut(), ada, Vec2::ZERO);
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
}

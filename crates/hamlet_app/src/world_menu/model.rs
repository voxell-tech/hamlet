//! The popup an agent's model is chosen in: the chief's favourites
//! and the providers down its side, the models of the one chosen
//! with a search, the efforts the model can think at, and whether it
//! runs on its provider's fast tier.

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::math::{Rect, Vec2};
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, Invoke, Layer, Mods, Scope,
    ScopeId, ScopeSpec, ShortcutAppExt,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame, icon,
    label, menu_item, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{
    Account, Bylaws, Fast, Kind, LivesIn, OwnEffort, OwnModel,
    Settings, name_of,
};
use hamlet_protocol::{Effort, client as protocol};
use hamlet_ui::styles::{
    ICON, choice, choices, description, hairline, note, scroll_area,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::providers::provider_mark;
use hamlet_ui::{AnyView, View, with_cx};
use serde::{Deserialize, Serialize};

use super::{Open, close};
use crate::models::Shelf;

/// How many models a list shows before "Show more".
const FIRST: usize = 8;

/// How tall the lists grow before they scroll.
const TALL: f32 = 300.0;

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
        });
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

/// The models the chief starred, by name, kept with the view.
#[derive(
    Resource, Default, Clone, PartialEq, Debug, Serialize, Deserialize,
)]
pub struct Favorites(pub Vec<String>);

/// The side of the popup chosen.
#[derive(Clone, PartialEq, Default, Debug)]
enum Side {
    Favorites,
    #[default]
    All,
    Provider(String),
}

/// What is typed and chosen in the popup.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Picking {
    search: String,
    side: Side,
    more: bool,
}

/// Opens the popup of `agent`'s model at `at`, on the favourites when
/// there are any.
pub fn open(world: &mut World, agent: Entity, at: Vec2) {
    let side = if world.resource::<Favorites>().0.is_empty() {
        Side::All
    } else {
        Side::Favorites
    };
    *world.resource_mut::<Picking>() = Picking {
        side,
        ..Picking::default()
    };
    *world.resource_mut::<Open>() = Open::Model(agent, at);
}

/// The provider a model's name starts with.
fn provider(name: &str) -> &str {
    name.split_once("::").map_or("", |(provider, _)| provider)
}

/// A model's name without its provider.
pub(crate) fn short(name: &str) -> &str {
    name.split_once("::").map_or(name, |(_, model)| model)
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
pub(crate) const fn words(effort: Effort) -> &'static str {
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

/// What `agent` runs now, and what it was given of its own.
#[derive(Clone, PartialEq)]
struct Now {
    /// The model it runs, its own or the one it falls back to.
    model: String,
    own: Option<String>,
    /// The model it falls back to without one of its own.
    fallback: String,
    effort: Option<Effort>,
    /// The effort it falls back to without one of its own.
    realm_effort: Option<Effort>,
    fast: bool,
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
        own: world.get::<OwnModel>(agent).map(|own| own.0.clone()),
        fallback,
        effort: world.get::<OwnEffort>(agent).map(|own| own.0),
        realm_effort: settings.and_then(|settings| settings.0.effort),
        fast: world.get::<Fast>(agent).is_some(),
    }
}

/// The words of the composer's chip: the model, the effort it thinks
/// at and its fast tier.
pub(crate) fn chip_words(world: &World, agent: Entity) -> String {
    let now = now(world, agent);
    let mut words = if now.model.is_empty() {
        "Default model".to_string()
    } else {
        short(&now.model).to_string()
    };
    if let Some(effort) = now.effort.or(now.realm_effort) {
        words = format!("{words} · {}", words_lower(effort));
    }
    if now.fast {
        words.push_str(" · fast");
    }
    words
}

fn words_lower(effort: Effort) -> String {
    words(effort).to_lowercase()
}

/// The models of the catalogue on `side` whose names hold `search`.
fn listed(world: &World, picking: &Picking) -> Vec<protocol::Listed> {
    let favorites = &world.resource::<Favorites>().0;
    let search = picking.search.trim().to_lowercase();
    world
        .get_resource::<Shelf>()
        .into_iter()
        .flat_map(|shelf| shelf.models.iter())
        .filter(|model| match &picking.side {
            Side::All => true,
            Side::Favorites => favorites.contains(&model.name),
            Side::Provider(name) => provider(&model.name) == name,
        })
        .filter(|model| model.name.to_lowercase().contains(&search))
        .cloned()
        .collect()
}

/// The models shown: the first few until more are asked for.
fn shown(world: &World) -> (Vec<protocol::Listed>, usize) {
    let picking = world.resource::<Picking>();
    let mut models = listed(world, picking);
    let all = models.len();
    if !picking.more {
        models.truncate(FIRST);
    }
    (models, all)
}

/// Gives `agent` the model `model`, or its fallback for none.
fn choose(world: &mut World, agent: Entity, model: Option<&str>) {
    let to = address(world, agent);
    let model = model.unwrap_or("none");
    crate::chat::command(world, format!("/model {to} {model}"));
}

fn pick_at<const N: usize>(world: &mut World, _: Invoke) {
    let Open::Model(agent, _) = *world.resource::<Open>() else {
        return;
    };
    if let Some(model) = shown(world).0.get(N) {
        let name = model.name.clone();
        choose(world, agent, Some(&name));
    }
}

/// The favourites and each provider of the catalogue, one above
/// another.
fn side() -> impl View {
    keyed(
        derived(|world: &World| {
            let mut providers = world
                .get_resource::<Shelf>()
                .into_iter()
                .flat_map(|shelf| shelf.models.iter())
                .map(|model| provider(&model.name).to_string())
                .filter(|provider| !provider.is_empty())
                .collect::<Vec<_>>();
            providers.sort();
            providers.dedup();
            (providers, world.resource::<Picking>().side.clone())
        })
        .when(every_frame()),
        |(providers, on)| {
            let tab = |side: Side, face: AnyView, says: String| {
                let picked = *on == side;
                button(face)
                    .width(px(30.0))
                    .rules(choice(picked))
                    .tooltip(move || note(says.clone()))
                    .on_activate(move |world| {
                        let mut picking =
                            world.resource_mut::<Picking>();
                        picking.side = side.clone();
                        picking.more = false;
                    })
                    .boxed()
            };
            let mut tabs = vec![
                tab(
                    Side::Favorites,
                    glyph(Glyph::Star, ICON).boxed(),
                    "Favourites".into(),
                ),
                tab(
                    Side::All,
                    glyph(Glyph::Globe, ICON).boxed(),
                    "Every model".into(),
                ),
            ];
            for name in providers {
                // Its mark, or the first letter of an endpoint that
                // has none.
                let face = match provider_mark(name, ICON) {
                    Some(mark) => icon(mark).size(ICON).boxed(),
                    None => label(
                        name.chars()
                            .next()
                            .map(|first| {
                                first.to_uppercase().to_string()
                            })
                            .unwrap_or_default(),
                    )
                    .bold(true)
                    .boxed(),
                };
                tabs.push(tab(
                    Side::Provider(name.clone()),
                    face,
                    name.clone(),
                ));
            }
            column(tabs).gap(4.0).boxed()
        },
    )
    .within(column(()))
}

/// A star that adds `name` to the favourites, or takes it off.
fn star(name: String, starred: bool) -> AnyView {
    let face = if starred { Glyph::Starred } else { Glyph::Star };
    button(glyph(face, ICON))
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

/// One row of a list: a check while it is the one chosen, its words,
/// and what stands at its end.
fn line(
    on: bool,
    words: AnyView,
    end: AnyView,
    pick: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let check = if on {
        glyph(Glyph::Check, ICON).tone(Tone::Accent).boxed()
    } else {
        frame().width(px(ICON)).boxed()
    };
    menu_item(
        row((check, words, frame().grow(1.0), end))
            .gap(8.0)
            .align(AlignItems::Center)
            .width(percent(100.0)),
    )
    .on_activate(pick)
    .boxed()
}

/// The search and the models it finds, the first nine with their
/// keys.
fn models(agent: Entity) -> impl View {
    let search = super::search::search(
        "Search models",
        |world| world.resource::<Picking>().search.clone(),
        |world, text| {
            let mut picking = world.resource_mut::<Picking>();
            if picking.search != text {
                picking.search = text;
                picking.more = false;
            }
        },
    );
    let list = keyed(
        derived(move |world: &World| {
            let (models, all) = shown(world);
            let picking = world.resource::<Picking>();
            (
                models,
                all,
                now(world, agent),
                world.resource::<Favorites>().0.clone(),
                picking.side == Side::All
                    && picking.search.is_empty(),
            )
        })
        .when(every_frame()),
        move |(models, all, now, favorites, whole)| {
            let mut lines = Vec::new();
            if *whole {
                let fallback = if now.fallback.is_empty() {
                    String::new()
                } else {
                    short(&now.fallback).to_string()
                };
                lines.push(line(
                    now.own.is_none(),
                    label("Default").wrap(false).boxed(),
                    label(fallback)
                        .tone(Tone::Faint)
                        .wrap(false)
                        .boxed(),
                    move |world| choose(world, agent, None),
                ));
            }
            for (at, model) in models.iter().enumerate() {
                let name = model.name.clone();
                let key = if let Some((_, _, key, _)) =
                    PICKED_BY_DIGIT.get(at)
                {
                    let chord = Chord {
                        key: *key,
                        mods: Mods::PRIMARY,
                    };
                    label(chord.to_string())
                        .tone(Tone::Faint)
                        .wrap(false)
                        .boxed()
                } else {
                    frame().boxed()
                };
                let starred = favorites.contains(&name);
                let end = row((key, star(name.clone(), starred)))
                    .gap(6.0)
                    .align(AlignItems::Center)
                    .boxed();
                let picked = now.own.as_deref() == Some(&name);
                lines.push(line(
                    picked,
                    label(short(&name).to_string())
                        .wrap(false)
                        .boxed(),
                    end,
                    move |world| choose(world, agent, Some(&name)),
                ));
            }
            if models.is_empty() && !*whole {
                lines.push(description("No model here.").boxed());
            }
            if *all > models.len() {
                lines.push(
                    menu_item(label("Show more").tone(Tone::Dim))
                        .on_activate(|world| {
                            world.resource_mut::<Picking>().more =
                                true;
                        })
                        .boxed(),
                );
            }
            column(lines).gap(2.0).width(percent(100.0)).boxed()
        },
    )
    .within(column(()).width(percent(100.0)));
    column((
        search,
        column((scroll_area((list,)),)).max_height(px(TALL)),
    ))
    .gap(8.0)
    .width(px(270.0))
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

/// The efforts the model can think at, under its name.
fn reasoning(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let now = now(world, agent);
            let card = card(world, &now.model);
            (now, card)
        })
        .when(every_frame()),
        move |(now, card)| {
            let efforts = match card {
                Some(card) if !card.efforts.is_empty() => {
                    card.efforts.clone()
                }
                _ => EFFORTS.to_vec(),
            };
            let mut lines = vec![
                label("Reasoning")
                    .tone(Tone::Faint)
                    .bold(true)
                    .boxed(),
                label(short(&now.model).to_string())
                    .tone(Tone::Dim)
                    .wrap(false)
                    .boxed(),
            ];
            if efforts == [Effort::Off] {
                lines
                    .push(description("It does not reason.").boxed());
                return column(lines).gap(4.0).boxed();
            }
            let fallback = now
                .realm_effort
                .or(card
                    .as_ref()
                    .and_then(|card| card.default_effort))
                .map(words)
                .unwrap_or_default();
            lines.push(line(
                now.effort.is_none(),
                label("Default").wrap(false).boxed(),
                label(fallback).tone(Tone::Faint).wrap(false).boxed(),
                move |world| set_effort(world, agent, None),
            ));
            for effort in efforts {
                lines.push(line(
                    now.effort == Some(effort),
                    label(words(effort)).wrap(false).boxed(),
                    frame().boxed(),
                    move |world| {
                        set_effort(world, agent, Some(effort));
                    },
                ));
            }
            column(lines).gap(2.0).boxed()
        },
    )
    .within(column(()).width(px(170.0)))
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

/// Standard or fast, for a model whose provider has a fast tier.
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
                return label("No fast tier for this model")
                    .tone(Tone::Faint)
                    .wrap(false)
                    .boxed();
            }
            choices(
                ["Standard", "Fast"],
                usize::from(*fast),
                move |world, at| {
                    let to = address(world, agent);
                    let on = if at == 1 { "on" } else { "off" };
                    crate::chat::command(
                        world,
                        format!("/fast {to} {on}"),
                    );
                },
            )
            .boxed()
        },
    )
    .within(row(()))
}

/// The popup of `agent`'s model at `at`.
pub(super) fn picker(agent: Entity, at: Vec2) -> impl View {
    with_cx(move |cx| {
        let (line, padding) = {
            let theme = cx.theme();
            (theme.palette.gold_6, theme.space.overlay_padding)
        };
        let upright = || frame().width(px(1.0)).fill(line);
        let body = column((
            row((
                side(),
                upright(),
                models(agent),
                upright(),
                reasoning(agent),
            ))
            .gap(10.0)
            .align(AlignItems::Stretch),
            hairline(line),
            row((tier(agent),)).justify(JustifyContent::FlexStart),
        ))
        .gap(10.0)
        .tagged(Scope(MODELS));
        popup(Rect::from_corners(at, at), (body,))
            .on_dismiss(|world, _| close(world))
            .padding(UiRect::all(px(padding)))
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_client::realm;

    use super::*;
    use crate::testing::{panel_app, press};

    #[test]
    fn a_model_and_an_effort_picked_are_the_chiefs_commands() {
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
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            super::super::view(),
        );
        app.update();
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

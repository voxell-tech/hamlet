//! Addresses of hamlets, agents and profiles, copied and drawn as
//! chips.

use bevy::app::{App, PreUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Or, With};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{PointerClick, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{AlignItems, UiRect, px};
use bevy::window::SystemCursorIcon;
use bevy_fynix::EntityCursor;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{Atom, FrameProps, TooltipExt, label, row};
use hamlet_client::realm::{
    self as agents, HamletId, Identity, Kind, Lead, Left, Profile,
    Shown,
};
use hamlet_core::panels::PanelSystems;
use hamlet_core::sound::{Cue, Sound};
use hamlet_desktop::clipboard::CopyRich;
use hamlet_protocol::address::{Address, is_address};
use hamlet_protocol::client as protocol;
use hamlet_ui::markdown::AgentLinks;
use hamlet_ui::styles::{note, raised};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};

use crate::convo::{talk_to, talk_to_hand};
use crate::{hub, sounds};

/// The attribute rich text carries the words of a reference in.
const ATTRIBUTE: &str = "data-hamlet-reference";

/// The size of a chip's glyph and words.
const CHIP_TEXT: f32 = 12.0;

pub(crate) fn plugin(app: &mut App) {
    app.insert_resource(links())
        .add_observer(tap)
        .add_observer(open)
        .add_systems(
            PreUpdate,
            named_moved.in_set(PanelSystems::Bind),
        );
}

/// The addresses the chat takes and the chips it draws them as.
pub(crate) fn links() -> AgentLinks {
    AgentLinks {
        is_address,
        resolve: chip_of,
        generation: 0,
    }
}

/// What a chip opens.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Named {
    Agent(Entity),
    Profile(Entity),
}

/// Copies `hamlet` as `hamlet <id> (<name>)`.
pub(crate) fn copy_hamlet(world: &mut World, hamlet: Entity) {
    let Some(id) =
        world.get::<HamletId>(hamlet).map(|id| id.0.clone())
    else {
        return;
    };
    let name = agents::shown_of(world, hamlet);
    copy(world, format!("hamlet {id} ({name})"));
}

/// Copies `agent` as its address and name, as in `agent#3 (Mira)`.
pub(crate) fn copy_agent(world: &mut World, agent: Entity) {
    // An agent's name, which commands know it by, is its address.
    let address = agents::name_of(world, agent);
    if address.is_empty() {
        return;
    }
    let name = agents::shown_of(world, agent);
    let text = match name == address {
        true => address,
        false => format!("{address} ({name})"),
    };
    copy(world, text);
}

fn copy(world: &mut World, text: String) {
    let escaped = escape(&text);
    let html =
        format!("<span {ATTRIBUTE}=\"{escaped}\">{escaped}</span>");
    world.trigger(CopyRich { text, html });
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The living agent or the profile `address` names.
fn named(world: &World, address: &str) -> Option<Named> {
    let agent = match address.parse::<Address>().ok()? {
        Address::Profile(id) => {
            return agents::profile(world, id).map(Named::Profile);
        }
        Address::Hand => agents::hand(world),
        Address::Lead { hamlet } => agents::lead_of(world, &hamlet?),
        address @ (Address::Agent(_) | Address::Worker(_)) => {
            agents::named(world, &address.to_string())
        }
    }?;
    world
        .get::<Left>(agent)
        .is_none()
        .then_some(Named::Agent(agent))
}

/// The chip of what `address` names, which copies as `words`.
pub(crate) fn chip_of(
    world: &mut World,
    address: &str,
    words: &str,
    key: String,
) -> Option<Atom> {
    let named = named(world, address)?;
    let (mark, name) = match named {
        Named::Agent(agent) => {
            (mark_of(world, agent)?, agents::shown_of(world, agent))
        }
        Named::Profile(profile) => {
            let name = world.get::<Profile>(profile)?.0.name.clone();
            (Glyph::Profile, name)
        }
    };
    // A name in brackets after the address must be its current name.
    let bracket = words
        .strip_prefix(address)
        .map(str::trim)
        .filter(|rest| !rest.is_empty());
    if let Some(bracket) = bracket
        && bracket.strip_prefix('(').and_then(|b| b.strip_suffix(')'))
            != Some(name.as_str())
    {
        return None;
    }
    let address = address.to_string();
    Some(Atom::new::<HamletTheme>(key, words, move || {
        chip(named, mark, name.clone(), address.clone())
    }))
}

/// The mark of `agent` on its chip, by what kind of agent it is.
fn mark_of(world: &World, agent: Entity) -> Option<Glyph> {
    Some(match world.get::<Kind>(agent)?.0 {
        protocol::AgentKind::Hand => Glyph::Tower,
        protocol::AgentKind::Hamlet
            if world.get::<Lead>(agent).is_some() =>
        {
            Glyph::Lead
        }
        protocol::AgentKind::Hamlet => Glyph::HamletAgent,
        protocol::AgentKind::Worker => Glyph::Worker,
    })
}

/// The face of a chip: its mark and name on the reference fill.
fn face(cx: &mut Cx, mark: Glyph, name: String) -> impl View + use<> {
    let theme = cx.theme();
    row((
        raised(glyph(mark, CHIP_TEXT).tone(Tone::Reference)),
        label(name)
            .size(CHIP_TEXT)
            .wrap(false)
            .tone(Tone::Reference),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .padding(UiRect::axes(px(6.0), px(1.0)))
    .fill(theme.palette.status.info.fill)
    .radius(theme.space.round)
}

/// The chip of `agent` as the chat draws it, to be read only: a press
/// on it is left to what holds it.
pub(crate) fn agent_chip(world: &World, agent: Entity) -> AnyView {
    let mark = mark_of(world, agent).unwrap_or(Glyph::HamletAgent);
    let name = agents::shown_of(world, agent);
    AnyView::new(move |cx: &mut Cx| {
        let face = face(cx, mark, name.clone());
        cx.build(face)
    })
}

/// A chip in a line of the chat, its address in its tooltip.
fn chip(
    named: Named,
    mark: Glyph,
    name: String,
    address: String,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let chip = face(cx, mark, name.clone());
        let address = address.clone();
        let node =
            cx.build(chip.tooltip(move || note(address.clone())));
        cx.world
            .entity_mut(node)
            .insert((named, EntityCursor(SystemCursorIcon::Pointer)));
        node
    })
}

/// Has the chips drawn again when what they name changes.
fn named_moved(
    moved: Query<
        (),
        Or<(
            Changed<Name>,
            Changed<Shown>,
            Changed<Left>,
            Changed<Lead>,
            Changed<Profile>,
        )>,
    >,
    mut gone: (
        RemovedComponents<Identity>,
        RemovedComponents<Lead>,
        RemovedComponents<Profile>,
    ),
    mut links: ResMut<AgentLinks>,
) {
    let lost = gone.0.read().count()
        + gone.1.read().count()
        + gone.2.read().count();
    if lost > 0 || !moved.is_empty() {
        links.generation = links.generation.wrapping_add(1);
    }
}

fn tap(
    press: On<PointerPress>,
    chips: Query<(), With<Named>>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    if press.button == PointerButton::Primary
        && chips.contains(press.entity)
    {
        sounds::write(&mut cues, &mut turn, Sound::Tap);
    }
}

fn open(
    click: On<PointerClick>,
    chips: Query<&Named>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(named) = chips.get(click.entity).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| match named {
        Named::Agent(agent) if agents::hand(world) == Some(agent) => {
            talk_to_hand(world);
        }
        Named::Agent(agent) => talk_to(world, agent),
        Named::Profile(profile) => hub::open_profile(world, profile),
    });
}

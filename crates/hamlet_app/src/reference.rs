//! A hamlet or an agent named by its address: copied with its name,
//! in words and in rich text that tells Hamlet what it names, and an
//! agent's or a scholar's address in the chat drawn as a chip that
//! opens it.

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
    self as agents, HamletId, Identity, Kind, Left, Mayor, Scholar,
    Shown,
};
use hamlet_desktop::clipboard::CopyRich;
use hamlet_protocol::client as protocol;
use hamlet_realm::sound::{Cue, Sound};
use hamlet_ui::markdown::AgentLinks;
use hamlet_ui::styles::note;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx};

use crate::convo::{talk_to, talk_to_hand};
use crate::{PanelSystems, hub, sounds};

/// The attribute rich text carries the words of a reference in.
const ATTRIBUTE: &str = "data-hamlet-reference";

/// The size of a chip's glyph and words.
const CHIP_TEXT: f32 = 12.0;

/// Draws the addresses in the chat as chips, and opens what a clicked
/// one names.
pub(crate) fn plugin(app: &mut App) {
    app.insert_resource(AgentLinks {
        resolve: chip_of,
        generation: 0,
    })
    .add_observer(tap)
    .add_observer(open)
    .add_systems(PreUpdate, named_moved.in_set(PanelSystems::Bind));
}

/// What a chip opens.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Named {
    Agent(Entity),
    Scholar(Entity),
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

/// Copies `agent` as its address and name, as in `hire#3 (Mira)`.
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

/// The living agent or the scholar `address` names.
fn named(world: &World, address: &str) -> Option<Named> {
    if let Some(id) = address.strip_prefix("scholar#") {
        let id = protocol::ScholarId(id.parse().ok()?);
        return agents::scholar(world, id).map(Named::Scholar);
    }
    let agent = match address {
        "hand" => agents::hand(world),
        _ => match address.strip_prefix("mayor@") {
            Some(hamlet) => agents::mayor_of(world, hamlet),
            None => agents::named(world, address),
        },
    }?;
    world
        .get::<Left>(agent)
        .is_none()
        .then_some(Named::Agent(agent))
}

/// The chip of what `address` names, which copies as `words`. Words
/// that name it in brackets after its address, as a copied agent's
/// do, must name it as it is called now: other words in brackets are
/// not its name, and leave no chip of them.
pub(crate) fn chip_of(
    world: &mut World,
    address: &str,
    words: &str,
    key: String,
) -> Option<Atom> {
    let named = named(world, address)?;
    let (mark, name) = match named {
        Named::Agent(agent) => {
            let kind = world.get::<Kind>(agent)?.0;
            let mark = match kind {
                protocol::AgentKind::Hand => Glyph::Tower,
                protocol::AgentKind::Hire
                    if world.get::<Mayor>(agent).is_some() =>
                {
                    Glyph::Mayor
                }
                protocol::AgentKind::Hire => Glyph::Hire,
                protocol::AgentKind::Worker => Glyph::Worker,
            };
            (mark, agents::shown_of(world, agent))
        }
        Named::Scholar(scholar) => {
            let name = world.get::<Scholar>(scholar)?.0.name.clone();
            (Glyph::Scholar, name)
        }
    };
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

/// A chip in a line of the chat: the glyph and the name of what it
/// names, its address in its tooltip.
fn chip(
    named: Named,
    mark: Glyph,
    name: String,
    address: String,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let chip = row((
            glyph(mark, CHIP_TEXT).tone(Tone::Reference),
            label(name.clone())
                .size(CHIP_TEXT)
                .wrap(false)
                .tone(Tone::Reference),
        ))
        .gap(4.0)
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(6.0), px(1.0)))
        .fill(theme.palette.status.info.fill)
        .radius(theme.space.round);
        let address = address.clone();
        let node =
            cx.build(chip.tooltip(move || note(address.clone())));
        cx.world
            .entity_mut(node)
            .insert((named, EntityCursor(SystemCursorIcon::Pointer)));
        node
    })
}

/// Has the chips drawn again when an agent or a scholar came, went,
/// was renamed or took a mayor's office.
fn named_moved(
    moved: Query<
        (),
        Or<(
            Changed<Name>,
            Changed<Shown>,
            Changed<Left>,
            Changed<Mayor>,
            Changed<Scholar>,
        )>,
    >,
    mut gone: (
        RemovedComponents<Identity>,
        RemovedComponents<Mayor>,
        RemovedComponents<Scholar>,
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

/// Opens the chat with the agent a clicked chip names, or the page of
/// its scholar.
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
        Named::Scholar(scholar) => hub::open_scholar(world, scholar),
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::math::Vec2;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy_fynix::mount;
    use hamlet_ui::markdown::fixed;

    use super::*;
    use crate::Convo;
    use crate::testing::{panel_app, seat_hand};

    #[test]
    fn a_pressed_address_opens_the_chat_with_its_agent() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        seat_hand(&mut app);
        mount::<HamletTheme>(
            app.world_mut(),
            fixed("Ask hire#2 (Ada) about it.", Tone::Body),
        );
        app.update();
        app.update();
        let ada = agents::named(app.world(), "hire#2").unwrap();
        let world = app.world_mut();
        let chips = world
            .query::<(Entity, &Named)>()
            .iter(world)
            .map(|(chip, named)| (chip, *named))
            .collect::<Vec<_>>();
        let [(chip, named)] = chips[..] else {
            panic!("one chip, not {chips:?}");
        };
        assert_eq!(named, Named::Agent(ada));
        let at = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        world.trigger(PointerClick {
            entity: chip,
            pointer: Pointer::new(PointerId::Mouse, at),
            button: PointerButton::Primary,
            hit: HitData::new(chip, 0.0, None, None),
            duration: Duration::ZERO,
            count: 1,
        });
        app.update();
        assert_eq!(app.world().resource::<Convo>().agent, ada);
    }
}

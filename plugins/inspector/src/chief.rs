//! A chief's page of the Inspector (docs/chief.md): who they are,
//! where their mascot is, and their look with every choice of it.

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
    scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, component, derived, keyed};
use hamlet_agents_api::convo::ChiefName;
use hamlet_realm::chief::{
    self, Beside, Cloak, Hat, Holds, Light, Look,
};
use hamlet_realm::palette::Palette;
use hamlet_realm::pick::{Thing, focus_on, select};
use hamlet_sdk::checks::{either, moved};

use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    CONTROL, Fills, action, back, brass, buttons, filling, lamp,
    note, pad, title,
};
use crate::platform::DisplayText;
use crate::roll::agents_moved;
use crate::{AnyView, Cx, View};

/// Selects the place of the chief this window belongs to, and
/// brings it into view.
pub fn go_home(world: &mut World) {
    let user = world.resource::<ChiefName>().0.clone();
    let Some(chief) = chief::named(world, &user) else {
        return;
    };
    let thing = Thing::ChiefPlace(chief);
    select(world, Some(thing));
    focus_on(world, thing, false);
}

/// The facts of a chief its page shows.
#[derive(Clone, PartialEq)]
struct Facts {
    name: String,
    look: Look,
    /// The agent it stands by.
    with: Option<String>,
    cloaks: [Color; 8],
    lights: [Color; 5],
}

fn facts(world: &World, chief: Entity) -> Option<Facts> {
    let name = world.get::<Name>(chief)?.as_str().to_string();
    let look = *world.get::<Look>(chief)?;
    let with = world
        .get::<Beside>(chief)
        .and_then(|beside| beside.0)
        .and_then(|agent| world.get::<Name>(agent))
        .map(|name| name.as_str().to_string());
    let own;
    let pal = match world.get_resource::<Palette>() {
        Some(pal) => pal,
        None => {
            own = Palette::default();
            &own
        }
    };
    let shown = |c: Vec3| Color::linear_rgb(c.x, c.y, c.z);
    Some(Facts {
        name,
        look,
        with,
        cloaks: Cloak::ALL.map(|cloak| shown(cloak.colour(pal))),
        lights: Light::ALL.map(|light| shown(light.colour(pal))),
    })
}

/// Changes the look of `chief`. The mascot and its file follow.
fn change(world: &mut World, chief: Entity, to: impl Fn(&mut Look)) {
    let Some(mut look) = world.get_mut::<Look>(chief) else {
        return;
    };
    let mut next = *look;
    to(&mut next);
    if *look != next {
        *look = next;
    }
}

fn heading(text: &'static str) -> impl View {
    label(text)
        .size(11.0)
        .bold(true)
        .tone(Tone::Faint)
        .tagged(DisplayText)
}

/// The frame of a choice: lit when it is the one chosen.
fn chosen(on: bool) -> fn(&mut Cx) {
    if on { lamp } else { brass }
}

/// A square of `colour` to press. The one chosen is lit.
fn swatch(
    colour: Color,
    on: bool,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    button(
        frame()
            .width(px(16.0))
            .height(px(16.0))
            .fill(colour)
            .radius(8.0),
    )
    .padding(UiRect::all(px(5.0)))
    .min_height(px(CONTROL))
    .rules(chosen(on))
    .tagged(Mood::Plain)
    .on_activate(act)
}

/// A word to press. The one chosen is lit.
fn word(
    text: &'static str,
    on: bool,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    button(label(text.to_uppercase()).wrap(false).tagged(DisplayText))
        .padding(pad())
        .min_height(px(CONTROL))
        .rules(chosen(on))
        .tagged(Mood::Plain)
        .on_activate(act)
}

/// Choices in rows of no more than `most`, so a narrow panel holds
/// them.
fn rows(mut choices: Vec<AnyView>, most: usize) -> impl View {
    let mut lines = Vec::new();
    while !choices.is_empty() {
        let rest = choices.split_off(most.min(choices.len()));
        lines.push(row(choices).gap(6.0).boxed());
        choices = rest;
    }
    column(lines).gap(6.0)
}

/// The choices of a look, each pressed to take it.
fn wardrobe(chief: Entity, facts: &Facts) -> impl View {
    let look = facts.look;
    let cloaks = Cloak::ALL
        .into_iter()
        .zip(facts.cloaks)
        .map(|(cloak, colour)| {
            swatch(colour, look.cloak == cloak, move |world| {
                change(world, chief, |look| look.cloak = cloak)
            })
            .boxed()
        })
        .collect::<Vec<_>>();
    let lights = Light::ALL
        .into_iter()
        .zip(facts.lights)
        .map(|(light, colour)| {
            swatch(colour, look.light == light, move |world| {
                change(world, chief, |look| look.light = light)
            })
            .boxed()
        })
        .collect::<Vec<_>>();
    let hats = Hat::ALL
        .into_iter()
        .map(|hat| {
            word(hat.name(), look.hat == hat, move |world| {
                change(world, chief, |look| look.hat = hat)
            })
            .boxed()
        })
        .collect::<Vec<_>>();
    let held = Holds::ALL
        .into_iter()
        .map(|holds| {
            word(holds.name(), look.holds == holds, move |world| {
                change(world, chief, |look| look.holds = holds)
            })
            .boxed()
        })
        .collect::<Vec<_>>();
    column((
        heading("CLOAK, AND THE ROOF AT HOME"),
        rows(cloaks, 8),
        heading("THE LIGHT OF THE WICK"),
        rows(lights, 8),
        heading("ON THE HEAD"),
        rows(hats, 3),
        heading("IN THE HAND"),
        rows(held, 4),
    ))
    .gap(8.0)
}

/// A chief: who they are, where their mascot is, and their look
/// with every choice of it.
pub fn panel(chief: Entity) -> AnyView {
    // Its look, its company and its name are its own, and the name
    // of who it stands by is an agent's.
    let mut look = component::<Look, _>(chief, |_| ());
    let mut beside = component::<Beside, _>(chief, |_| ());
    let mut name = component::<Name, _>(chief, |_| ());
    let own = move |world: &World| {
        let moved = [
            look.changed(world),
            beside.changed(world),
            name.changed(world),
        ];
        moved.contains(&true)
    };
    let told =
        derived(move |world: &World| facts(world, chief)).when(
            either(own, either(agents_moved(), moved::<Palette>())),
        );
    keyed(told, move |facts| {
        let Some(facts) = facts else {
            return column(()).boxed();
        };
        let (place, tone) = match &facts.with {
            Some(agent) => (format!("With {agent}"), Tone::Accent),
            None => ("At home".to_string(), Tone::Dim),
        };
        column((
            row((
                title(facts.name.to_uppercase()),
                label(place).tone(tone).wrap(false),
            ))
            .gap(12.0)
            .align(AlignItems::Center),
            label(
                "This is you. Whoever you talk to knows your mascot \
                 as Wickstep.",
            )
            .tone(Tone::Dim),
            note(facts.look.words()),
            buttons(vec![
                action(Glyph::Hearth, "THE HEARTH", go_home),
                back("ALL HAMLETS", |world| select(world, None))
                    .boxed(),
            ]),
            scroll((wardrobe(chief, facts),)).fills_height(),
        ))
        .fills_height()
        .gap(8.0)
        .boxed()
    })
    .within(filling())
    .boxed()
}

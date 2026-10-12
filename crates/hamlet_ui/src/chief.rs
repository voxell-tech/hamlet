//! The chief in the panels (docs/chief.md): the company its mascot
//! keeps, the look kept between launches, and the chief's page of the
//! Inspector.

use bevy::app::{App, Startup, Update};
use bevy::color::Color;
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::system::{Query, Res};
use bevy::ecs::world::World;
use bevy::math::{Vec2, Vec3};
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
    scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, component, derived, keyed};
use hamlet_core::core::HomeDir;
use hamlet_core::session::UserMessage;
use hamlet_world::chief::{
    self, Beside, Chief, Cloak, Hat, Holds, Light, Look, Stands,
};
use hamlet_world::generator::society::Place as Ground;
use hamlet_world::library::{Library, Stood};
use hamlet_world::moving;
use hamlet_world::palette::Palette;
use hamlet_world::pick::{Selected, Thing, focus_on, select};

use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    CONTROL, Fills, action, back, brass, buttons, filling, lamp,
    note, pad, title,
};
use crate::me::Me;
use crate::platform::DisplayText;
use crate::roll::{agents_moved, either, moved};
use crate::{Build, Ui};

/// Brings the chief this window belongs to into the world, with the
/// look that was kept for them.
pub(crate) fn enter(world: &mut World) {
    let user = world.resource::<Me>().user.clone();
    if chief::named(world, &user).is_some() {
        return;
    }
    let home = world.resource::<HomeDir>().0.root.clone();
    let chief = chief::arrive(world, &user, Look::read(&home, &user));
    if let Some(stands) = Stands::read(&home, &user) {
        world.entity_mut(chief).insert(stands);
    }
    // The place the library was last set down, too.
    let kept = Stands::read_at(&Stood::file(&home));
    if let (Some(Stands(at)), Some(mut stood)) =
        (kept, world.get_resource_mut::<Stood>())
    {
        stood.0 = Some(at);
    }
}

/// Takes the place of this window's chief in hand, to be set down
/// with a click on the ground.
pub(crate) fn carry_home(world: &mut World) {
    moving::pick_up(world, Ground::Chief);
}

/// Sets the place of this window's chief down at `to`.
pub(crate) fn set_home(world: &mut World, to: Vec2) {
    let user = world.resource::<Me>().user.clone();
    if let Some(chief) = chief::named(world, &user) {
        world.entity_mut(chief).insert(Stands(to));
    }
}

/// Keeps the place a chief set down.
pub(crate) fn keep_place(
    home: Res<HomeDir>,
    placed: Query<(&Chief, &Stands), Changed<Stands>>,
) {
    for (chief, stands) in &placed {
        // One read back at launch is as it was written.
        if Stands::read(&home.0.root, &chief.user) != Some(*stands) {
            let _ = stands.write(&home.0.root, &chief.user);
        }
    }
}

/// Selects the place of the chief this window belongs to, and
/// brings it into view.
pub(crate) fn go_home(world: &mut World) {
    let user = world.resource::<Me>().user.clone();
    let Some(chief) = chief::named(world, &user) else {
        return;
    };
    let thing = Thing::ChiefPlace(chief);
    select(world, Some(thing));
    focus_on(world, thing, false);
}

/// The company of the mascot of a chief who has chosen `selected`,
/// when the chat is with `agent`: that agent while it or its house is
/// selected, and a hamlet while its common is.
fn company(selected: Option<Thing>, agent: Entity) -> Option<Entity> {
    match selected {
        Some(
            Thing::Keeper
            | Thing::Cottage
            | Thing::Resident(_)
            | Thing::ResidentHouse(_),
        ) => Some(agent),
        Some(Thing::Common(hamlet)) => Some(hamlet),
        _ => None,
    }
}

/// Sends the mascot to whoever the chat is pointed at, and home when
/// nobody is selected.
pub(crate) fn accompany(
    me: Res<Me>,
    selected: Res<Selected>,
    libraries: Query<Entity, With<Library>>,
    mut chiefs: Query<(&Chief, &mut Beside)>,
) {
    if !me.is_changed() && !selected.is_changed() {
        return;
    }
    // To the library when that is what was chosen.
    let with = match selected.0 {
        Some(Thing::Library) => libraries.iter().next(),
        chosen => company(chosen, me.agent),
    };
    for (chief, mut beside) in &mut chiefs {
        if chief.user == me.user && beside.0 != with {
            beside.0 = with;
        }
    }
}

/// A message sent with nobody selected brings the mascot from home
/// to whoever it was sent to.
pub(crate) fn spoke(
    message: On<UserMessage>,
    me: Res<Me>,
    mut chiefs: Query<(&Chief, &mut Beside)>,
) {
    if message.key != me.key {
        return;
    }
    for (chief, mut beside) in &mut chiefs {
        if chief.user == me.user && beside.0.is_none() {
            beside.0 = Some(me.agent);
        }
    }
}

/// Writes a look that was changed to its chief's file.
pub fn keep(home: Res<HomeDir>, looks: Query<(&Chief, Ref<Look>)>) {
    for (chief, look) in &looks {
        if look.is_changed() && !look.is_added() {
            let _ = look.write(&home.0.root, &chief.user);
        }
    }
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

fn heading(text: &'static str) -> Ui {
    label(text)
        .size(11.0)
        .bold(true)
        .tone(Tone::Faint)
        .tagged(DisplayText)
        .boxed()
}

/// The frame of a choice: lit when it is the one chosen.
fn chosen(on: bool) -> fn(&mut Build) {
    if on { lamp } else { brass }
}

/// A square of `colour` to press. The one chosen is lit.
fn swatch(
    colour: Color,
    on: bool,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> Ui {
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
    .boxed()
}

/// A word to press. The one chosen is lit.
fn word(
    text: &'static str,
    on: bool,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> Ui {
    button(label(text.to_uppercase()).wrap(false).tagged(DisplayText))
        .padding(pad())
        .min_height(px(CONTROL))
        .rules(chosen(on))
        .tagged(Mood::Plain)
        .on_activate(act)
        .boxed()
}

/// Choices in rows of no more than `most`, so a narrow panel holds
/// them.
fn rows(mut choices: Vec<Ui>, most: usize) -> Ui {
    let mut lines = Vec::new();
    while !choices.is_empty() {
        let rest = choices.split_off(most.min(choices.len()));
        lines.push(row(choices).gap(6.0).boxed());
        choices = rest;
    }
    column(lines).gap(6.0).boxed()
}

/// The choices of a look, each pressed to take it.
fn wardrobe(chief: Entity, facts: &Facts) -> Ui {
    let look = facts.look;
    let cloaks = Cloak::ALL
        .into_iter()
        .zip(facts.cloaks)
        .map(|(cloak, colour)| {
            swatch(colour, look.cloak == cloak, move |world| {
                change(world, chief, |look| look.cloak = cloak)
            })
        })
        .collect::<Vec<_>>();
    let lights = Light::ALL
        .into_iter()
        .zip(facts.lights)
        .map(|(light, colour)| {
            swatch(colour, look.light == light, move |world| {
                change(world, chief, |look| look.light = light)
            })
        })
        .collect::<Vec<_>>();
    let hats = Hat::ALL
        .into_iter()
        .map(|hat| {
            word(hat.name(), look.hat == hat, move |world| {
                change(world, chief, |look| look.hat = hat)
            })
        })
        .collect::<Vec<_>>();
    let held = Holds::ALL
        .into_iter()
        .map(|holds| {
            word(holds.name(), look.holds == holds, move |world| {
                change(world, chief, |look| look.holds = holds)
            })
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
    .boxed()
}

/// A chief: who they are, where their mascot is, and their look
/// with every choice of it.
pub fn panel(chief: Entity) -> Ui {
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
                back("ALL HAMLETS", |world| select(world, None)),
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

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(spoke)
        .add_systems(Startup, enter)
        .add_systems(Update, (keep, keep_place));
}

#[cfg(test)]
mod tests {
    use bevy::app::{App, Update};
    use hamlet_core::session::SessionKey;

    use super::*;

    fn app() -> (App, Entity, Entity) {
        let mut app = App::new();
        let world = app.world_mut();
        let agent = world.spawn(Name::new("Juniper")).id();
        let session = world.spawn_empty().id();
        world.insert_resource(Me::of(
            session,
            agent,
            "wren",
            Some("Juniper"),
        ));
        world.init_resource::<Selected>();
        let chief = chief::arrive(world, "wren", Look::default());
        app.add_systems(Update, accompany).add_observer(spoke);
        (app, chief, agent)
    }

    fn beside(app: &App, chief: Entity) -> Option<Entity> {
        app.world().get::<Beside>(chief).unwrap().0
    }

    #[test]
    fn the_mascot_keeps_whoever_is_selected_company() {
        let (mut app, chief, agent) = app();
        app.update();
        assert_eq!(beside(&app, chief), None);

        let pick = |app: &mut App, thing: Option<Thing>| {
            app.world_mut().resource_mut::<Selected>().0 = thing;
            app.update();
        };
        pick(&mut app, Some(Thing::Resident(agent)));
        assert_eq!(beside(&app, chief), Some(agent));
        pick(&mut app, Some(Thing::ResidentHouse(agent)));
        assert_eq!(beside(&app, chief), Some(agent));

        // Its own place, a hamlet or nothing at all: it goes home.
        pick(&mut app, Some(Thing::ChiefPlace(chief)));
        assert_eq!(beside(&app, chief), None);
        pick(&mut app, Some(Thing::Resident(agent)));
        pick(&mut app, None);
        assert_eq!(beside(&app, chief), None);
    }

    #[test]
    fn a_message_with_nobody_selected_brings_the_mascot() {
        let (mut app, chief, agent) = app();
        app.update();
        let key = app.world().resource::<Me>().key.clone();
        // Another chat's message is not this chief's.
        app.world_mut().trigger(UserMessage {
            key: SessionKey::with_agent("app", "wren", "Sorrel"),
            user: "wren".into(),
            text: "hello".into(),
        });
        assert_eq!(beside(&app, chief), None);
        app.world_mut().trigger(UserMessage {
            key,
            user: "wren".into(),
            text: "hello".into(),
        });
        assert_eq!(beside(&app, chief), Some(agent));
    }

    #[test]
    fn a_changed_look_is_kept_and_a_new_one_is_not_written() {
        let home = hamlet_testing::Scratch::new("chief-look");
        let mut app = App::new();
        let world = app.world_mut();
        world.insert_resource(HomeDir(hamlet_home::Home {
            root: home.to_path_buf(),
            workspace: home.join("workspace"),
        }));
        let chief = chief::arrive(world, "wren", Look::default());
        app.add_systems(Update, keep);
        app.update();
        assert!(!Look::file(&home, "wren").exists());

        change(app.world_mut(), chief, |look| {
            look.hat = Hat::Crown;
            look.light = Light::Moon;
        });
        app.update();
        let kept = Look::read(&home, "wren");
        assert_eq!(kept.hat, Hat::Crown);
        assert_eq!(kept.light, Light::Moon);
        assert_eq!(kept.cloak, Cloak::default());
    }
}

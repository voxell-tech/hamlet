//! The chief in the panels (docs/chief.md): the company its mascot
//! keeps, the look kept between launches, and the chief's page of the
//! Hub.

use bevy::app::{App, PreUpdate, Startup, Update};
use bevy::color::{Alpha, Color, Mix};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{
    AlignItems, BoxShadow, Overflow, PositionType, ShadowStyle,
    UiRect, Val, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, Frame, FrameProps, button, column, frame, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, component, derived, keyed, style,
};
use hamlet_client::ViewFolder;
use hamlet_client::messages::ChiefMessage;
use hamlet_client::realm::Identity;
use hamlet_realm::chief::{
    self, Beside, Chief, Cloak, Hat, Holds, Kept, Light, Look, Player,
};
use hamlet_realm::generator::society::Ground;
use hamlet_realm::library::{Library, Stood};
use hamlet_realm::moving;
use hamlet_realm::palette::Palette;
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};
use hamlet_ui::styles::{
    Fills, action, breadcrumb, choice, description, filling, group,
    note, scroll_area, section, segmented_words, status_label, title,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::PanelSystems;
use crate::convo::Convo;
use crate::roll::{agents_moved, either, moved};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(spoke)
        .add_systems(Startup, enter)
        // The chief enters under the name just followed, and is there
        // before the mascot is sent, which moves only when the chat
        // or the selection does.
        .add_systems(
            PreUpdate,
            enter
                .run_if(resource_changed::<Convo>)
                .after(crate::convo::chief_name)
                .before(accompany)
                .in_set(PanelSystems::Follow),
        )
        .add_systems(Update, keep);
}

/// Brings the chief this window belongs to into the world, with the
/// look that was kept for them.
pub(crate) fn enter(world: &mut World) {
    let name = world.resource::<Convo>().name.clone();
    if name.is_empty() {
        return;
    }
    if let Some(chief) = chief::named(world, &name) {
        world.entity_mut(chief).insert(Player);
        return;
    }
    let kept = world.resource::<Kept>().clone();
    let chief = chief::arrive(world, &name, kept.look(&name));
    world.entity_mut(chief).insert(Player);
    if let Some(stands) = kept.place(&name) {
        world.entity_mut(chief).insert(stands);
    }
    // The place the library was last set down, too.
    if let (Some(at), Some(mut stood)) =
        (kept.library, world.get_resource_mut::<Stood>())
    {
        stood.0 = Some(at);
    }
}

/// Takes the place of this window's chief in hand, to be set down
/// with a click on the ground.
pub(crate) fn carry_home(world: &mut World) {
    moving::pick_up(world, Ground::Chief);
}

/// Selects the place of the chief this window belongs to, and
/// brings it into view.
pub(crate) fn go_home(world: &mut World) {
    let name = world.resource::<Convo>().name.clone();
    let Some(chief) = chief::named(world, &name) else {
        return;
    };
    let thing = Thing::ChiefPlace(chief);
    select(world, Some(thing));
    bring_into_view(world, thing);
}

/// The company of the mascot of a chief who has chosen `selected`,
/// when the chat is with `agent`: that agent while it or its house is
/// selected, and a hamlet while its common is.
fn company(selected: Option<Thing>, agent: Entity) -> Option<Entity> {
    match selected {
        Some(
            Thing::Keeper
            | Thing::Cottage
            | Thing::Agent(_)
            | Thing::House(_),
        ) => Some(agent),
        Some(Thing::Common(hamlet)) => Some(hamlet),
        _ => None,
    }
}

/// Sends the mascot to whoever the chat is pointed at, and home when
/// nobody is selected.
pub(crate) fn accompany(
    convo: Res<Convo>,
    selected: Res<Selected>,
    libraries: Query<Entity, With<Library>>,
    mut chiefs: Query<(&Chief, &mut Beside)>,
) {
    if !convo.is_changed() && !selected.is_changed() {
        return;
    }
    // To the library when that is what was chosen.
    let with = match selected.0 {
        Some(Thing::Library) => libraries.iter().next(),
        chosen => company(chosen, convo.agent),
    };
    for (chief, mut beside) in &mut chiefs {
        if chief.name == convo.name && beside.0 != with {
            beside.0 = with;
        }
    }
}

/// A message sent with nobody selected brings the mascot from home
/// to whoever it was sent to.
pub(crate) fn spoke(
    message: On<ChiefMessage>,
    convo: Res<Convo>,
    identities: Query<&Identity>,
    mut chiefs: Query<(&Chief, &mut Beside)>,
) {
    if identities.get(convo.agent).map(|identity| identity.0)
        != Ok(message.to)
    {
        return;
    }
    for (chief, mut beside) in &mut chiefs {
        if chief.name == convo.name && beside.0.is_none() {
            beside.0 = Some(convo.agent);
        }
    }
}

/// Writes a look that was changed to its chief's file.
pub fn keep(
    view: Res<ViewFolder>,
    mut kept: ResMut<Kept>,
    looks: Query<(&Chief, Ref<Look>)>,
) {
    for (chief, look) in &looks {
        if look.is_changed() && !look.is_added() {
            let _ = look.write(&view.0, &chief.name);
            kept.keep_look(&chief.name, *look);
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
    tints: Tints,
}

/// The colours of the figure's face, hair, hats and things held, as
/// the mascot wears them.
#[derive(Clone, Copy, PartialEq)]
struct Tints {
    face: Color,
    blush: Color,
    hair: Color,
    mantle: Color,
    brass: Color,
    leaves: Color,
    quill: Color,
    ink: Color,
    china: Color,
    leather: Color,
    page: Color,
}

impl Tints {
    /// The trim of a cloak of `cloak` and of its hat.
    fn trim(&self, cloak: Color) -> Color {
        self.mantle.mix(&cloak, 0.25)
    }
}

fn facts(world: &World, chief: Entity) -> Option<Facts> {
    let name = world.get::<Name>(chief)?.as_str().to_string();
    let look = *world.get::<Look>(chief)?;
    let with = world
        .get::<Beside>(chief)
        .and_then(|beside| beside.0)
        .map(|agent| hamlet_client::realm::shown_of(world, agent));
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
        tints: Tints {
            face: shown(pal.face),
            blush: shown(pal.blush),
            hair: shown(pal.hair),
            mantle: shown(pal.mantle),
            brass: shown(pal.brass),
            leaves: shown(pal.foliage_shade),
            quill: shown(pal.autumn),
            ink: shown(pal.ink),
            china: shown(pal.whitewash),
            leather: shown(pal.leather),
            page: shown(pal.page),
        },
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

/// The side of a swatch.
const SWATCH: f32 = 28.0;

/// The width of a swatch with its name under it.
const SWATCH_WIDTH: f32 = 50.0;

/// The width of the ring about a chosen swatch, and of the gap
/// inside it.
const RING: f32 = 2.0;

/// The side of the chief's portrait, which is drawn in hundredths
/// of it.
const FIGURE: f32 = 150.0;

/// `units` hundredths of the portrait, in pixels.
fn u(units: f32) -> f32 {
    units * FIGURE / 100.0
}

/// A disc of `colour` with `name` under it, to press, a choice like
/// those of words. The one chosen has a ring about its disc and its
/// name lit.
fn swatch(
    colour: Color,
    name: &'static str,
    on: bool,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    with_cx(move |cx| {
        let ring = if on {
            cx.theme().palette.accent
        } else {
            Color::NONE
        };
        let word = label(name).wrap(false);
        let word = if on { word.tone(Tone::Accent) } else { word };
        // Regular, not the bold of a choice's words, so that the
        // longest name fits its swatch's width.
        let word = word.rules(|cx: &mut Cx| {
            style::<HamletTheme>()
                .label(|l, _| l.bold(false))
                .apply(cx);
        });
        button(
            column((
                row((frame()
                    .width(px(SWATCH))
                    .height(px(SWATCH))
                    .fill(colour)
                    .radius(SWATCH / 2.0),))
                .padding(UiRect::all(px(RING)))
                .border(RING)
                .border_color(ring)
                .radius(SWATCH / 2.0 + 2.0 * RING),
                word,
            ))
            .gap(2.0)
            .align(AlignItems::Center),
        )
        .width(px(SWATCH_WIDTH))
        .padding(UiRect::vertical(px(4.0)))
        .rules(choice(false))
        .on_activate(act)
    })
    .boxed()
}

/// `swatches` side by side, wrapping where the panel is narrow. A
/// swatch's name is its own room, so they touch.
fn swatches(swatches: Vec<AnyView>) -> impl View {
    row(swatches)
        .gap(0.0)
        .align(AlignItems::Center)
        .tagged(Wraps)
}

/// A part of the wardrobe: its name over its choices, and what it
/// colours besides the chief, if anything.
fn part(
    name: &'static str,
    choices: impl View,
    also: Option<&'static str>,
) -> AnyView {
    match also {
        Some(also) => {
            group((section(name), choices, note(also))).boxed()
        }
        None => group((section(name), choices)).boxed(),
    }
}

/// A rounded block of the portrait, `dx` right of its middle and
/// `top` down from its top, `width` by `height`, of `colour`.
fn block(
    dx: f32,
    top: f32,
    width: f32,
    height: f32,
    colour: Color,
) -> Frame {
    frame()
        .width(px(u(width)))
        .height(px(u(height)))
        .radius(u(width.min(height) / 2.0))
        .fill(colour)
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(u(50.0 + dx - width / 2.0)),
            top: px(u(top)),
            right: Val::Auto,
            bottom: Val::Auto,
        })
}

/// A ring of the portrait, as [`block`] places one, its line
/// `colour`.
fn hoop(
    dx: f32,
    top: f32,
    width: f32,
    height: f32,
    colour: Color,
) -> Frame {
    block(dx, top, width, height, Color::NONE)
        .border(u(2.5))
        .border_color(colour)
}

/// What sits on the head, drawn over the face but for a hood, which
/// is behind it.
fn hat(hat: Hat, cloak: Color, tints: &Tints) -> Vec<Frame> {
    let felt = cloak.mix(&Color::BLACK, 0.2);
    let trim = tints.trim(cloak);
    match hat {
        Hat::Pointed => vec![
            block(9.0, 2.0, 7.0, 8.0, felt),
            block(6.0, 6.0, 11.0, 9.0, felt).radius(u(3.0)),
            block(3.0, 11.0, 19.0, 10.0, felt).radius(u(3.0)),
            block(1.0, 17.0, 28.0, 11.0, felt).radius(u(3.0)),
            block(0.0, 24.0, 34.0, 5.0, trim).radius(u(2.0)),
            block(0.0, 29.0, 58.0, 7.0, felt),
            block(12.0, 0.0, 7.0, 7.0, tints.brass),
        ],
        Hat::Crown => vec![
            block(-10.0, 23.0, 6.0, 10.0, tints.brass).radius(u(1.5)),
            block(0.0, 21.0, 6.0, 12.0, tints.brass).radius(u(1.5)),
            block(10.0, 23.0, 6.0, 10.0, tints.brass).radius(u(1.5)),
            block(0.0, 29.0, 34.0, 7.0, tints.brass).radius(u(1.5)),
        ],
        Hat::Wreath => {
            vec![hoop(0.0, 29.0, 42.0, 12.0, tints.leaves)]
        }
        Hat::Hood | Hat::Bare => Vec::new(),
    }
}

/// What is held, and the hand that holds it, by the shoulder.
fn held(holds: Holds, tints: &Tints) -> Vec<Frame> {
    let hand = block(31.0, 79.0, 12.0, 12.0, tints.face);
    match holds {
        Holds::Quill => vec![
            block(34.0, 50.0, 8.0, 28.0, tints.quill),
            block(34.0, 76.0, 2.5, 10.0, tints.ink),
            hand,
        ],
        Holds::Teacup => vec![
            hoop(40.0, 71.0, 9.0, 9.0, tints.china),
            block(31.0, 69.0, 17.0, 13.0, tints.china).radius(u(3.0)),
            hand,
        ],
        Holds::Book => vec![
            block(32.0, 61.0, 18.0, 22.0, tints.leather)
                .radius(u(2.0)),
            block(35.0, 63.0, 5.0, 18.0, tints.page).radius(u(1.0)),
            hand,
        ],
        Holds::Nothing => Vec::new(),
    }
}

/// The chief as a portrait of its mascot: the wick and its glow over
/// its head, its hat, its face and the cloak on its shoulders, and
/// what it holds.
fn figure(facts: &Facts) -> impl View {
    let light = facts.lights[facts.look.light as usize];
    let cloak = facts.cloaks[facts.look.cloak as usize];
    let (tints, look) = (facts.tints, facts.look);
    with_cx(move |cx| {
        // Over a pointed hat the wick floats by its tip.
        let (wick, high) = match look.hat {
            Hat::Pointed => (26.0, 10.0),
            _ => (0.0, 13.0),
        };
        let glow = BoxShadow(vec![ShadowStyle {
            color: light.with_alpha(0.9),
            x_offset: px(0.0),
            y_offset: px(0.0),
            spread_radius: px(u(8.0)),
            blur_radius: px(u(30.0)),
        }]);
        let flame = block(wick, high - 7.0, 11.0, 15.0, light)
            .tagged(glow)
            .boxed();
        let mut parts = vec![
            block(
                wick,
                high - 1.0,
                5.0,
                7.0,
                light.mix(&Color::WHITE, 0.6),
            ),
            block(0.0, 68.0, 86.0, 70.0, cloak),
        ];
        match look.hat {
            Hat::Hood => {
                parts.push(block(0.0, 25.0, 54.0, 54.0, cloak));
            }
            Hat::Bare => {
                parts.push(block(0.0, 28.0, 42.0, 22.0, tints.hair));
            }
            _ => {}
        }
        parts.extend([
            block(0.0, 67.0, 36.0, 9.0, tints.trim(cloak)),
            block(0.0, 72.0, 7.0, 7.0, tints.brass),
            block(0.0, 31.0, 40.0, 38.0, tints.face),
        ]);
        for side in [-1.0, 1.0] {
            parts.push(block(side * 8.0, 45.0, 5.0, 8.0, tints.ink));
            parts.push(block(
                side * 13.0,
                55.0,
                7.0,
                4.0,
                tints.blush.with_alpha(0.8),
            ));
        }
        parts.extend(hat(look.hat, cloak, &tints));
        parts.extend(held(look.holds, &tints));
        let palette = &cx.theme().palette;
        let views = std::iter::once(flame)
            .chain(parts.into_iter().map(ViewExt::boxed))
            .collect::<Vec<_>>();
        column(views)
            .width(px(FIGURE))
            .height(px(FIGURE))
            .shrink(0.0)
            .overflow(Overflow::clip())
            .radius(cx.theme().space.round * 3.0)
            .fill(palette.input)
            .border(cx.theme().space.border)
            .border_color(palette.gold_6)
    })
}

/// The chief's figure beside its name, where it is, and the way to
/// it in the world.
fn portrait(facts: &Facts) -> impl View {
    let (place, mark, tone) = match &facts.with {
        Some(agent) => {
            (format!("With {agent}"), Glyph::Robot, Tone::Accent)
        }
        None => ("At home".to_string(), Glyph::Hearth, Tone::Dim),
    };
    let words = column((
        title(facts.name.clone()),
        description(
            "This is you, the chief. What you choose here is how \
             you look in the world.",
        ),
        status_label(mark, place, tone),
        // In a row of its own, so it is as wide as its words.
        row((action(Glyph::Aim, "Find me in the world", go_home),)),
    ))
    .gap(6.0)
    .grow(1.0)
    .min_width(px(0.0));
    let figure = figure(facts);
    with_cx(move |cx| {
        row((figure, words))
            .gap(cx.theme().space.section)
            .align(AlignItems::FlexStart)
    })
}

/// The choices of a look, each pressed to take it.
fn wardrobe(chief: Entity, facts: &Facts) -> impl View + use<> {
    let look = facts.look;
    let cloaks = Cloak::ALL
        .into_iter()
        .zip(facts.cloaks)
        .map(|(cloak, colour)| {
            swatch(
                colour,
                cloak.name(),
                look.cloak == cloak,
                move |world| {
                    change(world, chief, |look| look.cloak = cloak);
                },
            )
        })
        .collect::<Vec<_>>();
    let lights = Light::ALL
        .into_iter()
        .zip(facts.lights)
        .map(|(light, colour)| {
            swatch(
                colour,
                light.name(),
                look.light == light,
                move |world| {
                    change(world, chief, |look| look.light = light);
                },
            )
        })
        .collect::<Vec<_>>();
    let hats = segmented_words(
        Hat::ALL.map(Hat::name),
        Hat::ALL
            .iter()
            .position(|hat| *hat == look.hat)
            .unwrap_or(0),
        move |world, at| {
            change(world, chief, |look| look.hat = Hat::ALL[at]);
        },
    );
    let held = segmented_words(
        Holds::ALL.map(Holds::name),
        Holds::ALL
            .iter()
            .position(|holds| *holds == look.holds)
            .unwrap_or(0),
        move |world, at| {
            change(world, chief, |look| look.holds = Holds::ALL[at]);
        },
    );
    with_cx(move |cx| {
        column((
            part(
                "Cloak",
                swatches(cloaks),
                Some("Your roof at home takes this colour too."),
            ),
            part(
                "Light",
                swatches(lights),
                Some(
                    "The flame over your head. The orb on your roof \
                     shines this colour too.",
                ),
            ),
            part("Hat", hats, None),
            part("Hand", held, None),
        ))
        .gap(cx.theme().space.section)
    })
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
        let views = (
            breadcrumb(
                vec!["Realm".into()],
                facts.name.clone(),
                |world, _| select(world, None),
            ),
            scroll_area((portrait(facts), wardrobe(chief, facts))),
        );
        with_cx(move |cx| {
            column(views).fills_height().gap(cx.theme().space.section)
        })
        .boxed()
    })
    .within(filling())
    .boxed()
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use hamlet_protocol::client as protocol;

    use super::*;

    #[test]
    fn the_chief_arrives_with_the_servers_name_after_the_window_opens()
     {
        use hamlet_client::{ClientPlugin, ViewFolder};
        use hamlet_testing::Scratch;

        let view = Scratch::new("chief-late-name");
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None })
            .insert_resource(ViewFolder(view.to_path_buf()))
            .init_resource::<Kept>()
            .init_resource::<Stood>()
            .add_plugins((crate::convo::plugin, plugin));
        app.update();
        assert!(chief::named(app.world_mut(), "").is_none());
        let mut state = hamlet_testing::realm::state();
        state.chief = "wren".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        let chief = chief::named(app.world_mut(), "wren").unwrap();
        assert!(app.world().get::<Player>(chief).is_some());
    }

    #[test]
    fn the_chiefs_look_returns_from_the_view_folder_at_launch() {
        use hamlet_client::ViewFolder;
        use hamlet_realm::chief::Stands;
        use hamlet_testing::Scratch;

        let dir = Scratch::new("chief-look");
        let stands = Stands(bevy::math::Vec2::new(123.5, -96.0));
        stands.write(&dir.join("view"), "wren").unwrap();
        let library = Stands(bevy::math::Vec2::new(-400.0, 72.5));
        library.write_at(&Stood::file(&dir.join("view"))).unwrap();
        let make = || {
            let mut app = App::new();
            let agent = app.world_mut().spawn(Name::new("ada")).id();
            let chat = app.world_mut().spawn_empty().id();
            app.insert_resource(ViewFolder(dir.join("view")))
                .insert_resource(
                    Kept::read(&dir.join("view")).unwrap(),
                )
                .insert_resource(Convo::of(chat, agent, "wren", None))
                .init_resource::<Stood>()
                .add_plugins(plugin);
            app
        };
        let mut app = make();
        app.update();
        assert!(!dir.join("view/chiefs/wren.toml").exists());
        let chief = chief::named(app.world_mut(), "wren").unwrap();
        let look = app
            .world_mut()
            .get_mut::<Look>(chief)
            .unwrap()
            .into_inner();
        look.hat = Hat::Crown;
        look.light = Light::Moon;
        app.update();
        assert!(dir.join("view/chiefs/wren.toml").exists());
        assert!(!dir.join("chiefs").exists());

        let mut relaunched = make();
        relaunched.update();
        let chief =
            chief::named(relaunched.world_mut(), "wren").unwrap();
        assert_eq!(
            relaunched.world().get::<Stands>(chief),
            Some(&stands)
        );
        assert_eq!(
            relaunched.world().resource::<Stood>().0,
            Some(library.0)
        );
        let look = relaunched.world().get::<Look>(chief).unwrap();
        assert_eq!(look.hat, Hat::Crown);
        assert_eq!(look.light, Light::Moon);
        assert_eq!(look.cloak, Cloak::default());
    }
}

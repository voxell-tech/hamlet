//! A place picked up and set down somewhere else. Setting down
//! changes nothing here: it is told as a [`SetDown`].

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hamlet_assets::tint;

use super::generator::land::{Land, Unfit};
use super::generator::layout::plot;
use super::generator::society::{Ground, Society};
use super::palette::Palette;
use super::pick::{Reach, Taken, Travel, WorldSurface};
use super::play::{Play, Played};
use super::{RealmSystems, label};
use crate::sky::CameraSettings;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Carrying>()
        .init_resource::<CarrySettings>()
        .add_message::<SetDown>()
        .add_observer(click)
        .add_systems(Startup, prepare)
        .add_systems(
            Update,
            (
                keys,
                reach,
                (
                    show.run_if(
                        resource_changed::<Carrying>
                            .or_else(resource_changed::<Society>)
                            .or_else(resource_changed::<Land>)
                            .or_else(
                                resource_changed::<CarrySettings>,
                            ),
                    ),
                    tell,
                ),
            )
                .chain()
                .in_set(RealmSystems::Act),
        );
}

/// A place in a chief's hand: how the footprint that shows where it
/// would stand is drawn.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarrySettings {
    /// Height of the footprint over the ground.
    pub lift: f32,
    /// Strength of its glow, over its colour.
    pub glow: f32,
    /// The lights its rim is made of: how many, and how wide each
    /// is.
    pub beads: u32,
    pub bead: f32,
    /// The width of the ring that marks a common, as a share of the
    /// ring's radius.
    pub band: f32,
    /// Radius and height of the light over its middle.
    pub beam: (f32, f32),
    /// Thickness of the mark where a house would stand.
    pub pad: f32,
    /// The words beside the pointer: where from the pointer, in
    /// pixels, their size and what they are written on.
    pub word_at: Vec2,
    pub word_size: f32,
    #[serde(with = "tint")]
    pub word_back: Color,
}

/// A place in a chief's hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub place: Ground,
    /// The ground under the pointer, while the pointer is over any.
    pub to: Option<Vec2>,
    /// `Ok` when it could be set down there.
    pub fits: Result<(), Unfit>,
}

/// The place a chief is carrying, if any.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Carrying(pub Option<Carry>);

/// A chief set a place down at `to`, where it fits.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct SetDown {
    pub place: Ground,
    pub to: Vec2,
}

/// Takes `place` in hand, to be set down with the next click on the
/// ground.
pub fn pick_up(world: &mut World, place: Ground) {
    let at = world.get_resource::<Land>().and_then(|land| {
        let society = world.get_resource::<Society>()?;
        let (at, _) = society.stands(land, place)?;
        Some(land.on_ground(at))
    });
    let Some(at) = at else {
        return;
    };
    world.resource_mut::<Carrying>().0 = Some(Carry {
        place,
        to: None,
        fits: Err(Unfit::Crowded),
    });
    world.write_message(Played {
        what: Play::Lift,
        at,
    });
}

/// The footprint of a carried place, where it would stand.
#[derive(Component)]
struct Ghost;

/// A part of the ghost.
#[derive(Component, Clone, Copy, PartialEq)]
enum Mark {
    /// One of the lights about the round of ground the place holds.
    Rim(u32),
    /// A town's common.
    Common,
    /// A light that stands over its middle.
    Beam,
    /// The ground one house of a town takes.
    House(u32),
}

/// The few words beside the pointer.
#[derive(Component)]
struct Word;

#[derive(Resource)]
struct Lights {
    fits: Handle<StandardMaterial>,
    unfit: Handle<StandardMaterial>,
}

fn prepare(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    land: Res<Land>,
    pal: Res<Palette>,
    set: Res<CarrySettings>,
) {
    let mut light = |colour: Vec3| {
        materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::from_vec3(colour * set.glow),
            ..default()
        })
    };
    let lights = Lights {
        fits: light(pal.place_fits),
        unfit: light(pal.place_unfit),
    };
    let band = meshes.add(Torus::new(1.0 - set.band, 1.0));
    let beam = meshes.add(Cylinder::new(set.beam.0, set.beam.1));
    let pad = meshes.add(Cylinder::new(1.0, set.pad));
    let ghost = commands
        .spawn((Ghost, Transform::default(), Visibility::Hidden))
        .id();
    let part = |mark: Mark, mesh: &Handle<Mesh>| {
        (
            mark,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(lights.fits.clone()),
            NotShadowCaster,
            Transform::default(),
            Visibility::Inherited,
            ChildOf(ghost),
        )
    };
    for i in 0..set.beads {
        commands.spawn(part(Mark::Rim(i), &pad));
    }
    commands.spawn(part(Mark::Common, &band));
    commands.spawn(part(Mark::Beam, &beam));
    for k in 0..land.set.town.plots {
        commands.spawn(part(Mark::House(k), &pad));
    }
    commands.spawn((
        Word,
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(8), px(3)),
            ..default()
        },
        BackgroundColor(set.word_back),
        Pickable::IGNORE,
        Visibility::Hidden,
        children![(
            Text::new(""),
            TextFont::from_font_size(set.word_size),
            Pickable::IGNORE,
        )],
    ));
    commands.insert_resource(lights);
}

/// The point the ray from the eye through the pointer meets the
/// ground at.
fn ground_under(land: &Land, ray: Ray3d) -> Option<Vec2> {
    let down = -ray.direction.y;
    if down < 0.02 {
        return None;
    }
    // From level ground, a few steps toward the ground as it lies.
    let mut far = (ray.origin.y - land.set.land.site_y) / down;
    for _ in 0..4 {
        let p = ray.origin + ray.direction * far;
        far = (ray.origin.y - land.height(p.xz())) / down;
    }
    (far > 0.0).then(|| (ray.origin + ray.direction * far).xz())
}

/// Follows the pointer over the ground with the carried place, and
/// finds whether it fits there.
fn reach(
    reach: Res<Reach>,
    land: Res<Land>,
    society: Res<Society>,
    mut carrying: ResMut<Carrying>,
) {
    let Some(carry) = carrying.0 else {
        return;
    };
    // A place that is gone is in nobody's hand.
    if society.stands(&land, carry.place).is_none() {
        carrying.0 = None;
        return;
    }
    let to = reach.ray.and_then(|ray| ground_under(&land, ray));
    let fits = match to {
        Some(to) => society.fits(&land, carry.place, to),
        None => Err(Unfit::Crowded),
    };
    let next = Carry { to, fits, ..carry };
    if carrying.0 != Some(next) {
        carrying.0 = Some(next);
    }
}

/// Draws the footprint where the place would stand: its rim, a
/// light over its middle, and for a town its common and the ground
/// of each of its houses.
fn show(
    carrying: Res<Carrying>,
    land: Res<Land>,
    society: Res<Society>,
    set: Res<CarrySettings>,
    lights: Res<Lights>,
    mut ghost: Query<
        (&mut Transform, &mut Visibility),
        (With<Ghost>, Without<Mark>),
    >,
    mut marks: Query<(
        &Mark,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Ok((mut at, mut seen)) = ghost.single_mut() else {
        return;
    };
    let shown = carrying.0.and_then(|carry| {
        let (_, radius) = society.stands(&land, carry.place)?;
        Some((carry, carry.to?, radius))
    });
    let Some((carry, to, radius)) = shown else {
        seen.set_if_neq(Visibility::Hidden);
        return;
    };
    seen.set_if_neq(Visibility::Visible);
    let level = land.set.land.site_y;
    // Over the ground as it lies now and as it would be levelled.
    let over = |p: Vec2| land.surface(p).max(level) + set.lift;
    *at = Transform::from_xyz(to.x, over(to), to.y);
    let town = match carry.place {
        Ground::Town(id) => society.town(id),
        _ => None,
    };
    // The plots as they would lie there.
    let there = town.map(|town| {
        let mut there = land.clone();
        there.move_site(town.site, to);
        (there, town)
    });
    let light = |good: bool| {
        if good {
            lights.fits.clone()
        } else {
            lights.unfit.clone()
        }
    };
    for (mark, mut local, mut seen, mut material) in &mut marks {
        let mut good = carry.fits.is_ok();
        let place = match *mark {
            // Each lies on the ground under it, so the rim shows
            // on a slope too.
            Mark::Rim(i) => {
                let turn = i as f32 / set.beads as f32 * TAU;
                let out = Vec2::from_angle(turn) * radius;
                Some(
                    Transform::from_xyz(
                        out.x,
                        over(to + out) - at.translation.y,
                        out.y,
                    )
                    .with_scale(Vec3::new(set.bead, 1.0, set.bead)),
                )
            }
            Mark::Beam => Some(Transform::from_translation(
                Vec3::Y * set.beam.1 * 0.5,
            )),
            Mark::Common => town.map(|_| {
                let r = land.set.town.plaza_radius;
                Transform::from_scale(Vec3::new(r, 1.0, r))
            }),
            Mark::House(k) => there
                .as_ref()
                .filter(|(_, town)| {
                    society.agents.iter().any(|a| {
                        a.town == town.id && a.plot == k && !a.cleared
                    })
                })
                .map(|(there, town)| {
                    let spot = plot(there, town.site, k);
                    good &= spot.usable;
                    let r = land.set.town.house_radius;
                    Transform::from_xyz(
                        spot.at.x - to.x,
                        over(spot.at) - at.translation.y,
                        spot.at.y - to.y,
                    )
                    .with_scale(Vec3::new(r, 1.0, r))
                }),
        };
        let want = match place {
            Some(_) => Visibility::Inherited,
            None => Visibility::Hidden,
        };
        seen.set_if_neq(want);
        if let Some(place) = place
            && *local != place
        {
            *local = place;
        }
        let light = light(good);
        if material.0 != light {
            material.0 = light;
        }
    }
}

/// Says beside the pointer what a click would do, or why the place
/// cannot stand there.
fn tell(
    carrying: Res<Carrying>,
    reach: Res<Reach>,
    set: Res<CarrySettings>,
    pal: Res<Palette>,
    mut word: Query<
        (&mut Node, &mut Visibility, &Children),
        With<Word>,
    >,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Ok((mut node, mut seen, children)) = word.single_mut() else {
        return;
    };
    let said = carrying.0.zip(reach.at);
    let Some((carry, at)) = said else {
        seen.set_if_neq(Visibility::Hidden);
        return;
    };
    seen.set_if_neq(Visibility::Visible);
    label::put(&mut node, at + set.word_at);
    let (says, colour) = match (carry.to, carry.fits) {
        (None, _) => ("Point at the ground", pal.place_unfit),
        (_, Ok(())) => {
            ("Click to set down. Esc to put back", pal.place_fits)
        }
        (_, Err(Unfit::Water)) => {
            ("Not in the water", pal.place_unfit)
        }
        (_, Err(Unfit::Steep)) => ("Too steep here", pal.place_unfit),
        (_, Err(Unfit::Crowded)) => {
            ("Too close to another place", pal.place_unfit)
        }
    };
    let colour = Color::LinearRgba(LinearRgba::from_vec3(colour));
    for child in children {
        if let Ok((mut text, mut ink)) = texts.get_mut(*child) {
            if text.0 != says {
                text.0 = says.to_string();
            }
            if ink.0 != colour {
                ink.0 = colour;
            }
        }
    }
}

/// A click on the world sets the place down where it fits. The
/// right button puts it back.
fn click(
    click: On<Pointer<Click>>,
    surfaces: Query<(), With<WorldSurface>>,
    travel: Res<Travel>,
    camera: Res<CameraSettings>,
    land: Res<Land>,
    mut carrying: ResMut<Carrying>,
    mut set_down: MessageWriter<SetDown>,
    mut played: MessageWriter<Played>,
) {
    let Some(carry) = carrying.0 else {
        return;
    };
    if !surfaces.contains(click.event_target())
        || travel.0 > camera.click_slop
    {
        return;
    }
    match (click.button, carry.to, carry.fits) {
        (PointerButton::Primary, Some(to), Ok(())) => {
            set_down.write(SetDown {
                place: carry.place,
                to,
            });
            played.write(Played {
                what: Play::Drop,
                at: land.on_ground(to),
            });
            carrying.0 = None;
        }
        (PointerButton::Secondary, ..) => carrying.0 = None,
        _ => {}
    }
}

/// Escape puts the place back, and does nothing else that frame.
fn keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut taken: ResMut<Taken>,
    mut carrying: ResMut<Carrying>,
) {
    // Taken before the place is put back, so whoever else reads
    // Escape this frame leaves it alone, whichever runs first.
    let held = carrying.0.is_some();
    if taken.escape != held {
        taken.escape = held;
    }
    if held
        && keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
    {
        carrying.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::GeneratorSettings;
    use crate::generator::society::TownId;

    fn app() -> (App, TownId) {
        let mut app = App::new();
        let mut land = Land::new(
            1,
            GeneratorSettings::default(),
            Palette::default(),
        );
        let mut society = Society::default();
        let town = society.found_town(&mut land);
        society.found_town(&mut land);
        for _ in 0..4 {
            society.add_agent(&land, town);
        }
        app.insert_resource(land)
            .insert_resource(society)
            .init_resource::<Carrying>()
            .init_resource::<Reach>()
            .init_resource::<Travel>()
            .init_resource::<Taken>()
            .init_resource::<CameraSettings>()
            .add_message::<SetDown>()
            .add_message::<Played>()
            .add_observer(click)
            .add_systems(Update, (keys, reach).chain());
        (app, town)
    }

    /// Points at the ground at `at` from above and runs a frame.
    fn point(app: &mut App, at: Vec2) {
        let origin = Vec3::new(at.x, 200.0, at.y + 150.0);
        let toward = Vec3::new(at.x, 1.3, at.y) - origin;
        app.world_mut().resource_mut::<Reach>().ray =
            Some(Ray3d::new(origin, Dir3::new(toward).unwrap()));
        app.update();
    }

    fn press(app: &mut App, button: PointerButton) {
        use bevy::camera::NormalizedRenderTarget;
        use bevy::picking::backend::HitData;
        use bevy::picking::pointer::{Location, PointerId};
        let surface = app.world_mut().spawn(WorldSurface).id();
        let at = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        let event = Click {
            button,
            hit: HitData::new(surface, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            at,
            event,
            surface,
        ));
        app.update();
    }

    fn carried(app: &App) -> Option<Carry> {
        app.world().resource::<Carrying>().0
    }

    fn set_down(app: &mut App) -> Vec<SetDown> {
        app.world_mut()
            .resource_mut::<Messages<SetDown>>()
            .drain()
            .collect()
    }

    /// Open ground far from every town.
    fn meadow(app: &App) -> Vec2 {
        let land = app.world().resource::<Land>();
        (0..600)
            .map(|i| {
                Vec2::from_angle(i as f32) * (300.0 + i as f32 * 3.0)
            })
            .find(|&p| land.suits(p).is_ok())
            .unwrap()
    }

    #[test]
    fn a_town_follows_the_pointer_and_a_click_sets_it_down() {
        let (mut app, town) = app();
        let place = Ground::Town(town);
        pick_up(app.world_mut(), place);
        let to = meadow(&app);
        point(&mut app, to);
        let carry = carried(&app).unwrap();
        assert_eq!(carry.fits, Ok(()));
        assert!(carry.to.unwrap().distance(to) < 1.0);
        // The town itself stays until the record says it moved.
        let before = app.world().resource::<Land>().sites[0].at;
        press(&mut app, PointerButton::Primary);
        assert_eq!(carried(&app), None);
        let said = set_down(&mut app);
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].place, place);
        assert!(said[0].to.distance(to) < 1.0);
        assert_eq!(
            app.world().resource::<Land>().sites[0].at,
            before
        );
    }

    #[test]
    fn it_is_not_set_down_on_water_or_on_another_town() {
        let (mut app, town) = app();
        pick_up(app.world_mut(), Ground::Town(town));
        let land = app.world().resource::<Land>().clone();
        let river = (0..4000)
            .map(|i| Vec2::new(i as f32 * 0.5 - 1000.0, 40.0))
            .find(|&p| land.is_water(p))
            .unwrap();
        point(&mut app, river);
        assert_eq!(carried(&app).unwrap().fits, Err(Unfit::Water));
        press(&mut app, PointerButton::Primary);
        assert!(carried(&app).is_some());
        assert!(set_down(&mut app).is_empty());

        let other = land.sites[1].at + Vec2::new(9.0, 4.0);
        point(&mut app, other);
        assert_eq!(carried(&app).unwrap().fits, Err(Unfit::Crowded));
        press(&mut app, PointerButton::Primary);
        assert!(set_down(&mut app).is_empty());

        // The right button puts it back, and so does Escape.
        press(&mut app, PointerButton::Secondary);
        assert_eq!(carried(&app), None);
        pick_up(app.world_mut(), Ground::Town(town));
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Escape);
        app.insert_resource(keys);
        app.update();
        assert_eq!(carried(&app), None);
        // That Escape was the place's alone, and the next is not.
        assert!(app.world().resource::<Taken>().escape);
        app.update();
        assert!(!app.world().resource::<Taken>().escape);
        assert!(set_down(&mut app).is_empty());
    }

    #[test]
    fn a_drag_is_no_click() {
        let (mut app, town) = app();
        pick_up(app.world_mut(), Ground::Town(town));
        let to = meadow(&app);
        point(&mut app, to);
        app.world_mut().resource_mut::<Travel>().0 = 40.0;
        press(&mut app, PointerButton::Primary);
        assert!(carried(&app).is_some());
        assert!(set_down(&mut app).is_empty());
    }
}

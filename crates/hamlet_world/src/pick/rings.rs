//! The rings of light on the ground about what is pointed at and what
//! is chosen.

use bevy::prelude::*;
use hamlet_vfx::mark::{MarkMaterial, Marks, Shape, lie};

use super::{
    Also, Footprint, Hovered, PickSettings, Selected, Thing,
};
use crate::generator::Floor;

#[derive(Component)]
enum Ring {
    Hover,
    Selected,
    Also,
}

/// The ground a ring's mark is laid on: the thing it was taken
/// for, its middle and how far it reaches to each side.
#[derive(Component, Default)]
struct Ground(Option<(Thing, Vec2, f32)>);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(Update, rings);
}

/// Each ring is a mark of light on the ground and what stands on it.
/// A world with no marks, as a test's, has no rings.
fn spawn(
    mut commands: Commands,
    marks: Option<Res<Marks>>,
    materials: Option<ResMut<Assets<MarkMaterial>>>,
) {
    let (Some(marks), Some(mut materials)) = (marks, materials)
    else {
        return;
    };
    let rings = [
        (Ring::Hover, 1.7),
        (Ring::Selected, 6.5),
        (Ring::Also, 4.0),
    ];
    let candle = Vec3::new(1.0, 0.64, 0.25);
    for (ring, glow) in rings {
        let level = marks.level(Vec3::ZERO, 0.0);
        let light = marks.shine(Shape::BAND, candle * glow, &level);
        commands.spawn((
            ring,
            Ground::default(),
            marks.mark(materials.add(light)),
            Visibility::Hidden,
            Transform::default(),
        ));
    }
}

/// Lays a ring on the ground about what is pointed at and what is
/// chosen.
fn rings(
    hovered: Res<Hovered>,
    selected: Res<Selected>,
    also: Res<Also>,
    pick: Res<PickSettings>,
    things: Query<(&Thing, &Footprint, &GlobalTransform)>,
    mut rings: Query<
        (
            &Ring,
            &mut Ground,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<MarkMaterial>,
        ),
        Without<Thing>,
    >,
    mut floor: Floor,
    marks: Res<Marks>,
    mut materials: ResMut<Assets<MarkMaterial>>,
) {
    for (ring, mut ground, mut at, mut visibility, light) in
        &mut rings
    {
        let wanted = match ring {
            Ring::Hover => hovered.0.filter(|h| {
                selected.0 != Some(*h) && also.0 != Some(*h)
            }),
            Ring::Selected => selected.0,
            Ring::Also => also.0.filter(|a| selected.0 != Some(*a)),
        };
        let found = wanted.and_then(|w| {
            things.iter().find(|(thing, ..)| **thing == w)
        });
        let Some((thing, room, place)) = found else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let centre = place.translation();
        let level = centre.y - room.lift + pick.ring_lift;
        let lies = lie(
            Vec3::new(centre.x, level, centre.z),
            room.radius,
            0.0,
        );
        at.set_if_neq(lies);
        // The ground is looked up again only when the ring has left
        // what was last looked up.
        let reach = room.radius * 1.1;
        let own = ground.0.filter(|(of, ..)| of == thing);
        let held = own.is_some_and(|(_, middle, half)| {
            let off = (centre.xz() - middle).abs().max_element();
            off + reach <= half
        });
        if !held && let Some(mut light) = materials.get_mut(&light.0)
        {
            // Its own ground to begin with, and more about it once
            // it is seen to move.
            let half = match own {
                Some(_) => reach + pick.ring_slack,
                None => reach,
            };
            let was = light.heights().clone();
            light.lay(&floor.under(lies.translation, half));
            // The level ground is every mark's to begin on.
            if !marks.is_level(&was) {
                floor.forget(&was);
            }
            ground.0 = Some((*thing, centre.xz(), half));
        }
        visibility.set_if_neq(Visibility::Visible);
    }
}

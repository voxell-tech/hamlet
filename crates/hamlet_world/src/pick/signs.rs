//! The names shown over things: a sign over each named thing, and a
//! tag over whatever the pointer is on.

use bevy::prelude::*;
use bevy::ui::UiSystems;

use super::{Footprint, Hovered, PickSettings, Thing};
use crate::label::{self, Lens};

/// A thing whose name is shown over it all the time.
#[derive(Component)]
pub struct Named;

/// The name shown over a `Named` thing.
#[derive(Component)]
#[relationship(relationship_target = Signed)]
pub struct Sign(Entity);

/// The sign of a `Named` thing, which goes with it.
#[derive(Component)]
#[relationship_target(relationship = Sign, linked_spawn)]
pub struct Signed(Entity);

#[derive(Component)]
struct SignText;

/// The hover tag is left to whoever shows something fuller.
#[derive(Resource)]
pub struct NoNameTag;

/// The name shown over the hovered thing.
#[derive(Component)]
pub struct NameTag;

#[derive(Component)]
struct NameText;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        PostUpdate,
        (
            name_tag,
            signs,
            write_signs.run_if(
                any_match_filter::<Or<(Changed<Name>, Changed<Thing>)>>,
            ),
        )
            .before(UiSystems::Layout),
    );
}

fn spawn(mut commands: Commands) {
    commands.spawn((
        NameTag,
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(8), px(3)),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(0x01, 0x0a, 0x13)),
        BorderColor::all(Color::srgb_u8(0x78, 0x5a, 0x28)),
        Pickable::IGNORE,
        Visibility::Hidden,
        children![(
            NameText,
            Text::new(""),
            TextFont::from_font_size(12.0),
            TextColor(Color::srgb_u8(0xf0, 0xe6, 0xd2)),
            Pickable::IGNORE,
        )],
    ));
}

/// Keeps a sign over every `Named` thing.
fn signs(
    mut commands: Commands,
    names: Query<&Name>,
    lens: Lens,
    pick: Res<PickSettings>,
    new: Query<(Entity, &Thing), (With<Named>, Without<Signed>)>,
    named: Query<(&Footprint, &GlobalTransform), With<Named>>,
    mut signs: Query<(
        &Sign,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
    )>,
) {
    for (thing, what) in &new {
        commands.spawn((
            Sign(thing),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(px(6), px(1)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(0x01, 0x0a, 0x13, 0xd8)),
            BorderColor::all(Color::srgb_u8(0x46, 0x37, 0x14)),
            Pickable::IGNORE,
            Visibility::Hidden,
            children![(
                SignText,
                Text::new(what.sign(&names).to_uppercase()),
                TextFont::from_font_size(11.0),
                TextColor(Color::srgb_u8(0xcd, 0xbe, 0x91)),
                Pickable::IGNORE,
            )],
        ));
    }
    let (camera, view) = lens.camera();
    let eye = view.translation();
    let top = |place: &GlobalTransform, room: &Footprint| {
        place.translation() + Vec3::Y * (room.height - room.lift)
    };
    for (sign, mut node, mut visibility, computed) in &mut signs {
        // Its place is worked out only for one near enough to be
        // read.
        let point = named
            .get(sign.0)
            .ok()
            .filter(|(room, was)| {
                eye.distance(top(was, room)) < pick.signs_far
            })
            .and_then(|(room, was)| {
                let place = lens.stands(sign.0, was);
                camera
                    .world_to_viewport(&view, top(&place, room))
                    .ok()
            });
        let Some(point) = point else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        label::put(&mut node, label::above(computed, point, 4.0));
        visibility.set_if_neq(Visibility::Visible);
    }
}

/// Writes on each sign the name it shows.
fn write_signs(
    names: Query<&Name>,
    renamed: Query<(), Changed<Name>>,
    named: Query<(Ref<Thing>, &Signed)>,
    signs: Query<&Children, With<Sign>>,
    mut texts: Query<&mut Text, With<SignText>>,
) {
    let every = !renamed.is_empty();
    for (thing, signed) in &named {
        if !every && !thing.is_changed() {
            continue;
        }
        let Ok(parts) = signs.get(signed.0) else {
            continue;
        };
        let name = thing.sign(&names).to_uppercase();
        for part in parts.iter() {
            if let Ok(mut text) = texts.get_mut(part)
                && text.0 != name
            {
                text.0.clone_from(&name);
            }
        }
    }
}

fn name_tag(
    quiet: Option<Res<NoNameTag>>,
    hovered: Res<Hovered>,
    names: Query<&Name>,
    lens: Lens,
    things: Query<
        (Entity, &Thing, &Footprint, &GlobalTransform),
        Without<Named>,
    >,
    mut tag: Single<
        (&mut Node, &mut Visibility, &ComputedNode),
        With<NameTag>,
    >,
    mut text: Single<&mut Text, With<NameText>>,
) {
    let (camera, view) = lens.camera();
    let found = hovered.0.filter(|_| quiet.is_none()).and_then(|h| {
        things.iter().find(|(_, thing, ..)| **thing == h)
    });
    let top = found.and_then(|(entity, thing, room, was)| {
        let top = lens.stands(entity, was).translation()
            + Vec3::Y * (room.height - room.lift);
        camera
            .world_to_viewport(&view, top)
            .ok()
            .map(|p| (thing, p))
    });
    let Some((thing, point)) = top else {
        tag.1.set_if_neq(Visibility::Hidden);
        return;
    };
    let name = thing.name(&names);
    if text.0 != name {
        text.0 = name;
    }
    let corner = label::above(tag.2, point, 10.0);
    label::put(&mut tag.0, corner.max(Vec2::splat(4.0)));
    tag.1.set_if_neq(Visibility::Visible);
}

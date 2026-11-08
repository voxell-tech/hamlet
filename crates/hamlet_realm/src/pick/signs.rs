//! The names shown over things: a sign over each named thing, and a
//! tag over whatever the pointer is on.

use bevy::prelude::*;
use bevy::ui::UiSystems;
use hamlet_client::realm::Shown;

use super::{Footprint, Hovered, Names, PickSettings, Thing};
use crate::label::{self, Lens};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        PostUpdate,
        (
            name_tag,
            signs,
            write_signs.run_if(
                any_match_filter::<
                    Or<(
                        Changed<Name>,
                        Changed<Thing>,
                        Changed<Reads>,
                    )>,
                >,
            ),
        )
            .before(UiSystems::Layout),
    );
}

/// A thing whose name is shown over it all the time.
#[derive(Component)]
pub struct Named;

/// The words on the sign of a `Named` thing, in place of its name.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Reads(pub String);

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
    names: Names,
    lens: Lens,
    pick: Res<PickSettings>,
    new: Query<
        (Entity, &Thing, Option<&Reads>),
        (With<Named>, Without<Signed>),
    >,
    named: Query<(&Footprint, &GlobalTransform), With<Named>>,
    mut signs: Query<(
        &Sign,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
        &mut UiTransform,
        &mut ZIndex,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut texts: Query<&mut TextColor, With<SignText>>,
) {
    for (thing, what, reads) in &new {
        commands.spawn((
            Sign(thing),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(px(7), px(2)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(PLATE),
            BorderColor::all(EDGE),
            UiTransform::default(),
            ZIndex(-1),
            Pickable::IGNORE,
            Visibility::Hidden,
            children![(
                SignText,
                Text::new(words(*what, reads, &names)),
                TextFont::from_font_size(12.0),
                TextColor(INK),
                Pickable::IGNORE,
            )],
        ));
    }
    let (camera, view) = lens.camera();
    let eye = view.translation();
    let top = |place: &GlobalTransform, room: &Footprint| {
        place.translation() + Vec3::Y * (room.height - room.lift)
    };
    for (
        sign,
        mut node,
        mut visibility,
        computed,
        mut transform,
        mut order,
        mut plate,
        mut edge,
        children,
    ) in &mut signs
    {
        // Its place is worked out only for one near enough to be
        // read.
        let found = named
            .get(sign.0)
            .ok()
            .map(|(room, was)| {
                (room, was, eye.distance(top(was, room)))
            })
            .filter(|(.., far)| *far < pick.signs_far)
            .and_then(|(room, was, far)| {
                let place = lens.stands(sign.0, was);
                camera
                    .world_to_viewport(&view, top(&place, room))
                    .ok()
                    .map(|point| (point, far))
            });
        let Some((point, far)) = found else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        // A far sign is smaller and fainter, and a near one is drawn
        // over it, so that they read as standing apart.
        let near = 1.0
            - ((far - pick.signs_far * CLOSE)
                / (pick.signs_far * (1.0 - CLOSE)))
                .clamp(0.0, 1.0);
        let shown = FAINTEST + (1.0 - FAINTEST) * near;
        transform.scale =
            Vec2::splat(SMALLEST + (1.0 - SMALLEST) * near);
        order.set_if_neq(ZIndex(-1 - (far * 10.0) as i32));
        plate.0 = PLATE.with_alpha(PLATE.alpha() * shown);
        *edge = BorderColor::all(EDGE.with_alpha(shown));
        for child in children {
            if let Ok(mut ink) = texts.get_mut(*child) {
                ink.0 = INK.with_alpha(shown);
            }
        }
        label::put(&mut node, label::above(computed, point, 4.0));
        visibility.set_if_neq(Visibility::Visible);
    }
}

/// A sign's plate, edge and words, at their nearest.
const PLATE: Color = Color::srgba(0.004, 0.039, 0.075, 0.85);
const EDGE: Color = Color::srgb(0.47, 0.353, 0.157);
const INK: Color = Color::srgb(0.941, 0.902, 0.824);

/// The share of the farthest a sign is shown from, the share of its
/// size it shrinks to there, and the share of its strength it fades
/// to.
const CLOSE: f32 = 0.2;
const SMALLEST: f32 = 0.75;
const FAINTEST: f32 = 0.4;

/// The words of the sign of `thing`.
fn words(
    thing: Thing,
    reads: Option<&Reads>,
    names: &Names,
) -> String {
    match reads {
        Some(reads) => reads.0.clone(),
        None => thing.sign(names).to_uppercase(),
    }
}

/// Writes on each sign the name it shows.
fn write_signs(
    names: Names,
    renamed: Query<(), Or<(Changed<Name>, Changed<Shown>)>>,
    mut unnicknamed: RemovedComponents<Shown>,
    named: Query<(Ref<Thing>, &Signed, Option<Ref<Reads>>)>,
    signs: Query<&Children, With<Sign>>,
    mut texts: Query<&mut Text, With<SignText>>,
) {
    let every =
        !renamed.is_empty() || unnicknamed.read().next().is_some();
    for (thing, signed, reads) in &named {
        let rewritten =
            reads.as_ref().is_some_and(DetectChanges::is_changed);
        if !every && !thing.is_changed() && !rewritten {
            continue;
        }
        let Ok(parts) = signs.get(signed.0) else {
            continue;
        };
        let name = words(*thing, reads.as_deref(), &names);
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
    names: Names,
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

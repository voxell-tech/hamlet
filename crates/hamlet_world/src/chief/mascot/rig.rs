//! The entities a wizard is drawn with, and how a stance places them.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{HEAD, Pose};

/// A wizard as it is drawn this frame: a chief's mascot or the
/// librarian.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Stance {
    pub pose: Pose,
    /// Its size, over the size it is built at.
    pub size: f32,
    /// The place its wick floats at, from its feet.
    pub crest: Vec3,
}

impl Default for Stance {
    fn default() -> Self {
        Stance {
            pose: Pose::rest(0.0),
            size: 1.0,
            crest: Vec3::ZERO,
        }
    }
}

/// Puts every wizard's rig in its stance.
pub fn stand(
    rigs: Query<(&Stance, &Rig), Changed<Stance>>,
    mut parts: Query<&mut Transform>,
    mut shown: Query<&mut Visibility>,
) {
    for (stance, rig) in &rigs {
        rig.place(
            &stance.pose,
            stance.size,
            stance.crest,
            &mut parts,
            &mut shown,
        );
    }
}

/// The entities of a wizard's rig.
#[derive(Component, Clone, Copy, Debug)]
#[require(Stance)]
pub struct Rig {
    /// The parent of every part, which takes the figure's squash.
    pub figure: Entity,
    pub cloak: Entity,
    pub head: Entity,
    pub eyes: Entity,
    pub hat: Entity,
    pub hands: [Entity; 2],
    pub feet: [Entity; 2],
    pub item: Entity,
    pub letter: Entity,
    pub wick: Entity,
    pub lamp: Entity,
}

impl Rig {
    /// Spawns the parts under `root`, with no meshes yet.
    pub fn spawn(
        commands: &mut Commands,
        root: Entity,
        matte: &Handle<StandardMaterial>,
        flame: (Handle<Mesh>, Handle<StandardMaterial>),
    ) -> Rig {
        let mut part = |parent: Entity| {
            commands
                .spawn((
                    Mesh3d::default(),
                    MeshMaterial3d(matte.clone()),
                    Transform::default(),
                    Visibility::Inherited,
                    ChildOf(parent),
                ))
                .id()
        };
        let figure = part(root);
        let cloak = part(figure);
        let head = part(figure);
        let (eyes, hat) = (part(head), part(head));
        let hands = [part(figure), part(figure)];
        let feet = [part(figure), part(figure)];
        let item = part(hands[1]);
        let letter = part(figure);
        let wick = commands
            .spawn((
                Mesh3d(flame.0),
                MeshMaterial3d(flame.1),
                Transform::default(),
                Visibility::Inherited,
                NotShadowCaster,
                ChildOf(root),
            ))
            .id();
        let lamp = commands
            .spawn((
                PointLight {
                    intensity: 0.0,
                    radius: 0.05,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::default(),
                ChildOf(root),
            ))
            .id();
        Rig {
            figure,
            cloak,
            head,
            eyes,
            hat,
            hands,
            feet,
            item,
            letter,
            wick,
            lamp,
        }
    }

    /// Each part that has a mesh of `Parts`, in the order of
    /// `Parts::each`. The two hands share one and the two feet
    /// another.
    pub fn dressed(&self) -> [(Entity, usize); 10] {
        [
            (self.cloak, 0),
            (self.head, 1),
            (self.eyes, 2),
            (self.hat, 3),
            (self.hands[0], 4),
            (self.hands[1], 4),
            (self.feet[0], 5),
            (self.feet[1], 5),
            (self.item, 6),
            (self.letter, 7),
        ]
    }

    /// Puts every part where `pose` has it, at `size` times the
    /// size it is built at, with the wick over `crest`.
    pub fn place(
        &self,
        pose: &Pose,
        size: f32,
        crest: Vec3,
        parts: &mut Query<&mut Transform>,
        shown: &mut Query<&mut Visibility>,
    ) {
        let mut set = |entity: Entity, to: Transform| {
            if let Ok(mut at) = parts.get_mut(entity)
                && *at != to
            {
                *at = to;
            }
        };
        // A figure drawn up to nothing is not drawn at all.
        let gone = pose.girth < 0.02;
        if let Ok(mut seen) = shown.get_mut(self.figure) {
            let want = if gone {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            seen.set_if_neq(want);
        }
        let girth = pose.girth.max(0.02);
        set(
            self.figure,
            Transform::from_xyz(0.0, pose.lift * size, 0.0)
                .with_rotation(
                    Quat::from_rotation_x(pose.lean)
                        * Quat::from_rotation_z(pose.roll),
                )
                .with_scale(
                    Vec3::new(girth, pose.height, girth) * size,
                ),
        );
        set(
            self.head,
            Transform::from_xyz(0.0, HEAD, 0.0).with_rotation(
                Quat::from_rotation_y(pose.head.x)
                    * Quat::from_rotation_x(pose.head.y)
                    * Quat::from_rotation_z(pose.head.z),
            ),
        );
        let open = 1.0 - 0.92 * pose.blink.clamp(0.0, 1.0);
        set(
            self.eyes,
            Transform::from_xyz(0.0, 0.025, 0.0)
                .with_scale(Vec3::new(1.0, open, 1.0)),
        );
        set(
            self.hat,
            Transform::from_xyz(0.0, pose.hat.x, 0.0).with_rotation(
                Quat::from_rotation_x(-0.1 - pose.hat.y),
            ),
        );
        for (hand, at) in self.hands.into_iter().zip(pose.hands) {
            set(hand, Transform::from_translation(at));
        }
        for (side, (foot, off)) in
            self.feet.into_iter().zip(pose.feet).enumerate()
        {
            let x = if side == 0 { -0.13 } else { 0.13 };
            set(foot, Transform::from_xyz(x, off.y, off.z));
        }
        set(
            self.item,
            Transform::from_rotation(Quat::from_rotation_x(
                pose.item,
            )),
        );
        set(
            self.letter,
            Transform::from_xyz(0.0, 0.76, 0.42)
                .with_rotation(Quat::from_rotation_x(-0.6))
                .with_scale(Vec3::splat(pose.letter.clamp(0.0, 1.0))),
        );
        set(
            self.wick,
            Transform::from_translation(
                (crest
                    + pose.wick
                    + Vec3::Y * (pose.lift + pose.hat.x))
                    * size,
            )
            .with_scale(Vec3::splat(pose.glow.max(0.0) * size)),
        );
        // The lamp stands for the wick's light, before the chest so
        // that it falls on the face.
        set(self.lamp, Transform::from_xyz(0.0, size, 0.55 * size));
    }
}

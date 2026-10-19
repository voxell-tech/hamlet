//! A chief in the world (docs/chief.md): its place, the Hearth, its
//! mascot, and the Wickstep that carries the mascot to whoever the
//! chief talks to.

mod dress;
pub mod look;
pub mod mascot;
pub mod place;
mod settle;
#[cfg(test)]
mod tests;
pub mod wickstep;

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_client::ViewDir;

pub use self::look::{Cloak, Hat, Holds, Light, Look};
pub use self::wickstep::{Mascot, Phase, Stay};
use crate::generator::land::Land;
use crate::hand::PlaySettings;
use crate::library::Library;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ChiefSettings>()
        .init_resource::<PlaySettings>()
        .add_observer(settle::leave)
        .add_systems(Startup, wickstep::prepare)
        .add_systems(
            Update,
            (
                (settle::resettle, settle::settle)
                    .chain()
                    .in_set(Settling),
                settle::hold_ground.run_if(
                    resource_changed::<Land>
                        .or_eager(resource_changed::<ChiefSettings>)
                        .or_eager(
                            any_match_filter::<
                                Or<(
                                    Changed<Place>,
                                    Changed<Library>,
                                )>,
                            >,
                        )
                        .or_eager(any_component_removed::<Place>)
                        .or_eager(any_component_removed::<Library>),
                ),
                dress::dress,
                wickstep::step,
                (dress::animate, wickstep::cast, dress::light),
                mascot::stand,
            )
                .chain()
                .before(crate::WorldSystems),
        )
        .add_systems(
            Update,
            keep_place.run_if(resource_exists::<ViewDir>),
        );
}

/// A chief's place, its mascot and the Wickstep that carries it.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefSettings {
    /// The mascot's size, over the size it is built at.
    pub size: f32,
    /// Seconds a Wickstep takes to go out, stays dark, and takes to
    /// come in.
    pub out_time: f32,
    pub dark_time: f32,
    pub in_time: f32,
    /// The rune circle of a Wickstep: its reach, how far over the
    /// ground it lies and how bright it is.
    pub rune_radius: f32,
    pub rune_lift: f32,
    pub rune_light: f32,
    /// The motes of a Wickstep: how many at full quality, the
    /// seconds each lasts, how high they rise, how far they spread
    /// and their size.
    pub motes: u32,
    pub mote_time: f32,
    pub mote_rise: f32,
    pub mote_spread: f32,
    pub mote_size: f32,
    /// The wick, and the motes with it: how bright it is drawn, the
    /// light it throws and how far, and how many times brighter
    /// it flares on arriving.
    pub wick_light: f32,
    pub lamp: f32,
    pub lamp_range: f32,
    pub flare: f32,
    /// Brightness of the orb on a chief's roof and of the waystone's
    /// ring, and of the lantern over the gate after dark.
    pub orb_light: f32,
    pub waystone_light: f32,
    pub lantern_light: f32,
    /// Beside an agent: the mascot's offset to its side and toward
    /// the camera, the stray of the agent before it is followed on
    /// foot, its distance before the mascot hurries, and the
    /// farthest the mascot goes on foot. Past that it goes by a
    /// Wickstep.
    pub beside: f32,
    pub before: f32,
    pub slack: f32,
    pub restep: f32,
    pub afoot: f32,
    /// Distance toward the camera from its agent of the point the
    /// mascot faces, so that its face shows.
    pub cheat: f32,
    /// By a hamlet's common: how far from its middle the mascot
    /// stands, and the seconds before it walks on to another spot.
    pub stroll: (f32, f32),
    /// Speeds of the mascot's walk and turn, and the seconds it
    /// takes to ease from one motion into the next. After an agent
    /// that gets ahead it walks faster, up to a robot's fastest.
    pub walk_speed: f32,
    pub turn_rate: f32,
    pub blend: f32,
    /// The least and the most seconds it stays at one spot of its
    /// grounds.
    pub linger: (f32, f32),
    /// The radius of the wall about a chief's grounds.
    pub grounds: f32,
    /// The box a pointer can hit for the mascot, and the radius and
    /// height of the room the house takes.
    pub mascot_box: Vec3,
    pub house: (f32, f32),
}

/// A human the world knows, by the name they sign in with.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Chief {
    pub user: String,
}

/// On the chief this app belongs to: the one whose place the hand
/// sets down.
#[derive(Component, Clone, Copy, Debug)]
pub struct Player;

/// The agent a chief is talking to, whose side its mascot keeps.
/// Nobody while it is at home.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Beside(pub Option<Entity>);

/// The spot a chief has set its place down on. Without one it stands
/// where the land puts it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Stands(pub Vec2);

impl Stands {
    fn file(view: &Path, user: &str) -> PathBuf {
        Look::file(view, user).with_extension("place")
    }

    /// The place the chief `user` last set its place down at, if it
    /// has.
    pub fn read(view: &Path, user: &str) -> Option<Stands> {
        Self::read_at(&Self::file(view, user))
    }

    /// The spot the place whose file is `file` was last set down at.
    pub fn read_at(file: &Path) -> Option<Stands> {
        let text = std::fs::read_to_string(file).ok()?;
        let mut parts = text.split_whitespace().map(|part| {
            part.parse::<f32>().ok().filter(|n| n.is_finite())
        });
        let (x, z) = (parts.next()??, parts.next()??);
        Some(Stands(Vec2::new(x, z)))
    }

    pub fn write(
        self,
        view: &Path,
        user: &str,
    ) -> std::io::Result<()> {
        self.write_at(&Self::file(view, user))
    }

    /// Keeps where a place was set down in `file`.
    pub fn write_at(self, file: &Path) -> std::io::Result<()> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(file, format!("{} {}\n", self.0.x, self.0.y))
    }
}

/// On a chief whose place and mascot are in the world.
#[derive(Component, Clone, Copy, Debug)]
pub struct Settled {
    pub place: Entity,
    pub mascot: Entity,
}

/// A chief's place. Its entity carries the mesh of the house and
/// the grounds.
#[derive(Component, Clone, Debug)]
pub struct Place {
    pub chief: Entity,
    panes: Entity,
    /// The middle of the ground it holds against the towns.
    held: Vec2,
    /// The light of the orb on its roof and of its waystone, and
    /// the glass of its gate's lantern.
    orb: Handle<StandardMaterial>,
    ring: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
}

/// The systems that give a new chief its place. They run before
/// any town is founded, so that a town never takes a chief's site.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Settling;

/// Brings the chief `user` into the world, looking like `look`.
pub fn arrive(world: &mut World, user: &str, look: Look) -> Entity {
    world
        .spawn((
            Chief {
                user: user.to_string(),
            },
            Name::new(user.to_string()),
            look,
            Beside::default(),
        ))
        .id()
}

/// Keeps the place a chief set down.
fn keep_place(
    view: Res<ViewDir>,
    placed: Query<(&Chief, &Stands), Changed<Stands>>,
) {
    for (chief, stands) in &placed {
        // One read back at launch is as it was written.
        if Stands::read(&view.0, &chief.user) != Some(*stands) {
            let _ = stands.write(&view.0, &chief.user);
        }
    }
}

/// The chief called `user`, if the world knows one.
pub fn named(world: &mut World, user: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Chief)>()
        .iter(world)
        .find(|(_, chief)| chief.user == user)
        .map(|(entity, _)| entity)
}

#[cfg(test)]
mod persistence_tests {
    use hamlet_client::ViewDir;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::generator::society::Society;
    use crate::sky::Quality;

    #[test]
    fn a_wizards_place_is_kept_under_the_view_folder() {
        let dir = Scratch::new("wizard-view-place");
        let mut app = App::new();
        crate::generator::testing::rehearse(&mut app);
        app.insert_resource(ViewDir(dir.join("view")))
            .init_resource::<Society>()
            .init_resource::<Quality>()
            .add_message::<crate::play::Played>()
            .add_plugins(plugin);
        let chief = arrive(app.world_mut(), "wren", Look::default());
        let at = Stands(Vec2::new(412.5, -96.0));
        app.world_mut().entity_mut(chief).insert(at);
        app.update();
        assert_eq!(Stands::read(&dir.join("view"), "wren"), Some(at));
        assert_eq!(
            std::fs::read_to_string(
                dir.join("view/chiefs/wren.place")
            )
            .unwrap(),
            "412.5 -96\n"
        );
        let to = Stands(Vec2::new(-240.0, 93.5));
        app.world_mut().entity_mut(chief).insert(to);
        app.update();
        assert_eq!(Stands::read(&dir.join("view"), "wren"), Some(to));
    }
}

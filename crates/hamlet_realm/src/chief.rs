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

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bevy::prelude::*;
use hamlet_client::ViewDir;

pub use self::look::{Cloak, Hat, Holds, Light, Look};
pub use self::wickstep::{Mascot, Phase, Stay};
use crate::generator::land::Land;
use crate::hand::PlaySettings;
use crate::library::{Library, Stood};

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
                .in_set(crate::RealmSystems::Act),
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

    /// The spot `text` holds, as [`Self::write_at`] writes it.
    fn parse(text: &str) -> Option<Stands> {
        let parts = text
            .split_whitespace()
            .map(|part| {
                part.parse::<f32>().ok().filter(|n| n.is_finite())
            })
            .collect::<Option<Vec<_>>>()?;
        let [x, z] = parts[..] else {
            return None;
        };
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

/// The chiefs' looks and places kept under the window's folder, and
/// the library's place, as read once at launch and kept since.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Kept {
    /// By the name each chief's files are kept under.
    looks: HashMap<String, Look>,
    places: HashMap<String, Stands>,
    /// Where the library was last set down.
    pub library: Option<Vec2>,
}

impl Kept {
    /// What `view` keeps. A file that is missing keeps nothing; one
    /// that does not parse is an error naming it.
    pub fn read(view: &Path) -> Result<Self> {
        let mut kept = Self::default();
        let chiefs = view.join("chiefs");
        let entries = match std::fs::read_dir(&chiefs) {
            Ok(entries) => {
                entries.collect::<std::io::Result<Vec<_>>>()
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
        .with_context(|| format!("reading {}", chiefs.display()))?;
        for entry in entries {
            let path = entry.path();
            let Some(name) =
                path.file_stem().and_then(|s| s.to_str())
            else {
                continue;
            };
            match path.extension().and_then(|s| s.to_str()) {
                Some("toml") => {
                    let look = text(&path)?.map(|text| {
                        toml::from_str::<Look>(&text)
                            .map_err(|e| unparsed(&path, e.message()))
                    });
                    if let Some(look) = look.transpose()? {
                        kept.looks.insert(name.into(), look);
                    }
                }
                Some("place") => {
                    if let Some(stands) = place(&path)? {
                        kept.places.insert(name.into(), stands);
                    }
                }
                _ => {}
            }
        }
        kept.library =
            place(&Stood::file(view))?.map(|stands| stands.0);
        Ok(kept)
    }

    /// The look kept for `user`, else the first look.
    pub fn look(&self, user: &str) -> Look {
        self.looks
            .get(&look::kept_name(user))
            .copied()
            .unwrap_or_default()
    }

    /// Where `user` last set its place down, if it has.
    pub fn place(&self, user: &str) -> Option<Stands> {
        self.places.get(&look::kept_name(user)).copied()
    }

    /// Notes that `look` is kept for `user`.
    pub fn keep_look(&mut self, user: &str, look: Look) {
        self.looks.insert(look::kept_name(user), look);
    }

    /// Notes that `stands` is kept for `user`.
    pub fn keep_place(&mut self, user: &str, stands: Stands) {
        self.places.insert(look::kept_name(user), stands);
    }
}

/// The text of `file`; `None` when there is no such file.
fn text(file: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e)
            .with_context(|| format!("reading {}", file.display())),
    }
}

/// The place `file` keeps, if there is such a file.
fn place(file: &Path) -> Result<Option<Stands>> {
    text(file)?
        .map(|text| {
            Stands::parse(&text).ok_or_else(|| {
                unparsed(file, "it does not hold two numbers")
            })
        })
        .transpose()
}

/// The error of a kept file that does not parse.
pub fn unparsed(file: &Path, why: &str) -> anyhow::Error {
    anyhow!(
        "{} does not parse: {why}. Fix it or remove it.",
        file.display()
    )
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
    mut kept: ResMut<Kept>,
    placed: Query<(&Chief, &Stands), Changed<Stands>>,
) {
    for (chief, stands) in &placed {
        // One read back at launch is as it was written.
        if kept.place(&chief.user) != Some(*stands) {
            let _ = stands.write(&view.0, &chief.user);
            kept.keep_place(&chief.user, *stands);
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
            .init_resource::<Kept>()
            .init_resource::<Society>()
            .init_resource::<Quality>()
            .add_message::<crate::play::Played>()
            .add_plugins(plugin);
        let chief = arrive(app.world_mut(), "wren", Look::default());
        let at = Stands(Vec2::new(412.5, -96.0));
        app.world_mut().entity_mut(chief).insert(at);
        app.update();
        let kept = || Kept::read(&dir.join("view")).unwrap();
        assert_eq!(kept().place("wren"), Some(at));
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
        assert_eq!(kept().place("wren"), Some(to));
        // Set down where it was at launch, it is kept there again.
        app.world_mut().entity_mut(chief).insert(at);
        app.update();
        assert_eq!(kept().place("wren"), Some(at));
    }
}

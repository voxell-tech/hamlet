//! The chief's records in the home, in the app's files: its look,
//! where it set its Hearth down, and where the library was set down.
//! Each change is kept and told to the watchers.

use std::path::{Path, PathBuf};

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use hamlet_core::core::HomeDir;
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{AgentsEvent, ChiefRecord, Look};

use crate::Chief;
use crate::watch::Watchers;

pub(crate) fn plugin(app: &mut App) {
    let home = app.world().resource::<HomeDir>().0.root.clone();
    let user = app.world().resource::<Chief>().name.clone();
    app.insert_resource(Kept(ChiefRecord {
        look: read_look(&look_file(&home, &user)),
        hearth: read_spot(
            &look_file(&home, &user).with_extension("place"),
        ),
        library: read_spot(&library_file(&home)),
        user,
    }));
}

/// The chief's records as they were last read or kept.
#[derive(Resource)]
pub(crate) struct Kept(pub(crate) ChiefRecord);

/// The chief's records' methods on [`World`].
pub(crate) trait ChiefWorldExt {
    fn set_look(&mut self, look: Look) -> Result<(), Failure>;

    fn set_hearth(&mut self, at: (f32, f32)) -> Result<(), Failure>;

    fn set_library(&mut self, at: (f32, f32)) -> Result<(), Failure>;

    /// The file the chief's look is kept in.
    fn look_file(&self) -> PathBuf;

    /// Tells the watchers of the records as they stand.
    fn tell_chief(&mut self);
}

impl ChiefWorldExt for World {
    fn set_look(&mut self, look: Look) -> Result<(), Failure> {
        let text = toml::to_string(&look)
            .map_err(|why| Failure::Failed(why.to_string()))?;
        write(&self.look_file(), &text)?;
        self.resource_mut::<Kept>().0.look = look;
        self.tell_chief();
        Ok(())
    }

    fn set_hearth(&mut self, at: (f32, f32)) -> Result<(), Failure> {
        write(&self.look_file().with_extension("place"), &spot(at)?)?;
        self.resource_mut::<Kept>().0.hearth = Some(at);
        self.tell_chief();
        Ok(())
    }

    fn set_library(&mut self, at: (f32, f32)) -> Result<(), Failure> {
        let home = &self.resource::<HomeDir>().0.root;
        write(&library_file(home), &spot(at)?)?;
        self.resource_mut::<Kept>().0.library = Some(at);
        self.tell_chief();
        Ok(())
    }

    fn look_file(&self) -> PathBuf {
        look_file(
            &self.resource::<HomeDir>().0.root,
            &self.resource::<Kept>().0.user,
        )
    }

    fn tell_chief(&mut self) {
        let record = self.resource::<Kept>().0.clone();
        let mut watchers = self.resource_mut::<Watchers>();
        if watchers.watched() {
            watchers.tell(AgentsEvent::Chief(record));
        }
    }
}

/// The file the look of the chief `user` is kept in.
fn look_file(home: &Path, user: &str) -> PathBuf {
    let name = user
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    home.join("chiefs").join(format!("{name}.toml"))
}

fn library_file(home: &Path) -> PathBuf {
    home.join("library.place")
}

/// The look kept in `file`, or the first look where none is kept or
/// it cannot be read.
fn read_look(file: &Path) -> Look {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| toml::from_str::<Look>(&text).ok())
        .unwrap_or_default()
}

/// The spot kept in `file`, as x and z.
fn read_spot(file: &Path) -> Option<(f32, f32)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut parts = text.split_whitespace().map(|part| {
        part.parse::<f32>().ok().filter(|n| n.is_finite())
    });
    Some((parts.next()??, parts.next()??))
}

/// A spot as its file keeps it.
fn spot((x, z): (f32, f32)) -> Result<String, Failure> {
    if !x.is_finite() || !z.is_finite() {
        return Err(Failure::Refused(
            "a spot is two finite numbers".into(),
        ));
    }
    Ok(format!("{x} {z}\n"))
}

fn write(file: &Path, text: &str) -> Result<(), Failure> {
    let failed =
        |why: std::io::Error| Failure::Failed(why.to_string());
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(failed)?;
    }
    std::fs::write(file, text).map_err(failed)
}

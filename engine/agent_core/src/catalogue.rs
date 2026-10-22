//! The home's catalogue of models, kept in step with its file, which
//! the core writes when an endpoint answers and a chief may write by
//! hand.

use std::time::{Duration, Instant, SystemTime};

use bevy_app::{App, Update};
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Local, Res, ResMut};
use hamlet_model::catalogue::{self, Catalogue};

use crate::core::{HomeDir, Listing, Settings};

/// The time between two looks at the catalogue's file.
const LOOK: Duration = Duration::from_secs(2);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<ModelCatalogue>()
        .add_systems(Update, follow);
}

/// The catalogue as its file was last read.
#[derive(Resource, Default)]
pub struct ModelCatalogue {
    pub catalogue: Catalogue,
    stamp: Option<SystemTime>,
}

impl ModelCatalogue {
    /// A catalogue that holds `catalogue`, as if its file had been
    /// read.
    pub fn of(catalogue: Catalogue) -> ModelCatalogue {
        ModelCatalogue {
            catalogue,
            stamp: Some(SystemTime::UNIX_EPOCH),
        }
    }

    /// Reads the file again at the next update, as after a change
    /// written to it.
    pub fn reread(&mut self) {
        self.stamp = None;
    }
}

/// Reads the catalogue again when the endpoints were asked what they
/// serve or the settings changed, after a change written to it, and
/// when its file was written.
fn follow(
    home: Res<HomeDir>,
    listing: Res<Listing>,
    settings: Res<Settings>,
    mut models: ResMut<ModelCatalogue>,
    mut due: Local<Option<Instant>>,
) {
    let now = Instant::now();
    if !listing.is_changed()
        && !settings.is_changed()
        && models.stamp.is_some()
        && due.is_some_and(|due| now < due)
    {
        return;
    }
    *due = Some(now + LOOK);
    let stamp = std::fs::metadata(catalogue::path(&home.0.root))
        .and_then(|file| file.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    if models.stamp != Some(stamp) {
        models.stamp = Some(stamp);
        models.catalogue = Catalogue::load(&home.0.root);
    }
}

//! The pictures of the library's meshes: one square image for each
//! mesh asked for, drawn by the window and drawn again only when the
//! mesh changes.

use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, SystemTime};

use bevy::asset::Handle;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::image::Image;
use hamlet_graph::{Asset, Shape};

use super::Library;

/// The side of a picture, in pixels.
pub const SIDE: u32 = 128;

/// How often the files of the meshes drawn are looked at again, for
/// one replaced outside the window.
const RECHECK: Duration = Duration::from_secs(3);

/// The pictures asked for and what each was drawn from.
#[derive(Resource, Default)]
pub struct Thumbnails {
    entries: BTreeMap<String, Entry>,
    /// The library's revision the keys were last worked out for.
    seen: Option<u64>,
    /// When they were last worked out.
    checked: Duration,
    /// Whether a mesh has been asked for since.
    asked: bool,
}

struct Entry {
    image: Option<Handle<Image>>,
    /// What the image shows, once it has been drawn.
    drawn: Option<Key>,
    /// What a job under way will draw.
    queued: Option<Key>,
}

/// What a picture is drawn from: the mesh's entry in the catalogue
/// and its file, as it was when last written.
#[derive(Clone, PartialEq, Debug)]
pub struct Key {
    asset: Asset,
    file: Option<Stamp>,
}

/// A file's length and the time it was last written.
type Stamp = (u64, SystemTime);

impl Key {
    fn of(library: &Library, name: &str) -> Option<Self> {
        let asset = library.catalogue.get(name)?;
        let file = match &asset.shape {
            Shape::Mesh(path) => library
                .root()
                .and_then(|root| fs::metadata(root.join(path)).ok())
                .and_then(|data| {
                    Some((data.len(), data.modified().ok()?))
                }),
            _ => None,
        };
        Some(Self { asset, file })
    }

    /// The catalogue entry the picture shows.
    pub fn asset(&self) -> &Asset {
        &self.asset
    }
}

/// A picture to draw.
#[derive(Debug)]
pub struct Job {
    pub name: String,
    pub key: Key,
    /// The image it was drawn into before, if it was.
    pub image: Option<Handle<Image>>,
}

impl Thumbnails {
    /// A request for the picture of the mesh `name`, made once.
    pub fn ask(world: &mut World, name: &str) {
        if world.resource::<Self>().entries.contains_key(name) {
            return;
        }
        let mut thumbnails = world.resource_mut::<Self>();
        thumbnails.entries.insert(
            name.to_owned(),
            Entry {
                image: None,
                drawn: None,
                queued: None,
            },
        );
        thumbnails.asked = true;
    }

    /// The image of the mesh `name`, once it has been drawn.
    pub fn picture(&self, name: &str) -> Option<Handle<Image>> {
        let entry = self.entries.get(name)?;
        entry.drawn.as_ref()?;
        entry.image.clone()
    }

    /// Whether [`due`](Self::due) has anything to look at.
    pub fn has_work(&self, library: &Library, now: Duration) -> bool {
        !self.entries.is_empty()
            && (self.asked
                || self.seen != Some(library.revision)
                || now.saturating_sub(self.checked) >= RECHECK)
    }

    /// The pictures that show something else than their mesh now,
    /// each given out once until it is [`drawn`](Self::drawn).
    pub fn due(
        &mut self,
        library: &Library,
        now: Duration,
    ) -> Vec<Job> {
        self.seen = Some(library.revision);
        self.checked = now;
        self.asked = false;
        let mut jobs = Vec::new();
        for (name, entry) in &mut self.entries {
            let Some(key) = Key::of(library, name) else {
                continue;
            };
            let current =
                entry.queued.as_ref().or(entry.drawn.as_ref());
            if current == Some(&key) {
                continue;
            }
            entry.queued = Some(key.clone());
            jobs.push(Job {
                name: name.clone(),
                key,
                image: entry.image.clone(),
            });
        }
        jobs
    }

    /// The record that `image` shows the mesh `name` as `key` has
    /// it.
    pub fn drawn(
        &mut self,
        name: &str,
        key: Key,
        image: Handle<Image>,
    ) {
        let Some(entry) = self.entries.get_mut(name) else {
            return;
        };
        if entry.queued.as_ref() == Some(&key) {
            entry.queued = None;
        }
        entry.drawn = Some(key);
        entry.image = Some(image);
    }
}

#[cfg(test)]
mod tests {
    use hamlet_graph::Paint;

    use super::*;

    fn due(
        thumbnails: &mut Thumbnails,
        library: &Library,
    ) -> Vec<Job> {
        thumbnails.due(library, Duration::ZERO)
    }

    #[test]
    fn a_picture_is_drawn_again_only_when_its_asset_changes() {
        let mut library = Library::default();
        let mut thumbnails = Thumbnails::default();
        thumbnails.entries.insert(
            "box".to_owned(),
            Entry {
                image: None,
                drawn: None,
                queued: None,
            },
        );

        let first = due(&mut thumbnails, &library);
        assert_eq!(first.len(), 1);
        assert!(
            due(&mut thumbnails, &library).is_empty(),
            "a job under way is not given out again"
        );
        let job = first.into_iter().next().unwrap();
        thumbnails.drawn(&job.name, job.key, Handle::default());
        assert!(thumbnails.picture("box").is_some());
        assert!(due(&mut thumbnails, &library).is_empty());

        let mut asset = library.catalogue.get("box").unwrap();
        asset.slots.insert(
            hamlet_graph::BODY.to_owned(),
            Paint::Palette("ochre".to_owned()),
        );
        library.catalogue.0.insert("box".to_owned(), asset);
        let again = due(&mut thumbnails, &library);
        assert_eq!(again.len(), 1);
        assert!(again[0].image.is_some(), "it keeps its image");
    }
}

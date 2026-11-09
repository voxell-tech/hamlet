//! The pictures of the library's meshes and blueprints: one square
//! image for each asked for, drawn by the window and drawn again only
//! when what it shows changes.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use bevy::asset::Handle;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::image::Image;
use hamlet_graph::{Asset, Shape};

use super::{Kind, Library, Source};

/// The side of a picture, in pixels.
pub const SIDE: u32 = 128;

/// How often the files of the meshes drawn are looked at again, for
/// one replaced outside the window.
const RECHECK: Duration = Duration::from_secs(3);

/// The pictures asked for, by kind and name, and what each was drawn
/// from. A mesh and a blueprint may share a name.
#[derive(Resource, Default)]
pub struct Thumbnails {
    entries: BTreeMap<(Kind, String), Entry>,
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

/// What a picture is drawn from, and its file as it was when last
/// written.
#[derive(Clone, PartialEq, Debug)]
pub struct Key {
    subject: Subject,
    file: Option<Stamp>,
}

/// What a picture shows.
#[derive(Clone, PartialEq, Debug)]
pub enum Subject {
    /// A mesh, as the catalogue has it.
    Mesh(Asset),
    /// A blueprint, as the library was at this revision, which a
    /// change to a graph it runs or an asset it puts down moves.
    Blueprint(u64),
}

/// A file's length and the time it was last written.
type Stamp = (u64, SystemTime);

fn stamp(path: &Path) -> Option<Stamp> {
    let data = fs::metadata(path).ok()?;
    Some((data.len(), data.modified().ok()?))
}

impl Key {
    fn of(library: &Library, kind: Kind, name: &str) -> Option<Self> {
        match kind {
            Kind::Mesh => {
                let asset = library.catalogue.get(name)?;
                let file = match &asset.shape {
                    Shape::Mesh(path) => library
                        .root()
                        .and_then(|root| stamp(&root.join(path))),
                    _ => None,
                };
                Some(Self {
                    subject: Subject::Mesh(asset),
                    file,
                })
            }
            Kind::Blueprint => {
                let item = library.find(Kind::Blueprint, name)?;
                let file = match &item.source {
                    Source::File(path) => stamp(path),
                    _ => None,
                };
                Some(Self {
                    subject: Subject::Blueprint(library.revision),
                    file,
                })
            }
            Kind::Shader | Kind::Material | Kind::Texture => None,
        }
    }

    /// What the picture shows.
    pub fn subject(&self) -> &Subject {
        &self.subject
    }
}

/// A picture to draw.
#[derive(Debug)]
pub struct Job {
    pub kind: Kind,
    pub name: String,
    pub key: Key,
    /// The image it was drawn into before, if it was.
    pub image: Option<Handle<Image>>,
}

impl Thumbnails {
    /// A request for the picture of the `kind` asset `name`, made
    /// once.
    pub fn ask(world: &mut World, kind: Kind, name: &str) {
        let key = (kind, name.to_owned());
        if world.resource::<Self>().entries.contains_key(&key) {
            return;
        }
        let mut thumbnails = world.resource_mut::<Self>();
        thumbnails.entries.insert(
            key,
            Entry {
                image: None,
                drawn: None,
                queued: None,
            },
        );
        thumbnails.asked = true;
    }

    /// The image of the `kind` asset `name`, once it has been drawn.
    pub fn picture(
        &self,
        kind: Kind,
        name: &str,
    ) -> Option<Handle<Image>> {
        let entry = self.entries.get(&(kind, name.to_owned()))?;
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

    /// The pictures that show something else than their asset now,
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
        for ((kind, name), entry) in &mut self.entries {
            let Some(key) = Key::of(library, *kind, name) else {
                continue;
            };
            let current =
                entry.queued.as_ref().or(entry.drawn.as_ref());
            if current == Some(&key) {
                continue;
            }
            entry.queued = Some(key.clone());
            jobs.push(Job {
                kind: *kind,
                name: name.clone(),
                key,
                image: entry.image.clone(),
            });
        }
        jobs
    }

    /// The record that `image` shows the `kind` asset `name` as
    /// `key` has it.
    pub fn drawn(
        &mut self,
        kind: Kind,
        name: &str,
        key: Key,
        image: Handle<Image>,
    ) {
        let Some(entry) =
            self.entries.get_mut(&(kind, name.to_owned()))
        else {
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
            (Kind::Mesh, "box".to_owned()),
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
        thumbnails.drawn(
            job.kind,
            &job.name,
            job.key,
            Handle::default(),
        );
        assert!(thumbnails.picture(Kind::Mesh, "box").is_some());
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

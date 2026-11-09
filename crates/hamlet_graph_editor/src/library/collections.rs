//! Collections: paths recorded in the library's `collections.ron`
//! that group its assets, of every kind. A collection holds no files:
//! each asset's file stays in its kind's shelf, and the record says
//! which collection the asset is in. A collection's path is its
//! parents' names and its own, joined by `/`; an asset in a
//! collection is also in the collections above.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Kind;

/// The collections and the collection each asset is in.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Collections {
    /// Every collection's path, a collection's parents among them.
    #[serde(default)]
    pub paths: BTreeSet<String>,
    /// The collection each asset is in, by its kind's shelf and its
    /// name, as `materials/plaster`. An asset not named is in none.
    #[serde(default)]
    pub placed: BTreeMap<String, String>,
}

impl Collections {
    /// How the asset `name` of `kind` is named in
    /// [`Collections::placed`].
    pub fn key(kind: Kind, name: &str) -> String {
        format!("{}/{name}", kind.shelf())
    }

    /// The collection the asset `name` of `kind` is in.
    pub fn of(&self, kind: Kind, name: &str) -> Option<&str> {
        self.placed.get(&Self::key(kind, name)).map(String::as_str)
    }

    /// Whether the asset `name` of `kind` is in the collection
    /// `collection` or one inside it.
    pub fn holds(
        &self,
        collection: &str,
        kind: Kind,
        name: &str,
    ) -> bool {
        self.of(kind, name).is_some_and(|at| within(at, collection))
    }

    /// Puts the asset `name` of `kind` in the collection
    /// `collection`, or in none.
    pub fn put(
        &mut self,
        kind: Kind,
        name: &str,
        collection: Option<&str>,
    ) -> anyhow::Result<()> {
        let key = Self::key(kind, name);
        match collection {
            Some(collection) => {
                if !self.paths.contains(collection) {
                    anyhow::bail!(
                        "there is no collection {collection}"
                    );
                }
                self.placed.insert(key, collection.to_owned());
            }
            None => {
                self.placed.remove(&key);
            }
        }
        Ok(())
    }

    /// Keeps the asset renamed from `from` to `to` in its collection.
    pub fn renamed(&mut self, kind: Kind, from: &str, to: &str) {
        if let Some(collection) =
            self.placed.remove(&Self::key(kind, from))
        {
            self.placed.insert(Self::key(kind, to), collection);
        }
    }

    /// Forgets the collection of the asset `name` of `kind`.
    pub fn removed(&mut self, kind: Kind, name: &str) {
        self.placed.remove(&Self::key(kind, name));
    }

    /// Makes the collection `path`, and the collections above it it
    /// needs, and gives its path as kept.
    pub fn add(&mut self, path: &str) -> anyhow::Result<String> {
        let path =
            clean(path).context("a collection needs a name")?;
        if self.paths.contains(&path) {
            anyhow::bail!("there is already a collection {path}");
        }
        for (at, _) in path.match_indices('/') {
            self.paths.insert(path[..at].to_owned());
        }
        self.paths.insert(path.clone());
        Ok(path)
    }

    /// A path for a new collection in `parent`, or at the top, named
    /// `wanted` or it with a number after it, that no collection has.
    pub fn free(&self, parent: Option<&str>, wanted: &str) -> String {
        let at = |name: &str| match parent {
            Some(parent) => format!("{parent}/{name}"),
            None => name.to_owned(),
        };
        std::iter::once(at(wanted))
            .chain((2..).map(|n| at(&format!("{wanted}_{n}"))))
            .find(|path| !self.paths.contains(path))
            .unwrap_or_default()
    }

    /// Gives the collection `from` the path `to`, with the
    /// collections and the assets inside it, and gives the path as
    /// kept.
    pub fn rename(
        &mut self,
        from: &str,
        to: &str,
    ) -> anyhow::Result<String> {
        if !self.paths.contains(from) {
            anyhow::bail!("there is no collection {from}");
        }
        let to = clean(to).context("a collection needs a name")?;
        if to == from {
            return Ok(to);
        }
        if within(&to, from) {
            anyhow::bail!("{from} cannot go inside itself");
        }
        if self.paths.contains(&to) {
            anyhow::bail!("there is already a collection {to}");
        }
        let moved = |path: &str| {
            within(path, from)
                .then(|| format!("{to}{}", &path[from.len()..]))
        };
        self.paths = std::mem::take(&mut self.paths)
            .into_iter()
            .map(|path| moved(&path).unwrap_or(path))
            .collect();
        for collection in self.placed.values_mut() {
            if let Some(path) = moved(collection) {
                *collection = path;
            }
        }
        for (at, _) in to.match_indices('/') {
            self.paths.insert(to[..at].to_owned());
        }
        Ok(to)
    }

    /// Takes the collection `path` away. The collections and the
    /// assets inside it go to the collection above it, or to the top.
    pub fn remove(&mut self, path: &str) -> anyhow::Result<()> {
        if !self.paths.remove(path) {
            anyhow::bail!("there is no collection {path}");
        }
        let parent = path.rsplit_once('/').map(|(parent, _)| parent);
        let moved = |at: &str| -> Option<String> {
            let rest = at.strip_prefix(path)?.strip_prefix('/')?;
            Some(match parent {
                Some(parent) => format!("{parent}/{rest}"),
                None => rest.to_owned(),
            })
        };
        self.paths = std::mem::take(&mut self.paths)
            .into_iter()
            .map(|at| moved(&at).unwrap_or(at))
            .collect();
        let mut placed = BTreeMap::new();
        for (key, collection) in std::mem::take(&mut self.placed) {
            let to = if collection == path {
                parent.map(str::to_owned)
            } else {
                Some(moved(&collection).unwrap_or(collection))
            };
            if let Some(to) = to {
                placed.insert(key, to);
            }
        }
        self.placed = placed;
        Ok(())
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        hamlet_graph::to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

/// Whether `path` is the collection `collection` or one inside it.
pub fn within(path: &str, collection: &str) -> bool {
    path.strip_prefix(collection)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// `path` with its names trimmed and the empty ones left out, joined
/// by `/`, or nothing when no name is left.
fn clean(path: &str) -> Option<String> {
    let names: Vec<&str> = path
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collection_renamed_or_removed_keeps_what_is_inside_it() {
        let mut collections = Collections::default();
        assert_eq!(
            collections.add(" village / roofs ").unwrap(),
            "village/roofs"
        );
        assert!(collections.paths.contains("village"));
        collections
            .put(Kind::Material, "slate", Some("village/roofs"))
            .unwrap();
        collections
            .put(Kind::Mesh, "barrel", Some("village"))
            .unwrap();
        assert!(collections.holds(
            "village",
            Kind::Material,
            "slate"
        ));
        assert!(!collections.holds(
            "village/roofs",
            Kind::Mesh,
            "barrel"
        ));

        assert_eq!(
            collections.rename("village", "town").unwrap(),
            "town"
        );
        assert_eq!(
            collections.of(Kind::Material, "slate"),
            Some("town/roofs")
        );
        assert_eq!(
            collections.of(Kind::Mesh, "barrel"),
            Some("town")
        );
        assert!(collections.rename("town", "town/inner").is_err());

        // The roofs collection goes, and its material moves up to the
        // town.
        collections.remove("town/roofs").unwrap();
        assert_eq!(
            collections.of(Kind::Material, "slate"),
            Some("town")
        );
        assert_eq!(
            collections.of(Kind::Mesh, "barrel"),
            Some("town")
        );
        assert_eq!(
            collections.paths,
            BTreeSet::from(["town".to_owned()])
        );
        // A collection at the top leaves its assets in none.
        collections.remove("town").unwrap();
        assert_eq!(collections.of(Kind::Mesh, "barrel"), None);
    }

    #[test]
    fn collections_read_back_as_they_were_written() {
        let mut collections = Collections::default();
        collections.add("village").unwrap();
        collections
            .put(Kind::Shader, "plaster", Some("village"))
            .unwrap();
        let text = collections.to_ron().unwrap();
        assert_eq!(
            Collections::from_ron(&text).unwrap(),
            collections
        );
    }
}

//! Folders: paths recorded in the library's `folders.ron` that group
//! its assets, of every kind. A folder holds no files: each asset's
//! file stays in its kind's shelf, and the record says which folder
//! the asset is in. A folder's path is its parents' names and its
//! own, joined by `/`; an asset in a folder is also in the folders
//! above.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Kind;

/// The folders and the folder each asset is in.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Folders {
    /// Every folder's path, a folder's parents among them.
    #[serde(default)]
    pub paths: BTreeSet<String>,
    /// The folder each asset is in, by its kind's shelf and its
    /// name, as `materials/plaster`. An asset not named is in
    /// none.
    #[serde(default)]
    pub placed: BTreeMap<String, String>,
}

impl Folders {
    /// How the asset `name` of `kind` is named in
    /// [`Folders::placed`].
    pub fn key(kind: Kind, name: &str) -> String {
        format!("{}/{name}", kind.shelf())
    }

    /// The folder the asset `name` of `kind` is in.
    pub fn of(&self, kind: Kind, name: &str) -> Option<&str> {
        self.placed.get(&Self::key(kind, name)).map(String::as_str)
    }

    /// Whether the asset `name` of `kind` is in the folder `folder`
    /// or one inside it.
    pub fn holds(
        &self,
        folder: &str,
        kind: Kind,
        name: &str,
    ) -> bool {
        self.of(kind, name).is_some_and(|at| within(at, folder))
    }

    /// Puts the asset `name` of `kind` in the folder `folder`, or in
    /// none.
    pub fn put(
        &mut self,
        kind: Kind,
        name: &str,
        folder: Option<&str>,
    ) -> anyhow::Result<()> {
        let key = Self::key(kind, name);
        match folder {
            Some(folder) => {
                if !self.paths.contains(folder) {
                    anyhow::bail!("there is no folder {folder}");
                }
                self.placed.insert(key, folder.to_owned());
            }
            None => {
                self.placed.remove(&key);
            }
        }
        Ok(())
    }

    /// Keeps the asset renamed from `from` to `to` in its folder.
    pub fn renamed(&mut self, kind: Kind, from: &str, to: &str) {
        if let Some(folder) =
            self.placed.remove(&Self::key(kind, from))
        {
            self.placed.insert(Self::key(kind, to), folder);
        }
    }

    /// Forgets the folder of the asset `name` of `kind`.
    pub fn removed(&mut self, kind: Kind, name: &str) {
        self.placed.remove(&Self::key(kind, name));
    }

    /// Makes the folder `path`, and the folders above it it needs,
    /// and gives its path as kept.
    pub fn add(&mut self, path: &str) -> anyhow::Result<String> {
        let path = clean(path).context("a folder needs a name")?;
        if self.paths.contains(&path) {
            anyhow::bail!("there is already a folder {path}");
        }
        for (at, _) in path.match_indices('/') {
            self.paths.insert(path[..at].to_owned());
        }
        self.paths.insert(path.clone());
        Ok(path)
    }

    /// A path for a new folder in `parent`, or at the top, named
    /// `wanted` or it with a number after it, that no folder has.
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

    /// Gives the folder `from` the path `to`, with the folders and
    /// the assets inside it, and gives the path as kept.
    pub fn rename(
        &mut self,
        from: &str,
        to: &str,
    ) -> anyhow::Result<String> {
        if !self.paths.contains(from) {
            anyhow::bail!("there is no folder {from}");
        }
        let to = clean(to).context("a folder needs a name")?;
        if to == from {
            return Ok(to);
        }
        if within(&to, from) {
            anyhow::bail!("{from} cannot go inside itself");
        }
        if self.paths.contains(&to) {
            anyhow::bail!("there is already a folder {to}");
        }
        let moved = |path: &str| {
            within(path, from)
                .then(|| format!("{to}{}", &path[from.len()..]))
        };
        self.paths = std::mem::take(&mut self.paths)
            .into_iter()
            .map(|path| moved(&path).unwrap_or(path))
            .collect();
        for folder in self.placed.values_mut() {
            if let Some(path) = moved(folder) {
                *folder = path;
            }
        }
        for (at, _) in to.match_indices('/') {
            self.paths.insert(to[..at].to_owned());
        }
        Ok(to)
    }

    /// Takes the folder `path` away. The folders and the assets
    /// inside it go to the folder above it, or to the top.
    pub fn remove(&mut self, path: &str) -> anyhow::Result<()> {
        if !self.paths.remove(path) {
            anyhow::bail!("there is no folder {path}");
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
        for (key, folder) in std::mem::take(&mut self.placed) {
            let to = if folder == path {
                parent.map(str::to_owned)
            } else {
                Some(moved(&folder).unwrap_or(folder))
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

/// Whether `path` is the folder `folder` or one inside it.
pub fn within(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
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
    fn a_folder_renamed_or_removed_keeps_what_is_inside_it() {
        let mut folders = Folders::default();
        assert_eq!(
            folders.add(" village / roofs ").unwrap(),
            "village/roofs"
        );
        assert!(folders.paths.contains("village"));
        folders
            .put(Kind::Material, "slate", Some("village/roofs"))
            .unwrap();
        folders.put(Kind::Mesh, "barrel", Some("village")).unwrap();
        assert!(folders.holds("village", Kind::Material, "slate"));
        assert!(!folders.holds(
            "village/roofs",
            Kind::Mesh,
            "barrel"
        ));

        assert_eq!(
            folders.rename("village", "town").unwrap(),
            "town"
        );
        assert_eq!(
            folders.of(Kind::Material, "slate"),
            Some("town/roofs")
        );
        assert_eq!(folders.of(Kind::Mesh, "barrel"), Some("town"));
        assert!(folders.rename("town", "town/inner").is_err());

        // The roofs folder goes, and its material moves up to the
        // town.
        folders.remove("town/roofs").unwrap();
        assert_eq!(folders.of(Kind::Material, "slate"), Some("town"));
        assert_eq!(folders.of(Kind::Mesh, "barrel"), Some("town"));
        assert_eq!(
            folders.paths,
            BTreeSet::from(["town".to_owned()])
        );
        // A folder at the top leaves its assets in none.
        folders.remove("town").unwrap();
        assert_eq!(folders.of(Kind::Mesh, "barrel"), None);
    }

    #[test]
    fn folders_read_back_as_they_were_written() {
        let mut folders = Folders::default();
        folders.add("village").unwrap();
        folders
            .put(Kind::Shader, "plaster", Some("village"))
            .unwrap();
        let text = folders.to_ron().unwrap();
        assert_eq!(Folders::from_ron(&text).unwrap(), folders);
    }
}

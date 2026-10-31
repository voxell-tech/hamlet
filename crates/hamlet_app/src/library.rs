//! The library: every asset the editors share, in one folder in the
//! window's folder. Meshes are `.glb` files in `meshes/`, each named
//! in `catalogue.ron` with its slots' colours and its sockets;
//! blueprint and material books are `.ron` files in `blueprints/` and
//! `materials/`; textures are `.png` files in `textures/`. A book
//! names the meshes it uses rather than carrying them. What is
//! removed is moved to `.trash/`, not deleted.

pub(crate) mod browser;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::time::Time;
use hamlet_blueprint::{Asset, Book, Catalogue, Shape};
use hamlet_client::ViewFolder;
use hamlet_ui::widgets::icons::Glyph;

/// How often the folder is read again, for files changed outside the
/// window.
const RESCAN: Duration = Duration::from_secs(3);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Library>()
        .add_systems(Update, (find, rescan))
        .add_plugins(browser::plugin);
}

/// What an asset is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Kind {
    Blueprint,
    Material,
    Mesh,
    Texture,
}

impl Kind {
    /// The folder in the library its files are kept in.
    pub fn shelf(self) -> &'static str {
        match self {
            Kind::Blueprint => "blueprints",
            Kind::Material => "materials",
            Kind::Mesh => "meshes",
            Kind::Texture => "textures",
        }
    }

    /// Its files' extension.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Blueprint | Kind::Material => "ron",
            Kind::Mesh => "glb",
            Kind::Texture => "png",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Kind::Blueprint => "Blueprint",
            Kind::Material => "Material",
            Kind::Mesh => "Mesh",
            Kind::Texture => "Texture",
        }
    }

    pub fn glyph(self) -> Glyph {
        match self {
            Kind::Blueprint => Glyph::Signpost,
            Kind::Material => Glyph::Drop,
            Kind::Mesh => Glyph::Hamlet,
            Kind::Texture => Glyph::Mountains,
        }
    }
}

/// Where an asset comes from.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Source {
    /// A file in the library.
    File(PathBuf),
    /// One of the meshes every book has.
    BuiltIn,
    /// An example book, its text in the window.
    Example(&'static str),
}

/// One asset of the library.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Item {
    pub kind: Kind,
    pub name: String,
    pub source: Source,
}

impl Item {
    /// Whether it can be renamed and removed.
    pub fn is_own(&self) -> bool {
        matches!(self.source, Source::File(_))
    }
}

/// The library's folder and what is in it.
#[derive(Resource, Default)]
pub(crate) struct Library {
    root: Option<PathBuf>,
    /// The meshes, by name.
    pub catalogue: Catalogue,
    /// Every asset, by kind and then name, the examples and built-in
    /// meshes after the library's own.
    pub items: Vec<Item>,
    /// Changes whenever what is in the library changes.
    pub revision: u64,
    /// The last thing done, or what went wrong.
    pub note: String,
    /// When the folder was last read.
    scanned: Duration,
}

impl Library {
    /// The library's folder, once the window's folder is known.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn catalogue_at(root: &Path) -> PathBuf {
        root.join("catalogue.ron")
    }

    /// Reads the catalogue and lists every file again.
    fn scan(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let catalogue = fs::read_to_string(Self::catalogue_at(&root))
            .ok()
            .and_then(|text| Catalogue::from_ron(&text).ok())
            .unwrap_or_default();
        let mut items = Vec::new();
        for kind in [Kind::Blueprint, Kind::Material, Kind::Texture] {
            let Ok(entries) = fs::read_dir(root.join(kind.shelf()))
            else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str())
                    != Some(kind.extension())
                {
                    continue;
                }
                let Some(name) = path.file_stem() else {
                    continue;
                };
                items.push(Item {
                    kind,
                    name: name.to_string_lossy().into_owned(),
                    source: Source::File(path),
                });
            }
        }
        for (name, asset) in &catalogue.0 {
            if let Shape::Mesh(file) = &asset.shape {
                items.push(Item {
                    kind: Kind::Mesh,
                    name: name.clone(),
                    source: Source::File(root.join(file)),
                });
            }
        }
        items.sort_by(|a, b| {
            (a.kind, &a.name).cmp(&(b.kind, &b.name))
        });
        for (kind, examples) in [
            (Kind::Blueprint, crate::graphs::blueprint_examples()),
            (Kind::Material, crate::graphs::material_examples()),
        ] {
            items.extend(examples.iter().map(|(name, text)| Item {
                kind,
                name: (*name).to_owned(),
                source: Source::Example(text),
            }));
        }
        for name in Catalogue::default().names() {
            if !catalogue.0.contains_key(&name) {
                items.push(Item {
                    kind: Kind::Mesh,
                    name,
                    source: Source::BuiltIn,
                });
            }
        }
        if items != self.items || catalogue != self.catalogue {
            self.items = items;
            self.catalogue = catalogue;
            self.revision += 1;
        }
    }

    fn write_catalogue(&self) -> anyhow::Result<()> {
        let root = self.root.as_deref().context("no library yet")?;
        fs::create_dir_all(root)
            .with_context(|| format!("making {}", root.display()))?;
        let text = self
            .catalogue
            .to_ron()
            .context("writing the catalogue")?;
        let path = Self::catalogue_at(root);
        fs::write(&path, text)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// A name for an asset of `kind`, `wanted` or it with a number
    /// after it, that none has.
    pub fn free_name(&self, kind: Kind, wanted: &str) -> String {
        let wanted = wanted.trim().replace(['/', '\\'], "_");
        let wanted = if wanted.is_empty() {
            kind.shelf().to_owned()
        } else {
            wanted
        };
        std::iter::once(wanted.clone())
            .chain((2..).map(|n| format!("{wanted}_{n}")))
            .find(|name| {
                !self.items.iter().any(|item| {
                    item.kind == kind
                        && item.name == *name
                        && item.is_own()
                }) && !(kind == Kind::Mesh
                    && self.catalogue.0.contains_key(name))
            })
            .unwrap_or_default()
    }

    /// Where a file named `name` of `kind` is kept.
    pub fn path_of(&self, kind: Kind, name: &str) -> Option<PathBuf> {
        Some(
            self.root
                .as_deref()?
                .join(kind.shelf())
                .join(format!("{name}.{}", kind.extension())),
        )
    }

    /// Puts `asset` in the catalogue as `name`, or takes the one
    /// named `name` out.
    pub fn set_asset(&mut self, name: String, asset: Option<Asset>) {
        match asset {
            Some(asset) => self.catalogue.0.insert(name, asset),
            None => self.catalogue.0.remove(&name),
        };
        if let Err(error) = self.write_catalogue() {
            self.note = format!("{error:#}");
        }
        self.scan();
        self.revision += 1;
    }

    /// Writes `book` as the `kind` book `name`, and gives its path.
    pub fn save_book(
        &mut self,
        kind: Kind,
        name: &str,
        book: &Book,
    ) -> anyhow::Result<PathBuf> {
        let path =
            self.path_of(kind, name).context("no library yet")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        let text = book.to_ron().context("writing the book")?;
        fs::write(&path, text)
            .with_context(|| format!("writing {}", path.display()))?;
        self.scan();
        Ok(path)
    }

    /// Copies the texture at `from` into the library.
    pub fn add_texture(
        &mut self,
        from: &Path,
    ) -> anyhow::Result<String> {
        let stem = from
            .file_stem()
            .map_or("texture".into(), |stem| stem.to_string_lossy());
        let name = self.free_name(Kind::Texture, &stem);
        let to = self
            .path_of(Kind::Texture, &name)
            .context("no library yet")?;
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        fs::copy(from, &to)
            .with_context(|| format!("copying {}", from.display()))?;
        self.scan();
        Ok(name)
    }

    /// Moves the meshes of `carried`, whose files are in `from`, into
    /// the library, each that it has no mesh of the same name.
    pub fn adopt(
        &mut self,
        carried: &Catalogue,
        from: &Path,
    ) -> anyhow::Result<()> {
        for (name, asset) in &carried.0 {
            // A book opened again brings what it moved in before.
            if self.catalogue.0.contains_key(name) {
                continue;
            }
            let mut asset = asset.clone();
            if let Shape::Mesh(file) = &asset.shape {
                let to = self
                    .path_of(Kind::Mesh, name)
                    .context("no library yet")?;
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(from.join(file), &to)
                    .with_context(|| format!("copying {file}"))?;
                asset.shape = Shape::Mesh(mesh_file(name));
            }
            self.catalogue.0.insert(name.clone(), asset);
        }
        self.write_catalogue()?;
        self.scan();
        Ok(())
    }

    /// Gives `item` the name `name`.
    pub fn rename(
        &mut self,
        item: &Item,
        name: &str,
    ) -> anyhow::Result<String> {
        let Source::File(from) = &item.source else {
            anyhow::bail!("{} cannot be renamed", item.name);
        };
        let name = self.free_name(item.kind, name);
        let to = self
            .path_of(item.kind, &name)
            .context("no library yet")?;
        fs::rename(from, &to).with_context(|| {
            format!("renaming {}", from.display())
        })?;
        if item.kind == Kind::Mesh
            && let Some(mut asset) =
                self.catalogue.0.remove(&item.name)
        {
            asset.shape = Shape::Mesh(mesh_file(&name));
            self.catalogue.0.insert(name.clone(), asset);
            self.write_catalogue()?;
        }
        self.scan();
        Ok(name)
    }

    /// Moves `item`'s file to the library's `.trash/` folder.
    pub fn remove(&mut self, item: &Item) -> anyhow::Result<()> {
        let Source::File(from) = &item.source else {
            anyhow::bail!("{} cannot be removed", item.name);
        };
        let root = self.root.as_deref().context("no library yet")?;
        let trash = root.join(".trash").join(item.kind.shelf());
        fs::create_dir_all(&trash)?;
        let file = from.file_name().context("a file with no name")?;
        let mut to = trash.join(file);
        let mut n = 2;
        while to.exists() {
            to = trash.join(format!(
                "{}_{n}.{}",
                item.name,
                item.kind.extension()
            ));
            n += 1;
        }
        fs::rename(from, &to)
            .with_context(|| format!("moving {}", from.display()))?;
        if item.kind == Kind::Mesh {
            self.catalogue.0.remove(&item.name);
            self.write_catalogue()?;
        }
        self.scan();
        Ok(())
    }
}

/// Where the mesh `name`'s file is, in the library.
pub(crate) fn mesh_file(name: &str) -> String {
    format!(
        "{}/{name}.{}",
        Kind::Mesh.shelf(),
        Kind::Mesh.extension()
    )
}

/// Finds the library's folder once the window's folder is known.
fn find(
    folder: Option<Res<ViewFolder>>,
    mut library: ResMut<Library>,
) {
    let Some(folder) = folder else {
        return;
    };
    if library.root.is_some() {
        return;
    }
    library.root = Some(folder.0.join("library"));
    library.scan();
}

/// Reads the folder again every few seconds.
fn rescan(time: Res<Time>, mut library: ResMut<Library>) {
    let now = time.elapsed();
    if library.root.is_none() || now - library.scanned < RESCAN {
        return;
    }
    library.scanned = now;
    library.scan();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_mesh_moved_in_renamed_and_removed_keeps_its_file_and_entry_together()
     {
        let folder = Scratch::new("library");
        let book = folder.join("book");
        fs::create_dir_all(book.join("assets")).unwrap();
        fs::write(book.join("assets/barrel.glb"), b"glb").unwrap();
        let carried = Catalogue(BTreeMap::from([(
            "barrel".to_owned(),
            Asset {
                shape: Shape::Mesh("assets/barrel.glb".to_owned()),
                slots: BTreeMap::new(),
                sockets: BTreeMap::new(),
            },
        )]));
        let mut library = Library {
            root: Some(folder.join("library")),
            ..Library::default()
        };
        library.adopt(&carried, &book).unwrap();
        let mesh = |library: &Library, name: &str| {
            library
                .items
                .iter()
                .find(|item| {
                    item.kind == Kind::Mesh && item.name == name
                })
                .cloned()
        };
        let barrel = mesh(&library, "barrel").unwrap();
        assert_eq!(
            barrel.source,
            Source::File(folder.join("library/meshes/barrel.glb"))
        );

        let name = library.rename(&barrel, "cask").unwrap();
        let cask = mesh(&library, &name).unwrap();
        assert!(folder.join("library/meshes/cask.glb").exists());
        assert!(!library.catalogue.0.contains_key("barrel"));

        library.remove(&cask).unwrap();
        assert!(mesh(&library, "cask").is_none());
        assert!(
            folder.join("library/.trash/meshes/cask.glb").exists()
        );
        // The built-in mesh of the same kind is still listed.
        assert!(
            library
                .items
                .iter()
                .any(|item| item.source == Source::BuiltIn)
        );
    }
}

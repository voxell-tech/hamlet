//! The library: every asset the editors share, in the folder the
//! window gives it as [`LibraryFolder`]. Meshes are `.glb` files in
//! `meshes/`, each named
//! in `catalogue.ron` with its slots' colours and its sockets, and in
//! `sources.ron` with the file outside the library it was added from
//! and follows; blueprint and shader graphs are `.ron` files in
//! `blueprints/` and `shaders/`, one graph to a file; materials are
//! `.ron` files in `materials/`, each a shader by name with values
//! for some of its inputs; textures are `.png` files in `textures/`.
//! A graph names the meshes it uses and the other graphs of its kind
//! it runs. `collections.ron` records the collections and the
//! collection each asset is in, of every kind, the files staying in
//! their shelves. A removed asset is moved to `.trash/`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::time::Time;
use hamlet_graph::material::Material;
use hamlet_graph::prefab::Prefab;
use hamlet_graph::{Asset, Book, Catalogue, Does, Shape, Sheet};
use hamlet_ui::widgets::icons::Glyph;

use self::collections::Collections;

pub mod carry;
pub mod collections;
pub mod picker;
pub mod search;
pub mod thumbnails;

/// How often the folder is read again, for files changed outside the
/// window.
const RESCAN: Duration = Duration::from_secs(3);

/// Adds the [`Library`] and the systems that follow and read its
/// folder, the pickers of its meshes, and the card a dragged tile
/// shows.
pub fn plugin(app: &mut App) {
    app.init_resource::<Library>()
        .init_resource::<thumbnails::Thumbnails>()
        .add_plugins((picker::plugin, search::plugin, carry::plugin))
        .add_systems(Update, (follow, rescan));
}

/// The folder the library is kept in. None is kept until the window
/// gives one.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub struct LibraryFolder(pub PathBuf);

/// The kind of an asset.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    /// A blueprint graph.
    Blueprint,
    /// A shader graph.
    Shader,
    /// A shader by name with values for some of its inputs.
    Material,
    /// A mesh by name with some of its objects changed.
    Prefab,
    /// A `.glb` mesh.
    Mesh,
    /// A `.png` texture.
    Texture,
}

impl Kind {
    /// The folder in the library its files are kept in.
    pub fn shelf(self) -> &'static str {
        match self {
            Kind::Blueprint => "blueprints",
            Kind::Shader => "shaders",
            Kind::Material => "materials",
            Kind::Prefab => "prefabs",
            Kind::Mesh => "meshes",
            Kind::Texture => "textures",
        }
    }

    /// Its files' extension.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Blueprint
            | Kind::Shader
            | Kind::Material
            | Kind::Prefab => "ron",
            Kind::Mesh => "glb",
            Kind::Texture => "png",
        }
    }

    /// Its name as a word.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Blueprint => "Blueprint",
            Kind::Shader => "Shader",
            Kind::Material => "Material",
            Kind::Prefab => "Prefab",
            Kind::Mesh => "Mesh",
            Kind::Texture => "Texture",
        }
    }

    /// Its icon.
    pub fn glyph(self) -> Glyph {
        match self {
            Kind::Blueprint => Glyph::Signpost,
            Kind::Shader => Glyph::Sphere,
            Kind::Material => Glyph::Drop,
            Kind::Prefab => Glyph::Hamlet,
            Kind::Mesh => Glyph::Cube,
            Kind::Texture => Glyph::Mountains,
        }
    }
}

/// The origin of an asset.
#[derive(Clone, PartialEq, Debug)]
pub enum Source {
    /// A file in the library.
    File(PathBuf),
    /// One of the meshes every graph has.
    BuiltIn,
    /// An example graph, its text in the window.
    Example(&'static str),
}

/// One asset of the library.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    /// Its kind.
    pub kind: Kind,
    /// Its name among the assets of its kind.
    pub name: String,
    /// Where it comes from.
    pub source: Source,
}

/// Marks a tile showing an asset, with the asset.
#[derive(Component, Clone)]
pub struct Tile(pub Item);

impl Item {
    /// Whether it can be renamed and removed.
    pub fn is_own(&self) -> bool {
        matches!(self.source, Source::File(_))
    }

    /// The book its file or text holds.
    pub fn read(&self) -> anyhow::Result<Book> {
        match &self.source {
            Source::File(path) => read_book(path),
            Source::Example(text) => {
                parse_book(text).with_context(|| {
                    format!("{} is not a graph", self.name)
                })
            }
            Source::BuiltIn => {
                anyhow::bail!("{} is not a graph", self.name)
            }
        }
    }

    /// The material its file holds.
    pub fn material(&self) -> anyhow::Result<Material> {
        let Source::File(path) = &self.source else {
            anyhow::bail!("{} is not a material", self.name);
        };
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        Material::from_ron(&text).with_context(|| {
            format!("{} is not a material file", path.display())
        })
    }

    /// The prefab its file holds.
    pub fn prefab(&self) -> anyhow::Result<Prefab> {
        let Source::File(path) = &self.source else {
            anyhow::bail!("{} is not a prefab", self.name);
        };
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        Prefab::from_ron(&text).with_context(|| {
            format!("{} is not a prefab file", path.display())
        })
    }
}

/// The library's folder and its contents.
#[derive(Resource, Default)]
pub struct Library {
    root: Option<PathBuf>,
    /// The example graphs of each kind.
    examples: Vec<(Kind, &'static [(&'static str, &'static str)])>,
    /// The meshes, by name.
    pub catalogue: Catalogue,
    /// The file outside the library each mesh was added from, by the
    /// mesh's name. Kept apart from the catalogue, which books and
    /// bundles carry to other machines.
    pub sources: BTreeMap<String, PathBuf>,
    /// The collections, and the collection each asset is in.
    pub collections: Collections,
    /// Every asset, by kind and then name, the examples and built-in
    /// meshes after the library's own.
    pub items: Vec<Item>,
    /// Changes whenever what is in the library changes.
    pub revision: u64,
    /// The last thing done, or its error.
    pub note: String,
    /// When the folder was last read.
    scanned: Duration,
}

impl Library {
    /// The library in the folder `root`, read, as the window finds it
    /// once its own folder is known.
    pub fn in_folder(root: PathBuf) -> Self {
        let mut library = Self {
            root: Some(root),
            ..Self::default()
        };
        library.scan();
        library
    }

    /// The library's folder, once the window's folder is known.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Lists `examples`, by name and text, as example graphs of
    /// `kind`.
    pub fn add_examples(
        &mut self,
        kind: Kind,
        examples: &'static [(&'static str, &'static str)],
    ) {
        self.examples.push((kind, examples));
        self.scan();
    }

    fn catalogue_at(root: &Path) -> PathBuf {
        root.join("catalogue.ron")
    }

    fn sources_at(root: &Path) -> PathBuf {
        root.join("sources.ron")
    }

    fn collections_at(root: &Path) -> PathBuf {
        root.join("collections.ron")
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
        let mut sources = fs::read_to_string(Self::sources_at(&root))
            .ok()
            .and_then(|text| {
                ron::from_str::<BTreeMap<String, PathBuf>>(&text).ok()
            })
            .unwrap_or_default();
        sources.retain(|name, _| catalogue.0.contains_key(name));
        let collections =
            fs::read_to_string(Self::collections_at(&root))
                .ok()
                .and_then(|text| Collections::from_ron(&text).ok())
                .unwrap_or_default();
        let mut items = Vec::new();
        for kind in [
            Kind::Blueprint,
            Kind::Shader,
            Kind::Material,
            Kind::Prefab,
            Kind::Texture,
        ] {
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
        for (kind, examples) in &self.examples {
            items.extend(examples.iter().map(|(name, text)| Item {
                kind: *kind,
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
        if items != self.items
            || catalogue != self.catalogue
            || sources != self.sources
            || collections != self.collections
        {
            self.items = items;
            self.catalogue = catalogue;
            self.sources = sources;
            self.collections = collections;
            self.revision += 1;
        }
    }

    fn write_collections(&self) -> anyhow::Result<()> {
        let root = self.root.as_deref().context("no library yet")?;
        fs::create_dir_all(root)
            .with_context(|| format!("making {}", root.display()))?;
        let text = self
            .collections
            .to_ron()
            .context("writing the collections")?;
        write_atomic(&Self::collections_at(root), &text)
    }

    /// Changes the collections as `change` says and writes them, and
    /// gives what `change` gives. Nothing is written when it fails.
    pub fn change_collections<T>(
        &mut self,
        change: impl FnOnce(&mut Collections) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut collections = self.collections.clone();
        let given = change(&mut collections)?;
        let before =
            std::mem::replace(&mut self.collections, collections);
        if let Err(error) = self.write_collections() {
            self.collections = before;
            return Err(error);
        }
        self.revision += 1;
        Ok(given)
    }

    fn write_catalogue(&self) -> anyhow::Result<()> {
        let root = self.root.as_deref().context("no library yet")?;
        fs::create_dir_all(root)
            .with_context(|| format!("making {}", root.display()))?;
        let text = self
            .catalogue
            .to_ron()
            .context("writing the catalogue")?;
        write_atomic(&Self::catalogue_at(root), &text)
    }

    fn write_sources(&self) -> anyhow::Result<()> {
        let root = self.root.as_deref().context("no library yet")?;
        fs::create_dir_all(root)
            .with_context(|| format!("making {}", root.display()))?;
        let text = ron::ser::to_string_pretty(
            &self.sources,
            ron::ser::PrettyConfig::default(),
        )
        .context("writing the sources")?;
        write_atomic(&Self::sources_at(root), &text)
    }

    /// Records `from` as the file the mesh `name` was added from and
    /// follows, or that it follows none.
    pub fn set_source(&mut self, name: &str, from: Option<PathBuf>) {
        match from {
            Some(from) => self.sources.insert(name.to_owned(), from),
            None => self.sources.remove(name),
        };
        if let Err(error) = self.write_sources() {
            self.note = format!("{error:#}");
        }
        self.scan();
        self.revision += 1;
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

    /// The asset of `kind` named `name` that is the library's own,
    /// else an example.
    pub fn find(&self, kind: Kind, name: &str) -> Option<Item> {
        self.items
            .iter()
            .find(|item| item.kind == kind && item.name == name)
            .cloned()
    }

    /// Writes `sheet` as the `kind` graph `name`, and gives its path.
    pub fn save_sheet(
        &mut self,
        kind: Kind,
        name: &str,
        sheet: &Sheet,
    ) -> anyhow::Result<PathBuf> {
        let path =
            self.path_of(kind, name).context("no library yet")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        let text = Book::of(name, sheet.clone())
            .to_ron()
            .context("writing the graph")?;
        write_atomic(&path, &text)?;
        self.scan();
        Ok(path)
    }

    /// Writes a new empty graph of `kind`, named `wanted` or the
    /// nearest free name.
    pub fn create(
        &mut self,
        kind: Kind,
        wanted: &str,
    ) -> anyhow::Result<Item> {
        let name = self.free_name(kind, wanted);
        match kind {
            Kind::Material => {
                self.save_material(&name, &Material::default())?;
            }
            Kind::Prefab => {
                self.save_prefab(&name, &Prefab::default())?;
            }
            _ => {
                self.save_sheet(kind, &name, &Sheet::default())?;
            }
        }
        self.find(kind, &name)
            .context("the new asset is not listed")
    }

    /// Writes `material` as the material `name`, and gives its path.
    /// The revision changes only when a new material is listed, so a
    /// value dragged in a field does not build the field again.
    pub fn save_material(
        &mut self,
        name: &str,
        material: &Material,
    ) -> anyhow::Result<PathBuf> {
        let path = self
            .path_of(Kind::Material, name)
            .context("no library yet")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        let text =
            material.to_ron().context("writing the material")?;
        write_atomic(&path, &text)?;
        self.scan();
        Ok(path)
    }

    /// Writes `prefab` as the prefab `name`, and gives its path. As
    /// with a material, the revision changes only when a new prefab
    /// is listed.
    pub fn save_prefab(
        &mut self,
        name: &str,
        prefab: &Prefab,
    ) -> anyhow::Result<PathBuf> {
        let path = self
            .path_of(Kind::Prefab, name)
            .context("no library yet")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        let text = prefab.to_ron().context("writing the prefab")?;
        write_atomic(&path, &text)?;
        self.scan();
        Ok(path)
    }

    /// The prefabs that name the mesh `mesh`, by name.
    pub fn prefabs_of(&self, mesh: &str) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.kind == Kind::Prefab)
            .filter(|item| {
                item.prefab().is_ok_and(|prefab| prefab.model == mesh)
            })
            .map(|item| item.name.clone())
            .collect()
    }

    /// Refuses to rename or remove the mesh `item` while a prefab
    /// names it.
    fn refuse_named_mesh(&self, item: &Item) -> anyhow::Result<()> {
        if item.kind != Kind::Mesh {
            return Ok(());
        }
        let prefabs = self.prefabs_of(&item.name);
        if !prefabs.is_empty() {
            anyhow::bail!(
                "{} is the mesh of the prefab {}",
                item.name,
                prefabs.join(", ")
            );
        }
        Ok(())
    }

    /// The sheet `item` holds, and the names of the assets split off
    /// from it. A file of several sheets is split: the entry stays in
    /// the file, and each other sheet becomes an asset of its own.
    pub fn read(
        &mut self,
        item: &Item,
    ) -> anyhow::Result<(Sheet, Vec<String>)> {
        let book = item.read()?;
        if book.sheets.len() < 2 || !item.is_own() {
            return Ok((lone(book), Vec::new()));
        }
        let (main, placed) =
            self.spread(item.kind, book, &item.name)?;
        self.save_sheet(item.kind, &item.name, &main)?;
        Ok((main, placed))
    }

    /// Adds the graph file at `from` to the library under the nearest
    /// free name for its own, with the meshes beside it, and any
    /// other graphs it holds as assets of their own. Gives the name.
    pub fn import(
        &mut self,
        kind: Kind,
        from: &Path,
    ) -> anyhow::Result<String> {
        let mut book = read_book(from)?;
        let beside = from.parent().unwrap_or(Path::new("."));
        self.adopt(&std::mem::take(&mut book.assets), beside)?;
        let stem =
            from.file_stem().map_or(kind.shelf().into(), |stem| {
                stem.to_string_lossy()
            });
        let name = self.free_name(kind, &stem);
        let (main, _) = self.spread(kind, book, &name)?;
        self.save_sheet(kind, &name, &main)?;
        Ok(name)
    }

    /// Writes every sheet of `book` but its entry as a `kind` asset
    /// under a free name, with the graphs they run renamed to match,
    /// and gives the entry's sheet, to be written as `entry`, and the
    /// names written.
    fn spread(
        &mut self,
        kind: Kind,
        mut book: Book,
        entry: &str,
    ) -> anyhow::Result<(Sheet, Vec<String>)> {
        let first = book.entry.clone();
        let first = if book.sheets.contains_key(&first) {
            first
        } else {
            book.sheets
                .keys()
                .next()
                .cloned()
                .context("a book with no graph")?
        };
        let mut main = book.sheets.remove(&first).unwrap_or_default();
        let mut named = BTreeMap::from([(first, entry.to_owned())]);
        let mut placed = Vec::new();
        for (sheet, graph) in &book.sheets {
            let mut wanted = sheet.clone();
            let name = loop {
                let free = self.free_name(kind, &wanted);
                if free != entry {
                    break free;
                }
                wanted = format!("{free}_2");
            };
            self.save_sheet(kind, &name, graph)?;
            named.insert(sheet.clone(), name.clone());
            placed.push(name);
        }
        runs_renamed(&mut main, &named);
        for (sheet, mut graph) in book.sheets {
            if runs_renamed(&mut graph, &named) {
                self.save_sheet(kind, &named[&sheet], &graph)?;
            }
        }
        Ok((main, placed))
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
                fs::copy(mesh_path(from, file)?, &to)
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
        self.refuse_named_mesh(item)?;
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
            if let Some(source) = self.sources.remove(&item.name) {
                self.sources.insert(name.clone(), source);
                self.write_sources()?;
            }
        }
        if self.collections.of(item.kind, &item.name).is_some() {
            self.collections.renamed(item.kind, &item.name, &name);
            self.write_collections()?;
        }
        self.scan();
        Ok(name)
    }

    /// Moves `item`'s file to the library's `.trash/` folder.
    pub fn remove(&mut self, item: &Item) -> anyhow::Result<()> {
        let Source::File(from) = &item.source else {
            anyhow::bail!("{} cannot be removed", item.name);
        };
        self.refuse_named_mesh(item)?;
        let root = self.root.as_deref().context("no library yet")?;
        let trash = root.join(".trash").join(item.kind.shelf());
        fs::create_dir_all(&trash)?;
        let file = from.file_name().context("a file with no name")?;
        let mut to = trash.join(file);
        let mut copy = 2;
        while to.exists() {
            to = trash.join(format!(
                "{}_{copy}.{}",
                item.name,
                item.kind.extension()
            ));
            copy += 1;
        }
        fs::rename(from, &to)
            .with_context(|| format!("moving {}", from.display()))?;
        if item.kind == Kind::Mesh {
            self.catalogue.0.remove(&item.name);
            self.write_catalogue()?;
            if self.sources.remove(&item.name).is_some() {
                self.write_sources()?;
            }
        }
        if self.collections.of(item.kind, &item.name).is_some() {
            self.collections.removed(item.kind, &item.name);
            self.write_collections()?;
        }
        self.scan();
        Ok(())
    }
}

/// The book a graph file's text holds, or a bare sheet as a book.
pub fn parse_book(text: &str) -> Option<Book> {
    Book::from_ron(text).ok().or_else(|| {
        Sheet::from_ron(text)
            .ok()
            .map(|sheet| Book::of("main", sheet))
    })
}

/// The book in the file at `path`.
pub fn read_book(path: &Path) -> anyhow::Result<Book> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    parse_book(&text).with_context(|| {
        format!("{} is not a graph file", path.display())
    })
}

/// The only sheet of `book`, else its entry's.
pub fn lone(mut book: Book) -> Sheet {
    match book.sheets.remove(&book.entry) {
        Some(sheet) => sheet,
        None => book.sheets.into_values().next().unwrap_or_default(),
    }
}

/// Points each node of `sheet` that runs a graph at its new name in
/// `named`, and whether any changed.
fn runs_renamed(
    sheet: &mut Sheet,
    named: &BTreeMap<String, String>,
) -> bool {
    let mut changed = false;
    for node in &mut sheet.graph.nodes {
        if let Does::Graph(name) = &mut node.does
            && let Some(new) = named.get(name)
            && new != name
        {
            name.clone_from(new);
            changed = true;
        }
    }
    changed
}

/// Where the file at `path`, as a mesh names it, is in the folder
/// `root`.
pub fn mesh_path(root: &Path, path: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        Shape::is_safe_path(path),
        "{path:?} is not a path inside the folder"
    );
    Ok(root.join(path))
}

/// Where `path` is written before it replaces what is there.
pub fn temp_of(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.tmp"))
}

/// Writes `bytes` to `path` so that the file there is the old one or
/// the whole new one, never a part.
pub fn write_atomic(
    path: &Path,
    bytes: impl AsRef<[u8]>,
) -> anyhow::Result<()> {
    let temp = temp_of(path);
    let written = fs::File::create(&temp).and_then(|mut file| {
        file.write_all(bytes.as_ref())?;
        file.sync_all()
    });
    written
        .and_then(|()| fs::rename(&temp, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
        .with_context(|| format!("writing {}", path.display()))
}

/// Where the mesh `name`'s file is, in the library.
pub fn mesh_file(name: &str) -> String {
    format!(
        "{}/{name}.{}",
        Kind::Mesh.shelf(),
        Kind::Mesh.extension()
    )
}

/// Reads the library from the folder the window gives, and again
/// from another when it gives that.
fn follow(
    folder: Option<Res<LibraryFolder>>,
    mut library: ResMut<Library>,
) {
    let Some(folder) = folder else {
        return;
    };
    if library.root.as_ref() == Some(&folder.0) {
        return;
    }
    library.root = Some(folder.0.clone());
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

    use bevy::math::Vec3;
    use hamlet_graph::{Graph, Node, NodeId};
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
                size: Vec3::ONE,
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
        let art = folder.join("art/barrel.glb");
        library.set_source("barrel", Some(art.clone()));

        let name = library.rename(&barrel, "cask").unwrap();
        let cask = mesh(&library, &name).unwrap();
        assert!(folder.join("library/meshes/cask.glb").exists());
        assert!(!library.catalogue.0.contains_key("barrel"));
        // It follows the same file under its new name, read back from
        // the library's folder.
        assert_eq!(
            Library::in_folder(folder.join("library")).sources,
            BTreeMap::from([("cask".to_owned(), art)])
        );

        library.remove(&cask).unwrap();
        assert!(mesh(&library, "cask").is_none());
        assert!(library.sources.is_empty());
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

    #[test]
    fn a_file_of_several_graphs_is_split_without_losing_one() {
        let folder = Scratch::new("library-split");
        let mut library = Library {
            root: Some(folder.join("library")),
            ..Library::default()
        };
        let sheet = |runs: Option<&str>| Sheet {
            graph: Graph {
                nodes: runs
                    .map(|name| Node {
                        id: NodeId(1),
                        does: Does::Graph(name.to_owned()),
                        settings: BTreeMap::new(),
                    })
                    .into_iter()
                    .collect(),
                ..Graph::default()
            },
            ..Sheet::default()
        };
        // An asset of the name the book's own `wall` would take.
        library
            .save_sheet(Kind::Blueprint, "wall", &sheet(None))
            .unwrap();
        let book = Book {
            entry: "house".to_owned(),
            sheets: BTreeMap::from([
                ("house".to_owned(), sheet(Some("wall"))),
                ("wall".to_owned(), sheet(None)),
            ]),
            assets: Catalogue::default(),
        };
        let path = library.path_of(Kind::Blueprint, "house").unwrap();
        fs::write(&path, book.to_ron().unwrap()).unwrap();
        library.scan();

        let house = library.find(Kind::Blueprint, "house").unwrap();
        let (main, split) = library.read(&house).unwrap();

        assert_eq!(split, ["wall_2"]);
        assert_eq!(main, sheet(Some("wall_2")));
        assert_eq!(house.read().unwrap().sheets.len(), 1);
        assert!(library.find(Kind::Blueprint, "wall_2").is_some());
    }

    #[test]
    fn a_new_material_is_listed_and_reads_back_as_it_was_saved() {
        let folder = Scratch::new("library-material");
        let mut library = Library {
            root: Some(folder.join("library")),
            ..Library::default()
        };
        let item = library.create(Kind::Material, "plaster").unwrap();
        assert_eq!(item.material().unwrap(), Material::default());

        let mut material = Material::of("wall");
        material.values.insert(
            "roughness".to_owned(),
            hamlet_graph::Constant::Number(0.8),
        );
        library.save_material(&item.name, &material).unwrap();

        assert!(
            folder.join("library/materials/plaster.ron").exists()
        );
        let again = Library::in_folder(folder.join("library"));
        let found = again.find(Kind::Material, "plaster").unwrap();
        assert_eq!(found.material().unwrap(), material);
    }

    #[test]
    fn an_asset_renamed_stays_in_its_collection_and_leaves_it_when_removed()
     {
        let folder = Scratch::new("library-collections");
        let mut library = Library {
            root: Some(folder.join("library")),
            ..Library::default()
        };
        let item = library.create(Kind::Material, "plaster").unwrap();
        library
            .change_collections(|collections| {
                let path = collections.add("village")?;
                collections.put(
                    Kind::Material,
                    &item.name,
                    Some(&path),
                )
            })
            .unwrap();

        let name = library.rename(&item, "limewash").unwrap();
        let again = Library::in_folder(folder.join("library"));
        assert_eq!(
            again.collections.of(Kind::Material, &name),
            Some("village")
        );
        assert_eq!(
            again.collections.of(Kind::Material, "plaster"),
            None
        );

        let renamed = library.find(Kind::Material, &name).unwrap();
        library.remove(&renamed).unwrap();
        assert!(library.collections.placed.is_empty());
        assert!(library.collections.paths.contains("village"));
    }
}

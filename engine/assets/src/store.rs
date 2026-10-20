//! The store in `<home>/assets/`: the bytes of each version of an
//! asset in a file of their own, and an index of what is known of
//! each. Every file is written beside its place and moved into it, so
//! it is whole or not at all, and an index that cannot be read is
//! never written over.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;
use hamlet_engine_api::{
    Asset, AssetId, AssetInfo, AssetType, Failure, PluginId, TypeName,
};
use hamlet_engine_core::Home;
use serde::{Deserialize, Serialize};

/// The store's folder in the home.
pub const FOLDER: &str = "assets";
/// The index's name in the store's folder.
pub const INDEX: &str = "index.toml";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexFile {
    /// The id the next asset is given.
    next: u64,
    #[serde(default)]
    assets: Vec<AssetInfo>,
}

/// Every asset, as the index has it.
#[derive(Resource, Debug)]
pub struct AssetStore {
    folder: PathBuf,
    next: u64,
    assets: BTreeMap<AssetId, AssetInfo>,
    /// Why the index could not be read. It is left as it is, and
    /// nothing is written.
    held: Option<String>,
}

impl AssetStore {
    /// The store in `home`. A missing index holds no assets.
    pub fn open(home: &Home) -> Self {
        let folder = home.path(FOLDER);
        let read = match fs::read_to_string(folder.join(INDEX)) {
            Ok(text) => toml::from_str::<IndexFile>(&text)
                .map_err(|e| e.message().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(IndexFile {
                    next: 1,
                    assets: Vec::new(),
                })
            }
            Err(e) => Err(e.to_string()),
        };
        let (next, assets, held) = match read {
            Ok(index) => (
                index.next.max(1),
                index
                    .assets
                    .into_iter()
                    .map(|info| (info.id, info))
                    .collect(),
                None,
            ),
            Err(why) => (1, BTreeMap::new(), Some(why)),
        };
        Self {
            folder,
            next,
            assets,
            held,
        }
    }

    /// Why the index could not be read, while it is left as it is.
    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    pub fn get(&self, id: AssetId) -> Option<&AssetInfo> {
        self.assets.get(&id)
    }

    /// The assets of the type `kind`, by id.
    pub fn of_type<'a>(
        &'a self,
        kind: &'a TypeName,
    ) -> impl Iterator<Item = &'a AssetInfo> {
        self.assets
            .values()
            .filter(move |info| info.kind.name == *kind)
    }

    /// How many assets `owner` has.
    pub fn owned(&self, owner: &PluginId) -> usize {
        self.assets
            .values()
            .filter(|info| info.owner == *owner)
            .count()
    }

    /// The asset `id` and its bytes, of at most `most`.
    pub fn read(
        &self,
        id: AssetId,
        most: u64,
    ) -> Result<Asset, Failure> {
        let info = self.get(id).ok_or(Failure::NotFound)?;
        let path = self.bytes_path(id, info.version);
        let unread = |why: String| {
            Failure::Failed(format!(
                "asset {} cannot be read: {why}",
                id.0
            ))
        };
        let size = fs::metadata(&path)
            .map_err(|e| unread(e.to_string()))?
            .len();
        if size > most {
            return Err(unread(format!(
                "it has more than {most} bytes"
            )));
        }
        let bytes =
            fs::read(&path).map_err(|e| unread(e.to_string()))?;
        Ok(Asset {
            info: info.clone(),
            bytes,
        })
    }

    /// Keeps a new asset of `owner`, of the type `kind`.
    pub fn create(
        &mut self,
        owner: PluginId,
        kind: AssetType,
        name: String,
        bytes: &[u8],
    ) -> Result<AssetId, Failure> {
        self.writable()?;
        let id = AssetId(self.next);
        let path = self.bytes_path(id, 1);
        write_whole(&path, bytes).map_err(unwritten)?;
        self.next += 1;
        self.assets.insert(
            id,
            AssetInfo {
                id,
                kind,
                owner,
                name,
                version: 1,
            },
        );
        if let Err(why) = self.save() {
            self.assets.remove(&id);
            self.next -= 1;
            let _ = fs::remove_file(&path);
            return Err(why);
        }
        Ok(id)
    }

    /// Keeps `bytes` as the next version of the asset `id`: that
    /// version.
    pub fn update(
        &mut self,
        id: AssetId,
        bytes: &[u8],
    ) -> Result<u64, Failure> {
        self.writable()?;
        let was = self.get(id).ok_or(Failure::NotFound)?.version;
        let path = self.bytes_path(id, was + 1);
        write_whole(&path, bytes).map_err(unwritten)?;
        self.set_version(id, was + 1);
        if let Err(why) = self.save() {
            self.set_version(id, was);
            let _ = fs::remove_file(&path);
            return Err(why);
        }
        let _ = fs::remove_file(self.bytes_path(id, was));
        Ok(was + 1)
    }

    /// Takes the asset `id` away.
    pub fn delete(&mut self, id: AssetId) -> Result<(), Failure> {
        self.writable()?;
        let info =
            self.assets.remove(&id).ok_or(Failure::NotFound)?;
        if let Err(why) = self.save() {
            self.assets.insert(id, info);
            return Err(why);
        }
        let _ = fs::remove_file(self.bytes_path(id, info.version));
        Ok(())
    }

    fn set_version(&mut self, id: AssetId, version: u64) {
        if let Some(info) = self.assets.get_mut(&id) {
            info.version = version;
        }
    }

    fn writable(&self) -> Result<(), Failure> {
        match &self.held {
            Some(why) => Err(Failure::Failed(format!(
                "{FOLDER}/{INDEX} cannot be read ({why}), so \
                 nothing is written"
            ))),
            None => Ok(()),
        }
    }

    fn bytes_path(&self, id: AssetId, version: u64) -> PathBuf {
        self.folder.join(format!("{}_{version}.bin", id.0))
    }

    fn save(&self) -> Result<(), Failure> {
        let index = IndexFile {
            next: self.next,
            assets: self.assets.values().cloned().collect(),
        };
        let text = toml::to_string(&index)
            .map_err(|e| Failure::Failed(e.to_string()))?;
        write_whole(&self.folder.join(INDEX), text.as_bytes())
            .map_err(unwritten)
    }
}

fn unwritten(why: std::io::Error) -> Failure {
    Failure::Failed(format!("the store cannot be written: {why}"))
}

/// Writes `bytes` beside `path` and moves them over it, so a stop in
/// the middle leaves the old file or the new one.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map_or_else(String::new, |name| {
        name.to_string_lossy().into_owned()
    });
    let saving = path.with_file_name(format!("saving_{name}"));
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut file = fs::File::create(&saving)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&saving, path)
}

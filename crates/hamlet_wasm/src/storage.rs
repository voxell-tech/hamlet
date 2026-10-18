//! A plugin's private store of keys and values: one file for each
//! key in a folder of the plugin's own, under caps.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::limits::Limits;

/// The folder of a plugin's folder that holds its storage.
pub const DATA: &str = "data";

/// The start of the name of a file that holds a value.
const VALUE: &str = "key-";
/// The start of the name of a file being written.
const WRITING: &str = "tmp-";

/// What a store may hold.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Caps {
    pub bytes: u64,
    pub keys: usize,
    pub key_bytes: usize,
    pub value_bytes: usize,
}

impl Caps {
    pub fn of(limits: &Limits) -> Self {
        Self {
            bytes: limits.storage_bytes,
            keys: limits.storage_keys,
            key_bytes: limits.storage_key_bytes,
            value_bytes: limits.storage_value_bytes,
        }
    }
}

/// The values of one plugin, in `folder`. A key is of the bytes
/// `a-z`, `0-9`, `.`, `_` and `-` and does not start with `.`, so it
/// is its own file name and cannot name a place outside the folder.
#[derive(Clone, PartialEq, Debug)]
pub struct Storage {
    folder: PathBuf,
    caps: Caps,
}

impl Storage {
    pub fn new(folder: PathBuf, caps: Caps) -> Self {
        Self { folder, caps }
    }

    /// The value of `key`, if it is there.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.check(key).ok()?;
        fs::read(self.file(key)).ok()
    }

    /// Sets `key` to `value`. A file is written beside it and renamed
    /// over it, so a value is whole or not there.
    pub fn set(&self, key: &str, value: &[u8]) -> Result<(), String> {
        self.check(key)?;
        if value.len() > self.caps.value_bytes {
            return Err(format!(
                "a value has {} bytes, the most is {}",
                value.len(),
                self.caps.value_bytes
            ));
        }
        let (mut keys, mut bytes) = (0, 0);
        let mut old = None;
        for (name, size) in self.entries() {
            keys += 1;
            bytes += size;
            if name == key {
                old = Some(size);
            }
        }
        if old.is_none() && keys >= self.caps.keys {
            return Err(format!(
                "the store holds {keys} keys, the most is {}",
                self.caps.keys
            ));
        }
        let after = bytes - old.unwrap_or(0) + value.len() as u64;
        if after > self.caps.bytes {
            return Err(format!(
                "the store would hold {after} bytes, the most is {}",
                self.caps.bytes
            ));
        }
        self.write(key, value)
            .map_err(|_| "the value could not be written".to_string())
    }

    /// Removes `key`, if it is there.
    pub fn delete(&self, key: &str) {
        if self.check(key).is_ok() {
            let _ = fs::remove_file(self.file(key));
        }
    }

    /// The keys that start with `prefix`, in order.
    pub fn keys(&self, prefix: &str) -> Vec<String> {
        let mut keys = self
            .entries()
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| name.starts_with(prefix))
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    fn file(&self, key: &str) -> PathBuf {
        self.folder.join(format!("{VALUE}{key}"))
    }

    /// Why `key` cannot be a key, if it cannot.
    fn check(&self, key: &str) -> Result<(), String> {
        let good = |byte: &u8| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-');
        if key.is_empty()
            || key.len() > self.caps.key_bytes
            || key.starts_with('.')
            || !key.as_bytes().iter().all(good)
        {
            return Err(format!(
                "`{}` is not a key: 1 to {} bytes of a-z, 0-9, `.`, \
                 `_` and `-`, not starting with `.`",
                key.escape_debug(),
                self.caps.key_bytes
            ));
        }
        Ok(())
    }

    /// Each key held, with the bytes of its value.
    fn entries(&self) -> Vec<(String, u64)> {
        let Ok(dir) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        dir.filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let key = name.strip_prefix(VALUE)?.to_string();
                let size = entry.metadata().ok()?.len();
                Some((key, size))
            })
            .collect()
    }

    fn write(&self, key: &str, value: &[u8]) -> std::io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        let temp = self.folder.join(format!("{WRITING}{key}"));
        let mut file = fs::File::create(&temp)?;
        let written =
            file.write_all(value).and_then(|()| file.sync_all());
        drop(file);
        let done =
            written.and_then(|()| fs::rename(&temp, self.file(key)));
        if done.is_err() {
            let _ = fs::remove_file(&temp);
        }
        done
    }
}

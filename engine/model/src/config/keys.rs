//! An endpoint's API key, and the other secrets of a home: in the OS
//! keychain when one is there, in a private file of the home
//! otherwise.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;

const KEYCHAIN_SERVICE: &str = "hamlet";

/// The keychain service this process keeps its secrets under, when it
/// is not [`KEYCHAIN_SERVICE`].
static SERVICE: OnceLock<&'static str> = OnceLock::new();

/// Keeps every secret of this process under the keychain service
/// `name`, so two apps with homes of their own never read or delete
/// each other's keys. Set once, before any secret is read.
pub fn use_keychain_service(name: &'static str) {
    let _ = SERVICE.set(name);
}

fn service() -> &'static str {
    SERVICE.get().copied().unwrap_or(KEYCHAIN_SERVICE)
}

/// The secrets read or kept this run, by the file each would be
/// kept in. The keychain may ask the chief for a password at each
/// read, so each secret is read from it once.
static KNOWN: Mutex<BTreeMap<PathBuf, String>> =
    Mutex::new(BTreeMap::new());

/// Remembers `secret` as the one kept in `file`, or that none is.
fn know(file: PathBuf, secret: Option<&str>) {
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    match secret {
        Some(secret) => known.insert(file, secret.to_string()),
        None => known.remove(&file),
    };
}

/// The name the key of `endpoint` is kept under.
fn key_name(endpoint: &str) -> String {
    format!("api_key.{endpoint}")
}

pub(super) fn load_key(
    home: &Path,
    endpoint: &str,
) -> Option<String> {
    load_secret(home, &key_name(endpoint))
}

pub(super) fn save_key(
    home: &Path,
    endpoint: &str,
    key: Option<&str>,
) -> Result<()> {
    save_secret(home, &key_name(endpoint), key)
}

/// The secret kept under `name`, in the keychain or in the file of
/// that name in the home.
pub(crate) fn load_secret(home: &Path, name: &str) -> Option<String> {
    let file = home.join(name);
    let known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(secret) = known.get(&file) {
        return Some(secret.clone());
    }
    drop(known);
    let read = keyring::Entry::new(service(), name)
        .and_then(|e| e.get_password());
    let secret = match read {
        Ok(k) if !k.is_empty() => Some(k),
        _ => std::fs::read_to_string(&file)
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty()),
    };
    // One not found is looked for again, as finding none asks for
    // no password.
    if let Some(secret) = &secret {
        know(file, Some(secret));
    }
    secret
}

/// Keeps `secret` under `name`, or forgets the one kept there.
pub(crate) fn save_secret(
    home: &Path,
    name: &str,
    secret: Option<&str>,
) -> Result<()> {
    let entry = keyring::Entry::new(service(), name);
    let file = home.join(name);
    let Some(secret) = secret.filter(|k| !k.is_empty()) else {
        know(file.clone(), None);
        if let Ok(e) = entry {
            let _ = e.delete_credential();
        }
        let _ = std::fs::remove_file(file);
        return Ok(());
    };
    if entry.and_then(|e| e.set_password(secret)).is_ok() {
        let _ = std::fs::remove_file(&file);
    } else {
        write_private(&file, secret)?;
    }
    know(file, Some(secret));
    Ok(())
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(contents.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn key_file_fallback_is_private() {
        let dir = Scratch::new("config");
        let file = dir.join(key_name("far"));
        write_private(&file, "k").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}

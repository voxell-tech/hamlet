//! An endpoint's API key: in the OS keychain when one is there, in a
//! private file of the home otherwise.

use std::path::{Path, PathBuf};

use anyhow::Result;

const KEYCHAIN_SERVICE: &str = "hamlet";
const KEYCHAIN_USER: &str = "api_key";

/// The file that keeps the key of `endpoint` when there is no
/// keychain.
fn key_file(home: &Path, endpoint: &str) -> PathBuf {
    home.join(format!("api_key.{endpoint}"))
}

/// The name a key is kept under in the keychain.
fn key_user(endpoint: &str) -> String {
    format!("{KEYCHAIN_USER}.{endpoint}")
}

pub(super) fn load_key(
    home: &Path,
    endpoint: &str,
) -> Option<String> {
    let read = |service: &str| {
        keyring::Entry::new(service, &key_user(endpoint))
            .and_then(|e| e.get_password())
    };
    match read(KEYCHAIN_SERVICE) {
        Ok(k) if !k.is_empty() => Some(k),
        _ => std::fs::read_to_string(key_file(home, endpoint))
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty()),
    }
}

pub(super) fn save_key(
    home: &Path,
    endpoint: &str,
    key: Option<&str>,
) -> Result<()> {
    let entry =
        keyring::Entry::new(KEYCHAIN_SERVICE, &key_user(endpoint));
    let file = key_file(home, endpoint);
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        if let Ok(e) = entry {
            let _ = e.delete_credential();
        }
        let _ = std::fs::remove_file(file);
        return Ok(());
    };
    if entry.and_then(|e| e.set_password(key)).is_ok() {
        let _ = std::fs::remove_file(file);
        return Ok(());
    }
    write_private(&file, key)
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
        write_private(&key_file(&dir, "far"), "k").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_file(&dir, "far"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}

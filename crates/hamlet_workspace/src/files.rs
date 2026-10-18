//! File operations executed only inside the operation namespace.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::wire::{MAX_BODY, MAX_FILE, Operation, Reply};

pub fn run() -> Result<()> {
    let operation =
        serde_json::from_reader(io::stdin().take(MAX_BODY as u64))?;
    let reply = match perform(operation) {
        Ok(value) => Reply::ok(value),
        Err(error) => {
            let code = match error.kind() {
                io::ErrorKind::NotFound => "not_found",
                io::ErrorKind::PermissionDenied => {
                    "permission_denied"
                }
                io::ErrorKind::NotADirectory => "not_directory",
                io::ErrorKind::IsADirectory => "is_directory",
                io::ErrorKind::InvalidInput => "invalid",
                io::ErrorKind::Unsupported => "not_supported",
                _ => "unknown",
            };
            Reply::error(
                code,
                "The workspace file operation failed. Check the \
                 path and current contents before repeating a write.",
                false,
            )
        }
    };
    serde_json::to_writer(io::stdout(), &reply)?;
    Ok(())
}

fn perform(operation: Operation) -> io::Result<Value> {
    match operation {
        Operation::TreeList {
            path,
            entries,
            bytes,
        } => {
            if entries > 20_000 || bytes > 512 * 1024 * 1024 {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            let mut result = Vec::new();
            let mut remaining = bytes;
            let root = Path::new(&path);
            tree(root, root, entries, &mut remaining, &mut result)?;
            return Ok(json!(result));
        }
        Operation::TreeWrite { path, data, mode } => {
            let bytes = STANDARD
                .decode(data)
                .map_err(|_| io::ErrorKind::InvalidInput)?;
            if bytes.len() > MAX_FILE || mode > 0o777 {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            let path = Path::new(&path);
            let parent =
                path.parent().ok_or(io::ErrorKind::InvalidInput)?;
            fs::create_dir_all(parent)?;
            let mut current = std::path::PathBuf::new();
            for part in parent.components() {
                current.push(part);
                if !fs::symlink_metadata(&current)?.is_dir() {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(path)?;
            file.write_all(&bytes)?;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            file.sync_all()?;
        }
        Operation::InitHouse => {
            for path in ["/house/skills", "/house/inbox"] {
                match fs::create_dir(path) {
                    Ok(()) => {}
                    Err(error)
                        if error.kind()
                            == io::ErrorKind::AlreadyExists =>
                    {
                        if !fs::symlink_metadata(path)?.is_dir() {
                            return Err(
                                io::ErrorKind::InvalidInput.into()
                            );
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open("/house/NOTES.md")
            {
                Ok(mut file) => {
                    file.write_all(b"# Notes\n")?;
                    file.sync_all()?;
                }
                Err(error)
                    if error.kind()
                        == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Operation::Exec { .. } => {
            return Err(io::ErrorKind::Unsupported.into());
        }
        Operation::Read { path, no_follow } => {
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(
                    libc::O_NONBLOCK
                        | if no_follow {
                            libc::O_NOFOLLOW
                        } else {
                            0
                        },
                )
                .open(&path)
                .map_err(|error| {
                    // O_NOFOLLOW refuses a final symlink with ELOOP.
                    if error.raw_os_error() == Some(libc::ELOOP) {
                        io::ErrorKind::InvalidInput.into()
                    } else {
                        error
                    }
                })?;
            let metadata = file.metadata()?;
            if metadata.is_dir() {
                return Err(io::ErrorKind::IsADirectory.into());
            }
            if !metadata.is_file() || metadata.len() > MAX_FILE as u64
            {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            let mut data = Vec::new();
            (&mut file)
                .take(MAX_FILE as u64 + 1)
                .read_to_end(&mut data)?;
            if data.len() > MAX_FILE {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            return Ok(
                json!({ "data": STANDARD.encode(data), "info": info(Path::new(&path), &metadata)? }),
            );
        }
        Operation::Info { path } => {
            return info(
                Path::new(&path),
                &fs::symlink_metadata(&path)?,
            );
        }
        Operation::Canonical { path } => {
            return Ok(json!(
                fs::canonicalize(path)?.to_string_lossy()
            ));
        }
        Operation::List { path } => {
            let mut entries = Vec::new();
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                match fs::symlink_metadata(entry.path()).and_then(
                    |metadata| info(&entry.path(), &metadata),
                ) {
                    Ok(value) => entries.push(value),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound
                                | io::ErrorKind::Unsupported
                        ) => {}
                    Err(error) => return Err(error),
                }
                if entries.len() > 10_000 {
                    return Err(io::ErrorKind::Unsupported.into());
                }
            }
            return Ok(json!(entries));
        }
        Operation::Write { path, data, append } => {
            let bytes = STANDARD
                .decode(data)
                .map_err(|_| io::ErrorKind::InvalidInput)?;
            if bytes.len() > MAX_FILE {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!append)
                .append(append)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)?;
            if !file.metadata()?.is_file() {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            file.write_all(&bytes)?;
        }
        Operation::Mkdir { path, recursive } => {
            if recursive {
                fs::create_dir_all(path)?;
            } else {
                fs::create_dir(path)?;
            }
        }
        Operation::Remove {
            path,
            recursive,
            force,
        } => {
            let result =
                fs::symlink_metadata(&path).and_then(|metadata| {
                    if metadata.is_dir() {
                        if recursive {
                            fs::remove_dir_all(path)
                        } else {
                            fs::remove_dir(path)
                        }
                    } else {
                        fs::remove_file(path)
                    }
                });
            if let Err(error) = result
                && (!force || error.kind() != io::ErrorKind::NotFound)
            {
                return Err(error);
            }
        }
        Operation::Rename { path, destination } => {
            fs::rename(path, destination)?
        }
        Operation::Truncate { path, size } => {
            if size > MAX_FILE as u64 {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)?
                .set_len(size)?;
        }
        Operation::Flush { path } => {
            fs::File::open(path)?.sync_all()?
        }
    }
    Ok(Value::Null)
}

fn tree(
    root: &Path,
    path: &Path,
    cap: u64,
    remaining: &mut u64,
    result: &mut Vec<Value>,
) -> io::Result<()> {
    if result.len() as u64 >= cap {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let metadata = fs::symlink_metadata(path)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::ErrorKind::InvalidInput)?;
    let kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else if metadata.is_symlink() {
        "link"
    } else {
        "special"
    };
    let mut entry = json!({"path": if relative.as_os_str().is_empty() { ".".into() } else { relative.to_string_lossy() }, "kind": kind, "mode": metadata.permissions().mode() & 0o777, "size": metadata.len()});
    if metadata.is_file() {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut size = 0;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            *remaining = remaining
                .checked_sub(count as u64)
                .ok_or(io::ErrorKind::InvalidInput)?;
            size += count;
            digest.update(&buffer[..count]);
        }
        entry["size"] = json!(size);
        entry["sha256"] = json!(format!("{:x}", digest.finalize()));
    }
    result.push(entry);
    if metadata.is_dir() {
        for child in fs::read_dir(path)? {
            tree(root, &child?.path(), cap, remaining, result)?;
        }
    }
    Ok(())
}

fn info(path: &Path, metadata: &fs::Metadata) -> io::Result<Value> {
    let kind = if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "directory"
    } else if metadata.is_symlink() {
        "symlink"
    } else {
        return Err(io::ErrorKind::Unsupported.into());
    };
    Ok(
        json!({ "name": path.file_name().unwrap_or_default().to_string_lossy(), "path": path.to_string_lossy(), "kind": kind,
        "size": metadata.len(), "mtimeMs": metadata.modified()?.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }),
    )
}

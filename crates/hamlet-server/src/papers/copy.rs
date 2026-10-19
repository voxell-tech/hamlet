//! Verified copies between house backends, including empty
//! directories and ordinary modes.

use std::collections::BTreeMap;
use std::fs::File;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use anyhow::{Result, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::files::{Files, TreeEntry, remote_entries};
use super::tree::{self, Entry};
use crate::workspace::Remote;

pub(crate) struct Snapshot {
    pub entries: Vec<TreeEntry>,
    pub files: BTreeMap<String, Entry>,
    pub skipped: Vec<String>,
}

impl Snapshot {
    pub async fn read(
        source: &Files,
        cap: u64,
        count: usize,
        skip: bool,
    ) -> Result<Self> {
        let entries = match source {
            Files::Local(root) => {
                let root = root.try_clone()?;
                tokio::task::spawn_blocking(move || {
                    local_entries(&root, cap, count)
                })
                .await??
            }
            Files::Remote { env, path } => {
                remote_entries(env, path, cap, count).await?
            }
        };
        let mut result = Self {
            entries: Vec::new(),
            files: BTreeMap::new(),
            skipped: Vec::new(),
        };
        for entry in entries {
            if !["file", "directory"].contains(&entry.kind.as_str()) {
                ensure!(
                    skip,
                    "A skill gift cannot include links or special \
                     files."
                );
                result.skipped.push(entry.path);
                continue;
            }
            if entry.kind == "file" {
                let bytes = match source {
                    Files::Local(root) => {
                        let root = root.try_clone()?;
                        let path = entry.path.clone();
                        tokio::task::spawn_blocking(move || {
                            tree::read_file(&root, &path, cap)
                        })
                        .await??
                    }
                    Files::Remote { env, path } => env
                        .read(&format!("{path}/{}", entry.path), true)
                        .await
                        .map_err(|error| {
                            anyhow::anyhow!("{}", error.message)
                        })?,
                };
                ensure!(
                    entry.size == bytes.len() as u64
                        && entry.sha256.as_deref()
                            == Some(&format!(
                                "{:x}",
                                Sha256::digest(&bytes)
                            )),
                    "The tree changed while it was copied. Inspect \
                     it before retrying."
                );
                result.files.insert(
                    entry.path.clone(),
                    Entry {
                        bytes,
                        mode: entry.mode,
                    },
                );
            }
            result.entries.push(entry);
        }
        result.entries.sort_by_key(|entry| {
            (entry.path.split('/').count(), entry.path.clone())
        });
        Ok(result)
    }

    pub async fn deliver(
        &self,
        env: &Remote,
        into: &str,
        roots: &[String],
        number: bool,
    ) -> Result<Vec<String>> {
        ensure!(
            self.files
                .values()
                .all(|file| file.bytes.len()
                    <= crate::workspace::MAX_FILE),
            "Remote files are limited to 4 MiB per transfer."
        );
        env.parents(&format!("{into}/entry"), false)
            .await
            .map_err(|error| anyhow::anyhow!("{}", error.message))?;
        let mut targets = Vec::new();
        for root in roots {
            let mut chosen = root.clone();
            let mut count = 1;
            while env
                .exists(&format!("{into}/{chosen}"))
                .await
                .map_err(|error| {
                    anyhow::anyhow!("{}", error.message)
                })?
            {
                ensure!(number, "gift target already exists");
                count += 1;
                let split = root
                    .rfind('.')
                    .filter(|i| *i > 0)
                    .unwrap_or(root.len());
                chosen = format!(
                    "{}-{count}{}",
                    &root[..split],
                    &root[split..]
                );
            }
            ensure!(
                !targets.contains(&chosen),
                "Gift names collide after numbering; no files were \
                 delivered."
            );
            targets.push(chosen);
        }
        let mapped = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (name, rest) = entry
                    .path
                    .split_once('/')
                    .unwrap_or((&entry.path, ""));
                let index =
                    roots.iter().position(|root| root == name)?;
                let path = format!(
                    "{into}/{}{}",
                    targets[index],
                    if rest.is_empty() {
                        String::new()
                    } else {
                        format!("/{rest}")
                    }
                );
                Some((entry, path))
            })
            .collect::<Vec<_>>();
        let mut claimed = Vec::new();
        let result = async {
            for (entry, path) in &mapped {
                env.parents(path, false).await.map_err(|error| anyhow::anyhow!("{}", error.message))?;
                if entry.kind == "directory" {
                    env.call::<()>(json!({"kind":"mkdir","path":path,"recursive":false})).await?;
                } else {
                    let file = &self.files[&entry.path];
                    ensure!(file.bytes.len() <= crate::workspace::MAX_FILE, "Remote files are limited to 4 MiB per transfer.");
                    env.call::<()>(json!({"kind":"tree_write","path":path,"mode":entry.mode & 0o777,"data":STANDARD.encode(&file.bytes)})).await?;
                }
                claimed.push(path.clone());
            }
            Ok::<_, anyhow::Error>(())
        }.await;
        if let Err(error) = result {
            let uncertain = error
                .downcast_ref::<crate::workspace::Failure>()
                .is_some_and(|error| {
                    error.uncertain
                        || ["unknown", "aborted"]
                            .contains(&error.code)
                });
            let mut complete = true;
            for path in claimed.into_iter().rev() {
                complete &= env.call::<()>(json!({"kind":"remove","path":path,"recursive":false,"force":false})).await.is_ok();
            }
            if uncertain {
                anyhow::bail!(
                    "The workspace did not confirm the tree \
                     operation, so part of it may have happened. \
                     Inspect the target before repeating it."
                );
            }
            ensure!(
                complete,
                "The tree operation stopped after changing part of \
                 its target. Inspect the target and keep or remove \
                 what is there before repeating it."
            );
            return Err(error);
        }
        for (entry, path) in mapped
            .iter()
            .rev()
            .filter(|(entry, _)| entry.kind == "directory")
        {
            chmod(env, path, entry.mode).await?;
        }
        for (name, chosen) in roots.iter().zip(&targets) {
            let actual = remote_entries(
                env,
                &format!("{into}/{chosen}"),
                512 * 1024 * 1024,
                20_000,
            )
            .await?;
            let expected = self
                .entries
                .iter()
                .filter(|entry| {
                    entry.path == *name
                        || entry.path.starts_with(&format!("{name}/"))
                })
                .collect::<Vec<_>>();
            ensure!(
                actual.len() == expected.len(),
                "The remote delivery changed before verification. \
                 Inspect the target before repeating it."
            );
            for expected in expected {
                let relative = expected
                    .path
                    .strip_prefix(&format!("{name}/"))
                    .unwrap_or(".");
                ensure!(
                    actual.iter().any(|entry| entry.path == relative
                        && same(entry, expected)),
                    "The remote delivery changed before \
                     verification. Inspect the target before \
                     repeating it."
                );
            }
        }
        Ok(targets)
    }

    pub fn write_local(&self, root: &File) -> Result<()> {
        for entry in &self.entries {
            if entry.path == "." {
                continue;
            }
            if entry.kind == "directory" {
                directory(root, &entry.path)?;
            } else {
                tree::put(
                    root,
                    &entry.path,
                    &self.files[&entry.path],
                )?;
            }
        }
        for entry in self
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.kind == "directory")
        {
            let dir = if entry.path == "." {
                root.try_clone()?
            } else {
                directory(root, &entry.path)?
            };
            dir.set_permissions(std::fs::Permissions::from_mode(
                entry.mode & 0o777,
            ))?;
        }
        Ok(())
    }
}

pub(crate) async fn chmod(
    env: &Remote,
    path: &str,
    mode: u32,
) -> Result<()> {
    let reply = env.call::<serde_json::Value>(json!({"kind":"exec","argv":["/usr/bin/chmod","--",format!("{:o}", mode & 0o777),path],"cwd":"/house","timeout_ms":120000})).await?;
    ensure!(
        reply["exitCode"] == 0,
        "The workspace did not confirm the file mode. Inspect the \
         target before repeating it."
    );
    Ok(())
}

pub(crate) fn same(a: &TreeEntry, b: &TreeEntry) -> bool {
    a.kind == b.kind
        && a.mode & 0o777 == b.mode & 0o777
        && (a.kind != "file"
            || (a.size == b.size && a.sha256 == b.sha256))
}

fn directory(root: &File, path: &str) -> Result<File> {
    let mut dir = root.try_clone()?;
    for part in path.split('/') {
        dir = tree::directory(&dir, part, true)?;
    }
    Ok(dir)
}

pub(crate) fn local_entries(
    root: &File,
    cap: u64,
    count: usize,
) -> Result<Vec<TreeEntry>> {
    use rustix::fs::{AtFlags, Dir, FileType, statat};
    let mut entries = vec![TreeEntry {
        path: ".".into(),
        kind: "directory".into(),
        size: 0,
        mode: root.metadata()?.mode() & 0o777,
        sha256: None,
    }];
    let mut todo = vec![(root.try_clone()?, String::new())];
    let mut left = cap;
    while let Some((dir, prefix)) = todo.pop() {
        for item in Dir::read_from(&dir)? {
            let item = item?;
            let name = item.file_name().to_str()?;
            if [".", ".."].contains(&name) {
                continue;
            }
            ensure!(
                entries.len() < count,
                "The tree has too many entries."
            );
            let path = format!("{prefix}{name}");
            let info = statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
            let kind = match FileType::from_raw_mode(info.st_mode) {
                FileType::Directory => "directory",
                FileType::RegularFile if info.st_nlink == 1 => "file",
                FileType::Symlink => "link",
                _ => "special",
            };
            let (size, sha256) = if kind == "file" {
                let bytes = tree::read_file(&dir, name, left)?;
                left -= bytes.len() as u64;
                (
                    bytes.len() as u64,
                    Some(format!("{:x}", Sha256::digest(&bytes))),
                )
            } else {
                (0, None)
            };
            if kind == "directory" {
                todo.push((
                    tree::directory(&dir, name, false)?,
                    format!("{path}/"),
                ));
            }
            // macOS mode_t is u16; Linux mode_t is u32.
            #[allow(clippy::useless_conversion)]
            let mode = u32::from(info.st_mode) & 0o777;
            entries.push(TreeEntry {
                path,
                kind: kind.into(),
                size,
                mode,
                sha256,
            });
        }
    }
    Ok(entries)
}

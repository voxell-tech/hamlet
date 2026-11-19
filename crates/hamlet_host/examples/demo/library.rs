//! The library's commands, for a demo started with `--realm-actor`:
//! each reaches the real library, `library/local`, as a hire's own
//! agent through its demo-only `forward`.
//!
//! ```text
//! demo library shelves <agent>        the list, as agent <agent>
//!                                     asks for it, listening after
//! demo library offer <agent> <call> <skill> <folder>
//!                                     offer the files under <folder>
//!                                     as the skill <skill>, in parts,
//!                                     as agent <agent>'s call <call>
//! ```
//!
//! Each prints the answer as `<action> <answer>`; `offer` prints one
//! line for each part.

use std::path::Path;

use anyhow::{Context, Result, bail};
use hamlet_host::Host;
use hamlet_messages::bytes::Base64;
use hamlet_messages::library::{Offer, Shelves};
use hamlet_messages::profile::Piece;
use hamlet_messages::workspace::CallId;
use hamlet_messages::{MOST_PART, PIECE_FRAME, address};

use crate::agent_host::forward;
use crate::profile::{agent, shown};

pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    match words {
        [verb, me] if verb == "shelves" => {
            let shelves = Shelves { version_seen: None };
            let list = forward(
                host,
                agent(me)?,
                &address::Library,
                &shelves,
            )
            .await?;
            shown(verb, &list)
        }
        [verb, me, call, skill, folder] if verb == "offer" => {
            let parts = parts(Path::new(folder))?;
            let count = parts.len();
            for (part, files) in parts.into_iter().enumerate() {
                let offer = Offer {
                    call: CallId(call.clone()),
                    skill: skill.clone(),
                    reason: "offered from the demo".into(),
                    part: u32::try_from(part)?,
                    last: part + 1 == count,
                    files,
                };
                let kept = forward(
                    host,
                    agent(me)?,
                    &address::Library,
                    &offer,
                )
                .await?;
                shown(verb, &kept)?;
            }
            Ok(())
        }
        _ => bail!("unknown library command: {}", words.join(" ")),
    }
}

/// The files under `folder` as pieces, packed into parts of at most
/// [`MOST_PART`], a file too large for one part cut across several.
fn parts(folder: &Path) -> Result<Vec<Vec<Piece>>> {
    let mut parts = vec![Vec::new()];
    let mut room = MOST_PART;
    for (path, bytes, executable) in files(folder, folder)? {
        let mut offset = 0;
        loop {
            let head = path.len() + PIECE_FRAME;
            if room <= head {
                parts.push(Vec::new());
                room = MOST_PART;
            }
            let sent = (bytes.len() - offset).min(room - head);
            let last = offset + sent == bytes.len();
            parts.last_mut().context("a part")?.push(Piece {
                path: path.clone(),
                offset: u32::try_from(offset)?,
                bytes: Base64(bytes[offset..offset + sent].to_vec()),
                last,
                executable,
            });
            room -= head + sent;
            offset += sent;
            if last {
                break;
            }
        }
    }
    Ok(parts)
}

/// Every file under `at`, by its path from `root`.
fn files(
    root: &Path,
    at: &Path,
) -> Result<Vec<(String, Vec<u8>, bool)>> {
    let mut found = Vec::new();
    let mut entries =
        std::fs::read_dir(at)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            found.extend(files(root, &path)?);
            continue;
        }
        let name = path
            .strip_prefix(root)?
            .to_str()
            .context("a path in UTF-8")?
            .replace('\\', "/");
        found.push((name, std::fs::read(&path)?, runs(&path)?));
    }
    Ok(found)
}

#[cfg(unix)]
fn runs(path: &Path) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt as _;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o100 != 0)
}

#[cfg(not(unix))]
fn runs(_: &Path) -> Result<bool> {
    Ok(false)
}

//! The library actor, `library/local`, driven through the stand-ins
//! of `demo/profile.rs` for the realm and the agents.
//!
//! ```text
//! demo library list                   the list, as the realm reads it
//! demo library shelves <agent>        the list, as agent <agent>
//!                                     asks for it, listening after
//! demo library offer <agent> <call> <skill> <folder>
//!                                     offer the files under <folder>
//!                                     as the skill <skill>, in parts,
//!                                     as agent <agent>'s call <call>
//! demo library verdict <offer> accept|refuse [<words ...>]
//! demo library unshelve <skill>
//! ```
//!
//! Each prints the answer as `<action> <answer>`; `offer` prints one
//! line for each part.

use std::path::Path;

use anyhow::{Context, Result, bail};
use hamlet_host::Host;
use hamlet_messages::address::{self, Party};
use hamlet_messages::bytes::Base64;
use hamlet_messages::library::{
    Offer, OfferId, ReadShelves, Shelves, Unshelve, Verdict,
};
use hamlet_messages::profile::Piece;
use hamlet_messages::workspace::CallId;
use hamlet_messages::{MOST_PART, PIECE_FRAME};

use crate::profile::{CHIEF, agent_at, request_id, shown};

pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    let library = address::Library.address()?;
    let realm = address::Realm.address()?;
    host.get_or_start(&library).await?;
    match words {
        [command] if command == "list" => {
            shown(host, &realm, &library, &ReadShelves).await
        }
        [command, agent] if command == "shelves" => {
            let shelves = Shelves { version_seen: None };
            shown(host, &agent_at(agent)?, &library, &shelves).await
        }
        [command, agent, call, skill, folder]
            if command == "offer" =>
        {
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
                shown(host, &agent_at(agent)?, &library, &offer)
                    .await?;
            }
            Ok(())
        }
        [command, offer, verdict, words @ ..]
            if command == "verdict" =>
        {
            let verdict = Verdict {
                id: request_id()?,
                offer: OfferId(offer.parse()?),
                accepted: match verdict.as_str() {
                    "accept" => true,
                    "refuse" => false,
                    _ => bail!("a verdict is accept or refuse"),
                },
                words: words.join(" "),
            };
            shown(host, &realm, &library, &verdict).await
        }
        [command, skill] if command == "unshelve" => {
            let unshelve = Unshelve {
                id: request_id()?,
                skill: skill.clone(),
                reason: None,
                by: CHIEF,
            };
            shown(host, &realm, &library, &unshelve).await
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

//! A profile's commands, for a demo started with `--realm-actor`:
//! each reaches the real profile `profile/<n>` through the realm, as
//! the chief or as an agent's tool, or as a hire's own agent through
//! its demo-only `forward`.
//!
//! ```text
//! demo profile make <lead> <call> <name> [<character ...>]
//!                         have agent <lead> create the profile
//!                         <name>, as its call <call>
//! demo profile <n> tag <version> [<note ...>]
//! demo profile <n> revert <version>   undo the one commit <version>
//!                         names, with a commit on main
//! demo profile <n> edit <path> <text ...>
//! demo profile <n> purge <path>
//! demo profile <n> dream [<hire>]     dream over the profile, or
//!                         over its hire <hire>
//! demo profile <n>,<m> <command ...>
//!                         run the command on two profiles at once,
//!                         printing when each began and ended
//! demo profile <n> fill <hire> <folder>
//!                         ask for the hire's files as its agent, in
//!                         parts, and write them under <folder>
//! demo profile <n> save <hire> <path> <text ...>
//!                         save one file as the hire's agent, on the
//!                         tip of its branch
//! ```
//!
//! `tag`, `revert`, `edit`, `purge` and `dream` ask as the chief.
//! `make` prints `made/<call> <profile>`, or `refused/<call> 1` and
//! logs why. Every other command prints the answer as `<verb>
//! <answer>`, and `fill` prints `fill parts=<n> files=<n>
//! largest=<bytes> tip=<commit>`, `largest` the largest part in CBOR.

use std::fmt::Debug;
use std::io::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_host::Host;
use hamlet_messages::address;
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    FileChange, Files, FilesAnswer, Piece, Revision, Save,
    VersionName,
};
use hamlet_messages::realm::profiles::{
    CreateProfile, Dream, Dreamer, EditProfile, Made, PurgeProfile,
    RevertProfile, TagProfile,
};
use hamlet_messages::realm::{ChiefRequest, ProfileId};
use hamlet_messages::workspace::{AgentNumber, CallId};

use crate::agent_host::forward;
use crate::realm_actor::{agent_number, refused, request, tool};

pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    let [first, rest @ ..] = words else {
        bail!(
            "usage: demo profile make <lead> ... | <n> <command> ..."
        );
    };
    if first == "make" {
        return make(host, rest).await;
    }
    if let Some((one, other)) = first.split_once(',') {
        let (one, other) = tokio::join!(
            timed(host, one, rest),
            timed(host, other, rest)
        );
        return one.and(other);
    }
    on(host, ProfileId(agent_number(first)?), rest).await
}

async fn make(host: &Host, words: &[String]) -> Result<()> {
    let [lead, call, name, character @ ..] = words else {
        bail!(
            "usage: demo profile make <lead> <call> <name> \
             [<character ...>]"
        );
    };
    let create = CreateProfile {
        name: name.clone(),
        character: (!character.is_empty())
            .then(|| character.join(" ")),
    };
    let made = forward(
        host,
        agent(lead)?,
        &address::Realm,
        &tool(call, create),
    )
    .await?;
    match made {
        Made::Made { profile, .. } => {
            writeln!(std::io::stdout(), "made/{call} {}", profile.0)?;
        }
        Made::Refused { why } => refused(call, &why)?,
    }
    Ok(())
}

async fn timed(host: &Host, n: &str, words: &[String]) -> Result<()> {
    let began = micros()?;
    let done = on(host, ProfileId(agent_number(n)?), words).await;
    writeln!(
        std::io::stdout(),
        "timed profile/{n} began={began} ended={}",
        micros()?
    )?;
    done
}

fn micros() -> Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())
}

async fn on(
    host: &Host,
    profile: ProfileId,
    rest: &[String],
) -> Result<()> {
    match rest {
        [verb, version, note @ ..] if verb == "tag" => {
            let tag = TagProfile {
                profile,
                version: VersionName(version.clone()),
                at: None,
                note: note.join(" "),
            };
            as_chief(host, verb, tag).await
        }
        [verb, version] if verb == "revert" => {
            let revert = RevertProfile {
                profile,
                what: Revision::Version {
                    version: VersionName(version.clone()),
                },
                reason: None,
            };
            as_chief(host, verb, revert).await
        }
        [verb, path, text @ ..] if verb == "edit" => {
            let edit = EditProfile {
                profile,
                path: path.clone(),
                text: text.join(" "),
                message: None,
            };
            as_chief(host, verb, edit).await
        }
        [verb, path] if verb == "purge" => {
            let purge = PurgeProfile {
                profile,
                path: path.clone(),
            };
            as_chief(host, verb, purge).await
        }
        [verb, hire @ ..] if verb == "dream" => {
            let of = match hire {
                [] => Dreamer::Profile { profile },
                [hire] => Dreamer::Hire { hire: agent(hire)? },
                _ => bail!("usage: demo profile <n> dream [<hire>]"),
            };
            as_chief(host, verb, Dream { of }).await
        }
        [verb, hire, folder] if verb == "fill" => {
            fill(host, agent(hire)?, profile, Path::new(folder)).await
        }
        [verb, hire, path, text @ ..] if verb == "save" => {
            let hire = agent(hire)?;
            let at = address::Profile(profile);
            let asked = Files {
                since: None,
                after: None,
            };
            let FilesAnswer::Part { tip, .. } =
                forward(host, hire, &at, &asked).await?
            else {
                bail!("hire {} has no files to save on", hire.0);
            };
            let save = Save {
                call: CallId(format!("save-{}", micros()?)),
                base: tip,
                part: 0,
                last: true,
                changes: vec![FileChange::Write(Piece {
                    path: path.clone(),
                    offset: 0,
                    bytes: Base64(text.join(" ").into_bytes()),
                    last: true,
                    executable: false,
                })],
            };
            shown(verb, &forward(host, hire, &at, &save).await?)
        }
        _ => bail!("unknown profile command: {}", rest.join(" ")),
    }
}

async fn as_chief<T>(host: &Host, verb: &str, act: T) -> Result<()>
where
    T: ChiefRequest,
    T::Answer: Debug,
{
    shown(verb, &request(host, act).await?)
}

async fn fill(
    host: &Host,
    hire: AgentNumber,
    profile: ProfileId,
    folder: &Path,
) -> Result<()> {
    let at = address::Profile(profile);
    let (mut parts, mut files, mut largest) = (0, 0, 0);
    let mut after = None;
    loop {
        let asked = Files {
            since: None,
            after: after.take(),
        };
        let part = forward(host, hire, &at, &asked).await?;
        let mut bytes = Vec::new();
        ciborium::into_writer(&part, &mut bytes)?;
        largest = largest.max(bytes.len());
        parts += 1;
        let FilesAnswer::Part {
            tip,
            files: got,
            next,
            ..
        } = part
        else {
            bail!("files {part:?}");
        };
        for piece in got {
            let path = folder.join(&piece.path);
            std::fs::create_dir_all(
                path.parent().context("a file has a folder")?,
            )?;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(piece.offset > 0)
                .write(true)
                .truncate(piece.offset == 0)
                .open(&path)?;
            std::io::Write::write_all(&mut file, &piece.bytes.0)?;
            files += usize::from(piece.last);
        }
        match next {
            Some(cursor) => after = Some(cursor),
            None => {
                writeln!(
                    std::io::stdout(),
                    "fill parts={parts} files={files} \
                     largest={largest} tip={}",
                    tip.0
                )?;
                return Ok(());
            }
        }
    }
}

pub fn shown(verb: &str, answer: &impl Debug) -> Result<()> {
    writeln!(std::io::stdout(), "{verb} {answer:?}")?;
    Ok(())
}

pub fn agent(word: &str) -> Result<AgentNumber> {
    Ok(AgentNumber(agent_number(word)?))
}

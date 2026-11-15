//! The profile actor, `profile/<n>`, driven through stand-ins for the
//! realm and the agents: each calls as itself what the command asks,
//! and prints what the profile or the library tells it.
//!
//! ```text
//! demo profile make <n> <character ...>
//!                         create profile <n> with a first character
//! demo profile <n> branch <hire>      make the branch hires/<hire>
//! demo profile <n> tag <version> [<note ...>]
//! demo profile <n> versions
//! demo profile <n> pin <hire> [<version>]
//!                         pin a hire, or let it follow main
//! demo profile <n> revert <version>   undo the one commit <version>
//!                         names, with a commit on main
//! demo profile <n> edit <path> <text ...>
//! demo profile <n> purge <path>
//! demo profile <n> papers
//! demo profile <n> erase
//! demo profile <n> dream <d> [<hire>] check main, and the hire's
//!                         branch for a hire's dream, out for dream <d>
//! demo profile <n> end <d> done|stopped [<summary ...>]
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
//! The realm calls as `realm/local` and a hire's agent as
//! `agent/<hire>`, each led by its chief, chief 1. A command prints
//! the answer as `<action> <answer>`, and `fill` prints `fill
//! parts=<n> files=<n> largest=<bytes> tip=<commit>`, `largest` the
//! largest answer as the host carried it. A stand-in prints `told
//! <address> <action> <what>` for each call made to it, and answers a
//! removal `removed`.

use std::fmt::Debug;
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_host::{Address, Caller, Host, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::bytes::Base64;
use hamlet_messages::library::{DropCopy, ShelvesChanged};
use hamlet_messages::profile::{
    Branch, By, DreamId, DreamOf, Edit, EndDream, Erase, FileChange,
    Files, FilesAnswer, Moved, Papers, Piece, Pin, PrepareDream,
    ProfileInput, Purge, Removal, RemovePath, Revert, Revision, Save,
    Tag, VersionName, Versions,
};
use hamlet_messages::realm::ProfileId;
use hamlet_messages::workspace::{AgentNumber, CallId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The chief every command speaks for.
pub const CHIEF: By = By::Chief { chief: 1 };

const CALL_WITHIN: Duration = Duration::from_secs(60);

/// The stand-ins' factories, for the realm's kind and the agents'.
pub fn stand_ins() -> Vec<(String, ActorFactory)> {
    [address::Realm::KIND, address::Agent::KIND]
        .into_iter()
        .map(|kind| {
            let mut registry = Registry::new();
            registry.register_actor_with::<StandIn>(
                kind,
                ActorConfig::default(),
            );
            let factory = registry
                .into_factories()
                .remove(kind)
                .expect("the kind just registered");
            (kind.to_owned(), factory)
        })
        .collect()
}

pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    let [first, rest @ ..] = words else {
        bail!("usage: demo profile make <n> ... | <n> <command> ...");
    };
    if first == "make" {
        let [n, character @ ..] = rest else {
            bail!("usage: demo profile make <n> <character ...>");
        };
        let input = ProfileInput::Character {
            text: character.join(" "),
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&input, &mut bytes)?;
        host.create(&profile(n)?, bytes).await?;
        writeln!(std::io::stdout(), "make {}", profile(n)?)?;
        return Ok(());
    }
    if let Some((one, other)) = first.split_once(',') {
        let (one, other) = tokio::join!(
            timed(host, one, rest),
            timed(host, other, rest)
        );
        return one.and(other);
    }
    on(host, &profile(first)?, rest).await
}

/// Runs `words` on profile `n` and prints when it began and ended,
/// in microseconds since the epoch.
async fn timed(host: &Host, n: &str, words: &[String]) -> Result<()> {
    let began = micros()?;
    let done = on(host, &profile(n)?, words).await;
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
    at: &Address,
    rest: &[String],
) -> Result<()> {
    let at = at.clone();
    let realm = address::Realm.address()?;
    match rest {
        [command, hire] if command == "branch" => {
            let name = format!("hires/{}", number(hire)?);
            shown(host, &realm, &at, &Branch { name }).await
        }
        [command, version, note @ ..] if command == "tag" => {
            let tag = Tag {
                id: request_id()?,
                version: VersionName(version.clone()),
                at: None,
                note: note.join(" "),
                by: CHIEF,
            };
            shown(host, &realm, &at, &tag).await
        }
        [command] if command == "versions" => {
            shown(host, &realm, &at, &Versions).await
        }
        [command, hire, to @ ..] if command == "pin" => {
            let pin = Pin {
                id: request_id()?,
                hire: AgentNumber(number(hire)?),
                to: to.first().map(|v| VersionName(v.clone())),
                by: CHIEF,
            };
            shown(host, &realm, &at, &pin).await
        }
        [command, version] if command == "revert" => {
            let revert = Revert {
                id: request_id()?,
                what: Revision::Version {
                    version: VersionName(version.clone()),
                },
                reason: None,
                by: CHIEF,
            };
            shown(host, &realm, &at, &revert).await
        }
        [command, path, text @ ..] if command == "edit" => {
            let edit = Edit {
                id: request_id()?,
                path: path.clone(),
                text: text.join(" "),
                by: CHIEF,
            };
            shown(host, &realm, &at, &edit).await
        }
        [command, path] if command == "purge" => {
            let purge = Purge {
                id: request_id()?,
                path: path.clone(),
                by: CHIEF,
            };
            shown(host, &realm, &at, &purge).await
        }
        [command] if command == "papers" => {
            shown(host, &realm, &at, &Papers).await
        }
        [command] if command == "erase" => {
            let erase = Erase { id: request_id()? };
            shown(host, &realm, &at, &erase).await
        }
        [command, dream, hire @ ..] if command == "dream" => {
            let of = match hire {
                [] => DreamOf::Profile,
                [hire] => DreamOf::Hire {
                    agent: AgentNumber(number(hire)?),
                },
                _ => bail!(
                    "usage: demo profile <n> dream <d> [<hire>]"
                ),
            };
            let prepare = PrepareDream {
                dream: DreamId(number(dream)?),
                of,
            };
            shown(host, &realm, &at, &prepare).await
        }
        [command, dream, end, summary @ ..] if command == "end" => {
            let end = EndDream {
                dream: DreamId(number(dream)?),
                done: match end.as_str() {
                    "done" => true,
                    "stopped" => false,
                    _ => bail!("a dream ends done or stopped"),
                },
                summary: summary.join(" "),
            };
            shown(host, &realm, &at, &end).await
        }
        [command, hire, folder] if command == "fill" => {
            fill(host, &agent_at(hire)?, &at, Path::new(folder)).await
        }
        [command, hire, path, text @ ..] if command == "save" => {
            let agent = agent_at(hire)?;
            let asked = Files {
                since: None,
                after: None,
            };
            let bytes = carried(host, &agent, &at, &asked).await?;
            let FilesAnswer::Part { tip, .. } =
                ciborium::from_reader(bytes.as_slice())?
            else {
                bail!("hire {hire} has no files to save on");
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
            shown(host, &agent, &at, &save).await
        }
        _ => bail!("unknown profile command: {}", rest.join(" ")),
    }
}

/// Asks for every file of the hire's branch as its agent does, part
/// by part from one commit, and writes them under `folder`.
async fn fill(
    host: &Host,
    agent: &Address,
    at: &Address,
    folder: &Path,
) -> Result<()> {
    let (mut parts, mut files, mut largest) = (0, 0, 0);
    let mut after = None;
    loop {
        let asked = Files {
            since: None,
            after: after.take(),
        };
        let bytes = carried(host, agent, at, &asked).await?;
        largest = largest.max(bytes.len());
        parts += 1;
        let part: FilesAnswer =
            ciborium::from_reader(bytes.as_slice())?;
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

/// Calls `to` as `from` and prints the answer.
pub async fn shown<A>(
    host: &Host,
    from: &Address,
    to: &Address,
    asked: &A,
) -> Result<()>
where
    A: Action,
    A::Output: DeserializeOwned + Debug,
{
    let bytes = carried(host, from, to, asked).await?;
    let answer: A::Output = ciborium::from_reader(bytes.as_slice())?;
    writeln!(std::io::stdout(), "{} {answer:?}", A::NAME)?;
    Ok(())
}

/// The answer of `to` to `asked`, called as `from`, as the host
/// carried it.
async fn carried<A: Action>(
    host: &Host,
    from: &Address,
    to: &Address,
    asked: &A,
) -> Result<Vec<u8>> {
    host.get_or_start(from).await?;
    let forward = Forward {
        to: to.to_string(),
        name: A::NAME.to_owned(),
        args: hamlet_actor::action::encode_positional(asked)?,
    };
    let reply = host
        .call(
            from,
            Forward::NAME,
            hamlet_actor::action::encode_positional(&forward)?,
            CALL_WITHIN,
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}

fn profile(word: &str) -> Result<Address> {
    Ok(address::Profile(ProfileId(number(word)?)).address()?)
}

/// The address of agent `word`, a hire's number.
pub fn agent_at(word: &str) -> Result<Address> {
    Ok(address::Agent(AgentNumber(number(word)?)).address()?)
}

fn number(word: &str) -> Result<u32> {
    word.parse()
        .with_context(|| format!("not a number: {word}"))
}

/// A fresh request id, as a chief's client makes one.
pub fn request_id<T: DeserializeOwned>() -> Result<T> {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let now =
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    let text = format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (now >> 32) as u32,
        (now >> 16) as u16,
        (now & 0xfff) as u16,
        made & 0xfff,
        u64::from(std::process::id()) << 16 | (made >> 12) & 0xffff,
    );
    Ok(ciborium::Value::Text(text).deserialized()?)
}

/// Stands in for the realm or an agent: calls another actor as
/// itself, and prints what it is told.
struct StandIn;

#[derive(Serialize, Deserialize)]
struct Forward {
    to: String,
    name: String,
    args: Vec<u8>,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

#[async_trait::async_trait]
impl Actor for StandIn {
    type Input = ();
    type Actions =
        (Forward, Moved, RemovePath, ShelvesChanged, DropCopy);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> Result<Self> {
        Ok(Self)
    }
}

impl Handler<Forward> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        forward: Forward,
    ) -> Result<Vec<u8>> {
        let (kind, key) = forward
            .to
            .split_once('/')
            .context("an address is <kind>/<key>")?;
        let to = Address::new(kind, key)?;
        HostRef::of(ctx.inner())?
            .call(&to, &forward.name, forward.args, CALL_WITHIN)
            .await
    }
}

fn told(
    ctx: &Ctx<StandIn>,
    action: &str,
    what: &impl Debug,
) -> Result<()> {
    writeln!(
        std::io::stdout(),
        "told {} {action} {what:?}",
        ctx.actor_id()
    )?;
    Ok(())
}

impl Handler<Moved> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        moved: Moved,
    ) -> Result<()> {
        told(&ctx, Moved::NAME, &moved.tip.0)
    }
}

impl Handler<RemovePath> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        remove: RemovePath,
    ) -> Result<Removal> {
        told(&ctx, RemovePath::NAME, &remove.path)?;
        Ok(Removal::Removed)
    }
}

impl Handler<ShelvesChanged> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        changed: ShelvesChanged,
    ) -> Result<()> {
        let names = changed
            .list
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>();
        told(&ctx, ShelvesChanged::NAME, &names)
    }
}

impl Handler<DropCopy> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        drop: DropCopy,
    ) -> Result<()> {
        told(&ctx, DropCopy::NAME, &drop.skill)
    }
}

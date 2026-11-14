//! A stand-in Node host for the link's tests. It takes the link's
//! ends as the Node host does, and serves the link with the one kind
//! `agent`, or plays a part of the Node host's half by hand.
//!
//! ```text
//! link_child [--env <file>] [--starts <file>] [--stray] [--frames <file>]
//!            [--protocol <version> | --hold | --welcomed
//!             | --ask-early <file> | --forge <stamp> | --silent]
//!
//! --env <file>          write the names of its environment's
//!                       variables to <file>, one to a line
//! --starts <file>       append `<pid> <ms>` to <file> once it has
//!                       taken the link's ends, <ms> in Unix
//!                       milliseconds
//! --stray               write a line, once the link is taken, to a
//!                       copy of the standard output it started with
//! --frames <file>       by hand or at --protocol, append the name
//!                       of each frame it reads to <file>, one to a
//!                       line
//! --protocol <version>  without serving the link, say hello on its
//!                       ends stating <version>, and end at the Rust
//!                       host's answer
//! --hold                by hand, take the link and say nothing
//! --welcomed            by hand, say hello, take the welcome, then
//!                       say nothing
//! --ask-early <file>    by hand, say hello, take the welcome, ask
//!                       `realm/r`'s `who` as `agent/a`, and write the
//!                       reply to <file> as JSON
//! --forge <stamp>       by hand, get to Ready, then ask `realm/r`'s
//!                       `who` as <stamp>: `none`, `client` or
//!                       `<kind>/<key>`
//! --silent              by hand, get to Ready, then answer nothing
//! ```

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, PipeReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use hamlet_actor::action::encode_positional;
use hamlet_actor::{Actor, ActorConfig, Ctx, Handler, Registry};
use hamlet_actor_core::ActorFactory;
use hamlet_host::link::{self, Ask, Frame, PROTOCOL, WireError};
use hamlet_host::{Address, Caller, HostRef, child};
use hamlet_os::process::{self, Ends};
use hamlet_protocol::Version;

#[path = "../tests/support/actions.rs"]
mod actions;

use actions::{Exit, Fail, Input, Relay, Seen, Slow, Started, Who};

/// The one kind the stand-in holds.
const KIND: &str = "agent";

const DEADLINE: Duration = Duration::from_secs(10);

/// What the stand-in does on the link.
enum Mode {
    Serve,
    Hello(Version),
    ByHand(Part),
}

enum Part {
    Hold,
    Welcomed,
    AskEarly(PathBuf),
    Forge(Option<Caller>),
    Silent,
}

fn main() -> Result<()> {
    let mut env = None;
    let mut starts = None;
    let mut frames = None;
    let mut stray = false;
    let mut mode = Mode::Serve;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .with_context(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--env" => env = Some(PathBuf::from(value()?)),
            "--starts" => starts = Some(PathBuf::from(value()?)),
            "--frames" => frames = Some(PathBuf::from(value()?)),
            "--stray" => stray = true,
            "--protocol" => mode = Mode::Hello(value()?.parse()?),
            "--hold" => mode = Mode::ByHand(Part::Hold),
            "--welcomed" => mode = Mode::ByHand(Part::Welcomed),
            "--ask-early" => {
                mode = Mode::ByHand(Part::AskEarly(value()?.into()));
            }
            "--forge" => {
                mode = Mode::ByHand(Part::Forge(stamp(&value()?)?));
            }
            "--silent" => mode = Mode::ByHand(Part::Silent),
            _ => bail!("unknown argument {arg}"),
        }
    }
    if let Some(path) = env {
        let names = std::env::vars_os()
            .map(|(name, _)| format!("{}\n", name.to_string_lossy()))
            .collect::<String>();
        std::fs::write(path, names)?;
    }
    match mode {
        Mode::Serve => {
            let copy = stray.then(stdout_copy).transpose()?;
            child::take_link()?;
            if let Some(mut copy) = copy {
                copy.write_all(b"stray\n")?;
            }
            record_start(starts.as_deref())?;
            tokio::runtime::Runtime::new()?
                .block_on(link::serve(factories()))
        }
        Mode::Hello(protocol) => {
            let Ends { input, mut output } = Ends::take()?;
            record_start(starts.as_deref())?;
            say_hello(&mut output, protocol)?;
            Reader::new(input, frames.as_deref())?.next()?;
            Ok(())
        }
        Mode::ByHand(part) => {
            let Ends { input, output } = Ends::take()?;
            record_start(starts.as_deref())?;
            by_hand(
                part,
                Reader::new(input, frames.as_deref())?,
                output,
            )
        }
    }
}

fn stamp(text: &str) -> Result<Option<Caller>> {
    Ok(match text {
        "none" => None,
        "client" => Some(Caller::Client),
        actor => {
            let (kind, key) =
                actor.split_once('/').with_context(|| {
                    format!(
                        "{actor} is not none, client or <kind>/<key>"
                    )
                })?;
            Some(Caller::Actor(Address::new(kind, key)?))
        }
    })
}

fn stdout_copy() -> io::Result<File> {
    #[cfg(unix)]
    let copy = std::os::fd::AsFd::as_fd(&io::stdout())
        .try_clone_to_owned()?;
    #[cfg(windows)]
    let copy =
        std::os::windows::io::AsHandle::as_handle(&io::stdout())
            .try_clone_to_owned()?;
    Ok(File::from(copy))
}

/// Appends this process's id and the time to `path`, in one write.
fn record_start(path: Option<&Path>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(
            format!("{} {ms}\n", std::process::id()).as_bytes(),
        )?;
    Ok(())
}

fn say_hello(
    output: &mut PipeWriter,
    protocol: Version,
) -> Result<()> {
    link::write(
        output,
        &Frame::Hello {
            protocol,
            fingerprint: link::fingerprint().to_owned(),
            kinds: vec![KIND.to_owned()],
            pid: std::process::id(),
        },
    )?;
    Ok(())
}

struct Reader {
    input: PipeReader,
    record: Option<File>,
}

impl Reader {
    fn new(input: PipeReader, record: Option<&Path>) -> Result<Self> {
        let record = record
            .map(|path| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
            })
            .transpose()?;
        Ok(Self { input, record })
    }

    fn next(&mut self) -> Result<Option<Frame>> {
        let Ok(Some(frame)) = link::read(&mut self.input) else {
            return Ok(None);
        };
        if let Some(record) = &mut self.record {
            writeln!(record, "{}", frame.name())?;
        }
        Ok(Some(frame))
    }

    fn hold(&mut self) -> Result<()> {
        loop {
            match self.next()? {
                Some(Frame::Goodbye) => return Ok(()),
                Some(_) => {}
                None => process::end_now(),
            }
        }
    }
}

fn by_hand(
    part: Part,
    mut reader: Reader,
    mut output: PipeWriter,
) -> Result<()> {
    if !matches!(part, Part::Hold) {
        say_hello(&mut output, PROTOCOL)?;
        match reader.next()? {
            Some(Frame::Welcome { .. }) => {}
            frame => bail!("the Rust host answered {frame:?}"),
        }
    }
    match part {
        Part::Hold | Part::Welcomed => {}
        Part::AskEarly(answer) => {
            ask_early(&mut reader, &mut output, &answer)?;
        }
        Part::Forge(stamp) => {
            link::write(&mut output, &Frame::Ready)?;
            link::write(&mut output, &ask_who(stamp)?)?;
        }
        Part::Silent => link::write(&mut output, &Frame::Ready)?,
    }
    reader.hold()
}

fn ask_early(
    reader: &mut Reader,
    output: &mut PipeWriter,
    answer: &Path,
) -> Result<()> {
    let agent = Caller::Actor(Address::new(KIND, "a")?);
    link::write(output, &ask_who(Some(agent))?)?;
    let reply = loop {
        match reader.next()? {
            Some(Frame::Reply { id: 0, result }) => break result,
            Some(_) => {}
            None => process::end_now(),
        }
    };
    let reply = match reply {
        Ok(output) => Ok(ciborium::from_reader::<
            serde_json::Value,
            _,
        >(output.as_slice())?),
        Err(error) => Err::<_, WireError>(error),
    };
    std::fs::write(answer, serde_json::to_string(&reply)?)?;
    Ok(())
}

fn ask_who(from: Option<Caller>) -> Result<Frame> {
    Ok(Frame::Request {
        id: 0,
        from,
        to: Address::new("realm", "r")?,
        ask: Ask::Call {
            name: "who".to_owned(),
            args: Vec::new(),
            deadline_ms: u64::try_from(DEADLINE.as_millis())?,
        },
    })
}

fn factories() -> HashMap<String, ActorFactory> {
    let mut registry = Registry::new();
    registry.register_actor_with::<Agent>(
        KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry.into_factories()
}

struct Agent {
    input: Option<Address>,
    started: Option<Seen>,
}

#[async_trait::async_trait]
impl Actor for Agent {
    type Input = Option<Address>;
    type Actions = (Who, Input, Started, Relay, Fail, Slow, Exit);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let input = ctx.input()?;
        let started = match &input {
            Some(note) => Some(Seen::of_call(
                HostRef::of(ctx.inner())?
                    .call(
                        note,
                        "note",
                        encode_positional(&())?,
                        DEADLINE,
                    )
                    .await,
            )),
            None => None,
        };
        Ok(Self { input, started })
    }
}

impl Handler<Who> for Agent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Who,
    ) -> Result<Option<Caller>> {
        ctx.conn().context("the call has no connection")?.params()
    }
}

impl Handler<Input> for Agent {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Input,
    ) -> Result<Option<Address>> {
        Ok(self.input.clone())
    }
}

impl Handler<Started> for Agent {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Started,
    ) -> Result<Option<Seen>> {
        Ok(self.started.clone())
    }
}

impl Handler<Relay> for Agent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        relay: Relay,
    ) -> Result<Seen> {
        relay.run(ctx.inner()).await
    }
}

impl Handler<Fail> for Agent {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        fail: Fail,
    ) -> Result<()> {
        Err(fail.into_error())
    }
}

impl Handler<Slow> for Agent {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        slow: Slow,
    ) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(slow.ms)).await;
        Ok(())
    }
}

impl Handler<Exit> for Agent {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Exit,
    ) -> Result<()> {
        process::end_now()
    }
}

//! A stand-in Node host for the link's tests. It takes the link's
//! ends as the Node host does, and serves the link with the one kind
//! `agent`, whose actors it never starts.
//!
//! ```text
//! link_child [--env <file>] [--starts <file>] [--stray] [--protocol <version> | --hold]
//!
//! --env <file>          write the names of its environment's
//!                       variables to <file>, one to a line
//! --starts <file>       append `<pid> <ms>` to <file> once it has
//!                       taken the link's ends, <ms> in Unix
//!                       milliseconds
//! --stray               write a line, once the link is taken, to a
//!                       copy of the standard output it started with
//! --protocol <version>  without serving the link, say hello on its
//!                       ends stating <version>, and end at the Rust
//!                       host's answer
//! --hold                take the link, then say nothing and wait to
//!                       be killed
//! ```

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, PipeReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_host::child;
use hamlet_host::link::{self, Frame};
use hamlet_os::process::Ends;
use hamlet_protocol::Version;

/// The one kind the stand-in holds.
const KIND: &str = "agent";

/// What the stand-in does on the link.
enum Mode {
    Serve,
    Hello(Version),
    Hold,
}

fn main() -> Result<()> {
    let mut env = None;
    let mut starts = None;
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
            "--stray" => stray = true,
            "--protocol" => mode = Mode::Hello(value()?.parse()?),
            "--hold" => mode = Mode::Hold,
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
        Mode::Hold => {
            child::take_link()?;
            record_start(starts.as_deref())?;
            loop {
                std::thread::park();
            }
        }
        Mode::Hello(protocol) => {
            let Ends { input, output } = Ends::take()?;
            record_start(starts.as_deref())?;
            hello(protocol, input, output)
        }
    }
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

fn factories() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some(KIND.to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    let factory = ActorFactory::new(config, |_| {
        Box::pin(async { anyhow::Ok(()) })
    });
    HashMap::from([(KIND.to_owned(), factory)])
}

fn hello(
    protocol: Version,
    mut input: PipeReader,
    mut output: PipeWriter,
) -> Result<()> {
    link::write(
        &mut output,
        &Frame::Hello {
            protocol,
            fingerprint: link::fingerprint().to_owned(),
            kinds: vec![KIND.to_owned()],
            pid: std::process::id(),
        },
    )?;
    link::read(&mut input)?;
    Ok(())
}

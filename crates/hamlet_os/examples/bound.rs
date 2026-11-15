//! Runs a command as this process's bound child, with this process's
//! environment and folder. The child's output end is copied to this
//! process's standard output, and its standard output and error to
//! this process's standard error. Its input end stays open, and
//! nothing is written to it, until the child exits. Exits with the
//! child's code, or 1 when it has none.
//!
//! ```text
//! bound <program> [<argument>...]
//! ```

use std::io;
use std::process::ExitCode;

use hamlet_os::process::{Command, Pipes, Tree};

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<ExitCode> {
    let argv = std::env::args().skip(1).collect::<Vec<_>>();
    let env = std::env::vars().collect();
    let cwd = std::env::current_dir()?;
    let (
        mut tree,
        Pipes {
            input,
            mut output,
            mut errors,
        },
    ) = Tree::spawn_bound(Command {
        argv: &argv,
        env: &env,
        cwd: &cwd.to_string_lossy(),
    })?;
    let copies = [
        std::thread::spawn(move || {
            io::copy(&mut output, &mut io::stdout())
        }),
        std::thread::spawn(move || {
            io::copy(&mut errors, &mut io::stderr())
        }),
    ];
    let status = tree.ended().await?;
    tree.kill();
    drop(input);
    for copy in copies {
        copy.join()
            .map_err(|_| io::Error::other("a copy panicked"))??;
    }
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

//! The stand-in Node host, the example `link_child`, as a host's
//! child.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_host::child::{ChildOptions, LinkState};
use hamlet_os::process::{Command, Pipes, Tree};
use tokio::sync::watch;

use crate::logs::Logs;

/// The stand-in, in the `examples` folder of the profile's folder,
/// which holds the test at some depth: the depth differs between
/// cargo's build layouts.
pub fn stand_in() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .map(|folder| {
            folder.join("examples").join(format!(
                "link_child{}",
                std::env::consts::EXE_SUFFIX
            ))
        })
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            panic!(
                "no stand-in Node host above {}: build it with \
                 `cargo build -p hamlet_host --examples`",
                exe.display()
            )
        })
}

/// The stand-in run with `args`, holding `agent`, as the child.
pub fn child(args: &[&str]) -> ChildOptions {
    let mut argv = vec![stand_in().to_str().unwrap().to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    ChildOptions {
        argv,
        kinds: vec!["agent".to_owned()],
    }
}

/// The stand-in, started by the test itself on `home`, with this
/// process as its Rust host.
pub fn own_stand_in(home: &Path, args: &[&str]) -> (Tree, Pipes) {
    let mut argv = vec![stand_in().to_str().unwrap().to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let home = home.to_str().unwrap();
    let env =
        BTreeMap::from([("HAMLET_HOME".to_owned(), home.to_owned())]);
    Tree::spawn_bound(Command {
        argv: &argv,
        env: &env,
        cwd: home,
    })
    .unwrap()
}

/// Waits for the link's state `to`, failing past 30 seconds.
pub async fn wait_for(
    state: &mut watch::Receiver<LinkState>,
    to: LinkState,
) {
    let reached = tokio::time::timeout(
        Duration::from_secs(30),
        state.wait_for(|now| *now == to),
    )
    .await;
    assert!(
        matches!(reached, Ok(Ok(_))),
        "the link never got {to:?}"
    );
}

/// Whether a line of `logs` holds each of `words`.
pub fn has_line(logs: &Logs, words: &[&str]) -> bool {
    logs.text()
        .lines()
        .any(|line| words.iter().all(|word| line.contains(word)))
}

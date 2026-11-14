use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_host::child::{ChildOptions, LinkState};
use hamlet_os::process::{Command, Pipes, Tree};
use tokio::sync::watch;

use crate::logs::Logs;

/// How long a child may outlive its link.
pub const GONE_WAIT: Duration = Duration::from_secs(2);

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

pub fn child(args: &[&str]) -> ChildOptions {
    let mut argv = vec![stand_in().to_str().unwrap().to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    ChildOptions {
        argv,
        kinds: vec!["agent".to_owned()],
    }
}

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

pub fn has_line(logs: &Logs, words: &[&str]) -> bool {
    logs.text()
        .lines()
        .any(|line| words.iter().all(|word| line.contains(word)))
}

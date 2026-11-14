#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::fs;
use std::time::Duration;

use hamlet_sandbox::{
    AllLeftovers, Env, Launch, Policy, Sandbox, host_env,
};
use hamlet_testing::Scratch;

fn groups_holding(word: &str) -> Option<BTreeSet<String>> {
    let mut groups = BTreeSet::new();
    let mut slept = false;
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let path = entry.path();
        let Ok(argv) = fs::read(path.join("cmdline")) else {
            continue;
        };
        let argv = String::from_utf8_lossy(&argv).replace('\0', " ");
        if !argv.contains(word) {
            continue;
        }
        let Ok(stat) = fs::read_to_string(path.join("stat")) else {
            continue;
        };
        // After the name: state, parent, group.
        let after = &stat[stat.rfind(')')? + 1..];
        groups.insert(after.split_whitespace().nth(2)?.to_owned());
        slept |= argv.starts_with("sleep ");
    }
    slept.then_some(groups)
}

#[tokio::test]
async fn a_command_runs_in_one_process_group() {
    let scratch = Scratch::new("sandbox-group");
    let dir = scratch.join("work");
    fs::create_dir_all(&dir).unwrap();
    let policy = Policy {
        rw: vec![dir.clone()],
        env: host_env(),
        ..Policy::default()
    };
    let env =
        Env::new(&dir.to_string_lossy(), policy, Sandbox::testing())
            .unwrap();
    let word = format!("9000.{}", std::process::id());
    let argv =
        ["bash".to_owned(), "-c".to_owned(), format!("sleep {word}")];
    let leftovers = AllLeftovers::default().of(1);
    let mut ignored = |_: &str| {};
    let ran = env.exec(
        Launch::Shell(&argv),
        Duration::from_secs(30),
        &[],
        &leftovers,
        &mut ignored,
    );
    let found = async {
        loop {
            if let Some(groups) = groups_holding(&word) {
                return groups;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let groups = tokio::select! {
        ran = ran => panic!("the command ended first: {ran:?}"),
        groups = tokio::time::timeout(Duration::from_secs(10), found) => {
            groups.expect("the command's sleep within 10 s")
        }
    };
    assert_eq!(groups.len(), 1, "groups {groups:?}");
}

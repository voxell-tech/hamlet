use std::io;
use std::process::Stdio;

use rustix::process::{
    Pid, Signal, kill_process, kill_process_group,
};

use crate::process::{Command, Limits};

#[derive(Default)]
pub(crate) struct Group(Option<Pid>);

impl Group {
    pub(crate) fn kill(&self) {
        if let Some(pid) = self.0
            && kill_process_group(pid, Signal::KILL).is_err()
        {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

pub(crate) fn spawn(
    command: Command<'_>,
    _limits: &Limits,
) -> io::Result<(Group, tokio::process::Child)> {
    let mut spawn = tokio::process::Command::new(&command.argv[0]);
    spawn
        .args(&command.argv[1..])
        .env_clear()
        .envs(command.env)
        .current_dir(command.cwd)
        .stdin(command.stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let child = spawn.spawn()?;
    let leader = child.id().and_then(|id| Pid::from_raw(id as i32));
    Ok((Group(leader), child))
}

pub(crate) fn limited(
    limits: &Limits,
    argv: &[String],
) -> Vec<String> {
    // bash counts file sizes in KiB. A lower machine limit stays in
    // force.
    let script = [
        ("t", limits.cpu_secs),
        ("u", limits.max_procs),
        ("f", limits.max_file_mb * 1024),
        ("c", 0),
    ]
    .iter()
    .map(|(flag, value)| {
        format!(
            "ulimit -{flag} {value} 2>/dev/null || ulimit -{flag} \
             hard"
        )
    })
    .collect::<Vec<_>>()
    .join("\n");
    let mut out = vec![
        "bash".to_owned(),
        "-c".into(),
        format!("{script}\nexec \"$@\""),
        "bash".into(),
    ];
    out.extend(argv.iter().cloned());
    out
}

pub(crate) fn unenforced(
    _limits: &Limits,
) -> &'static [&'static str] {
    &[]
}

pub(crate) fn is_executable(path: &str) -> bool {
    rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

//! Running a command in a sandbox, under its limits.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::{Policy, Sandbox};

/// The wait for the output of a command that was killed.
const KILL_GRACE: Duration = Duration::from_secs(2);

fn exit_code(s: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        s.code()
            .or_else(|| s.signal().map(|n| 128 + n))
            .unwrap_or(-1)
    }
    #[cfg(not(unix))]
    {
        s.code().unwrap_or(-1)
    }
}
/// A command's output and its ending.
pub struct RunOutput {
    pub exit_code: i32,
    pub output: String,
    pub timed_out: bool,
}

/// Reads to EOF, keeping at most `cap` bytes and discarding the rest
/// so the writer never blocks. Returns the kept bytes and whether any
/// were dropped.
async fn read_capped<R: AsyncRead + Unpin>(
    mut r: R,
    cap: usize,
) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut over = false;
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(buf.len());
                if n > room {
                    over = true;
                }
                buf.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
    (buf, over)
}

#[cfg(unix)]
/// Lowers a limit, clamped to the current hard limit (raising it is
/// not permitted).
fn set_rlimit(
    resource: libc::c_int,
    value: u64,
) -> std::io::Result<()> {
    let mut cur = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit and setrlimit are async-signal-safe and the
    // structs are valid.
    unsafe {
        if libc::getrlimit(resource as _, &mut cur) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let v = (value as libc::rlim_t).min(cur.rlim_max);
        let lim = libc::rlimit {
            rlim_cur: v,
            rlim_max: v,
        };
        if libc::setrlimit(resource as _, &lim) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
/// Runs `argv` in `sandbox` under `policy`, with `stdin` as its
/// input.
pub async fn run(
    sandbox: &dyn Sandbox,
    policy: &Policy,
    argv: &[String],
    cwd: &Path,
    stdin: Option<&[u8]>,
) -> Result<RunOutput> {
    if argv.is_empty() {
        bail!("empty argv");
    }
    let lim = policy.limits.clone();
    let mut cmd = sandbox.command(policy, argv, cwd);
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        let cpu = lim.cpu_secs;
        let procs = lim.max_procs;
        let fsize = lim.max_file_mb.saturating_mul(1024 * 1024);
        // SAFETY: the closure only calls getrlimit/setrlimit, which
        // are async-signal-safe, and does not allocate
        // (errors are built from errno only).
        unsafe {
            cmd.pre_exec(move || {
                set_rlimit(libc::RLIMIT_CPU as _, cpu)?;
                set_rlimit(libc::RLIMIT_NPROC as _, procs)?;
                set_rlimit(libc::RLIMIT_FSIZE as _, fsize)?;
                set_rlimit(libc::RLIMIT_CORE as _, 0)?;
                Ok(())
            });
        }
    }

    let mut wrap = CommandWrap::from(cmd);
    #[cfg(unix)]
    wrap.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    wrap.wrap(JobObject);
    wrap.wrap(KillOnDrop);
    let mut child = wrap
        .spawn()
        .with_context(|| format!("spawning {}", sandbox.name()))?;

    if let Some(bytes) = stdin
        && let Some(mut pipe) = child.stdin().take()
    {
        let data = bytes.to_vec();
        tokio::spawn(async move {
            let _ = pipe.write_all(&data).await;
        });
    }

    let cap = lim.max_output_bytes.saturating_add(1);
    let out = child.stdout().take().context("no stdout")?;
    let err = child.stderr().take().context("no stderr")?;
    let mut out_task = tokio::spawn(read_capped(out, cap));
    let mut err_task = tokio::spawn(read_capped(err, cap));

    let wait = async {
        let status = child.wait().await;
        let o = (&mut out_task).await;
        let e = (&mut err_task).await;
        (status, o, e)
    };
    let (status, out_res, err_res, timed_out) =
        match tokio::time::timeout(
            Duration::from_secs(lim.wall_secs),
            wait,
        )
        .await
        {
            Ok((status, o, e)) => (Some(status?), o, e, false),
            Err(_) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                // Escaped processes may still hold the pipes, so
                // don't wait forever.
                let o =
                    tokio::time::timeout(KILL_GRACE, &mut out_task)
                        .await;
                let e =
                    tokio::time::timeout(KILL_GRACE, &mut err_task)
                        .await;
                let flat = |r: Result<Result<_, _>, _>| {
                    r.unwrap_or_else(|_| Ok((Vec::new(), false)))
                };
                (None, flat(o), flat(e), true)
            }
        };
    let (out_bytes, out_over) = out_res.unwrap_or_default();
    let (err_bytes, err_over) = err_res.unwrap_or_default();

    let exit_code = match status {
        None => -1,
        Some(s) => exit_code(s),
    };

    let mut text = String::from_utf8_lossy(&out_bytes).into_owned();
    if !err_bytes.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&String::from_utf8_lossy(&err_bytes));
    }
    let max = lim.max_output_bytes;
    if out_over || err_over || text.len() > max {
        let mut end = max.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[output truncated]");
    }
    Ok(RunOutput {
        exit_code,
        output: text,
        timed_out,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::Unsandboxed;
    use crate::tests::{policy, s};

    async fn sh(
        p: &Policy,
        script: &str,
        stdin: Option<&[u8]>,
    ) -> RunOutput {
        run(
            &Unsandboxed,
            p,
            &s(&["sh", "-c", script]),
            Path::new("/"),
            stdin,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn timeout_kills_process_group() {
        let mut p = policy();
        p.limits.wall_secs = 1;
        // NPROC is per user, so the default can already be exceeded
        // on a busy dev machine.
        p.limits.max_procs = 100_000;
        let t = Instant::now();
        let r = sh(&p, "sleep 30", None).await;
        assert!(r.timed_out);
        assert_eq!(r.exit_code, -1);
        let r = sh(&p, "sh -c 'sleep 30 & wait'", None).await;
        assert!(r.timed_out);
        assert!(
            t.elapsed() < Duration::from_secs(6),
            "{:?}",
            t.elapsed()
        );
    }

    #[tokio::test]
    async fn output_is_cut_at_the_cap_on_a_char_boundary() {
        let mut p = policy();
        p.limits.max_output_bytes = 10;
        let r = sh(&p, "echo 0123456789abcdef", None).await;
        assert_eq!(r.output, "0123456789\n[output truncated]");
        p.limits.max_output_bytes = 4;
        let r =
            sh(&p, "printf '\\303\\251\\303\\251\\303\\251'", None)
                .await;
        assert_eq!(r.output, "\u{e9}\u{e9}\n[output truncated]");
    }

    #[tokio::test]
    async fn env_cleared() {
        // cargo sets CARGO_MANIFEST_DIR for every test.
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        let mut p = policy();
        p.env.push(("KEPT".into(), "yes".into()));
        let r = sh(
            &p,
            "echo \"${CARGO_MANIFEST_DIR-absent} $KEPT\"",
            None,
        )
        .await;
        assert_eq!(r.output, "absent yes\n");
    }
}

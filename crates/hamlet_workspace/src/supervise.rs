//! Trusted PID 1 drains every descendant before reporting an
//! operation's result.

#![expect(
    unsafe_code,
    reason = "namespaces, the loopback, limits and reaping are set \
              through libc"
)]

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::process::CommandExt;
use std::process::{
    Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio,
};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::wire::{
    Code, MAX_BODY, MAX_OUTPUT, Operation, Ran, Reply,
};

/// Unix socket of the worker's proxy inside a network-granted
/// operation.
const NETWORK_SOCKET: &str = "/network.sock";
/// Loopback port of the relay to [`NETWORK_SOCKET`].
const PROXY_PORT: u16 = 3128;

/// Enters a new user and network namespace, raises its loopback with
/// `--loopback`, and runs the sandbox executable there.
pub fn netless(args: &[String]) -> Result<()> {
    let (loopback, args) = match args.split_first() {
        Some((flag, rest)) if flag == "--loopback" => (true, rest),
        _ => (false, args),
    };
    let executable =
        args.first().context("Missing sandbox executable")?;
    // SAFETY: this single-threaded launcher has not opened any
    // operation processes.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    // SAFETY: unshare changes only this launcher's namespaces.
    if unsafe {
        libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET)
    } != 0
    {
        return Err(io::Error::last_os_error().into());
    }
    fs::write("/proc/self/setgroups", "deny")?;
    fs::write("/proc/self/uid_map", format!("0 {uid} 1"))?;
    fs::write("/proc/self/gid_map", format!("0 {gid} 1"))?;
    if loopback {
        raise_loopback()?;
    }
    Err(Command::new(executable).args(&args[1..]).exec().into())
}

/// Sets `lo` up unless it already is, as in gVisor, whose new network
/// namespaces start with loopback up and addressed.
fn raise_loopback() -> io::Result<()> {
    // SAFETY: socket takes no pointer arguments.
    let socket = unsafe {
        libc::socket(
            libc::AF_INET,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if socket < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor was just opened and has no other owner.
    let socket = unsafe { OwnedFd::from_raw_fd(socket) };
    // SAFETY: ifreq is plain data for which all zeroes is valid.
    let mut request = unsafe { std::mem::zeroed::<libc::ifreq>() };
    request.ifr_name[..2].copy_from_slice(&[b'l' as _, b'o' as _]);
    // SAFETY: request is a live, NUL-terminated ifreq naming lo.
    if unsafe {
        libc::ioctl(
            socket.as_raw_fd(),
            libc::SIOCGIFFLAGS,
            &mut request,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: SIOCGIFFLAGS filled the flags member of the union.
    let flags = unsafe { request.ifr_ifru.ifru_flags };
    if flags & libc::IFF_UP as libc::c_short != 0 {
        return Ok(());
    }
    request.ifr_ifru.ifru_flags =
        flags | libc::IFF_UP as libc::c_short;
    // SAFETY: request is the same live ifreq with only its flags
    // changed.
    if unsafe {
        libc::ioctl(
            socket.as_raw_fd(),
            libc::SIOCSIFFLAGS,
            &mut request,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Runs one operation as PID 1. `network` starts the proxy relay for
/// a command.
pub fn run(args: &[String]) -> Result<()> {
    let network = match args {
        [] => false,
        [flag] if flag == "network" => true,
        _ => bail!("usage: init [network]"),
    };
    // SAFETY: process identity queries have no pointer arguments.
    if unsafe { libc::getpid() } != 1 {
        bail!("The operation supervisor must be PID 1");
    }
    // SAFETY: the supervisor must not expose its control descriptors
    // through procfs.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut stdin = io::stdin();
    let line = operation_line(&mut stdin)?;
    let operation = serde_json::from_slice::<Operation>(&line)?;
    let mut task = Task::new(&operation)?;
    let reply = match network && !task.file {
        true => task.relay(),
        false => Ok(()),
    };
    let reply = match reply {
        Ok(()) => task.supervise(&line, &mut stdin)?,
        Err(reply) => {
            reap()?;
            reply
        }
    };
    serde_json::to_writer(io::stdout(), &reply)?;
    Ok(())
}

/// The operation's JSON, which ends at the first newline.
fn operation_line(stdin: &mut io::Stdin) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0];
        stdin.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            return Ok(line);
        }
        line.push(byte[0]);
        if line.len() > MAX_BODY {
            bail!("Oversize operation");
        }
    }
}

/// The process of one operation: its command, or the file helper.
struct Task {
    command: Command,
    timeout: Duration,
    file: bool,
}

impl Task {
    fn new(operation: &Operation) -> Result<Self> {
        let mut task = match operation {
            Operation::Exec(exec) => {
                if exec.argv.is_empty()
                    || exec.timeout_ms == 0
                    || exec.timeout_ms > 120_000
                {
                    bail!("Invalid command or timeout");
                }
                let mut command = Command::new(&exec.argv[0]);
                command.args(&exec.argv[1..]).current_dir(&exec.cwd);
                Self {
                    command,
                    timeout: Duration::from_millis(exec.timeout_ms),
                    file: false,
                }
            }
            _ => {
                let mut command = Command::new("/runner");
                command.arg("file");
                Self {
                    command,
                    timeout: Duration::from_secs(30),
                    file: true,
                }
            }
        };
        task.command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/house")
            .env("LANG", "C.UTF-8")
            .stdin(if task.file {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: these resource limits are scalar kernel calls in
        // the forked child.
        unsafe {
            task.command.pre_exec(|| {
                for (resource, limit) in [
                    (libc::RLIMIT_FSIZE, 32 * 1024 * 1024),
                    (libc::RLIMIT_NOFILE, 128),
                    (libc::RLIMIT_CPU, 120),
                ] {
                    let limits = libc::rlimit {
                        rlim_cur: limit,
                        rlim_max: limit,
                    };
                    if libc::setrlimit(resource, &limits) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(task)
    }

    /// Starts the proxy relay and points the command at it.
    fn relay(&mut self) -> Result<(), Reply> {
        start_relay()?;
        let proxy = format!("http://127.0.0.1:{PROXY_PORT}");
        for name in
            ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"]
        {
            self.command.env(name, &proxy);
        }
        Ok(())
    }

    /// Runs the process until it ends, is cancelled through `stdin`
    /// or times out, then drains every descendant and gives its
    /// reply.
    fn supervise(
        mut self,
        line: &[u8],
        stdin: &mut io::Stdin,
    ) -> Result<Reply> {
        let mut child = self.command.spawn()?;
        if self.file {
            child
                .stdin
                .take()
                .context("Missing file stdin")?
                .write_all(line)?;
        }
        let mut stdout =
            child.stdout.take().context("Missing stdout")?;
        let mut stderr =
            child.stderr.take().context("Missing stderr")?;
        for fd in [
            stdin.as_raw_fd(),
            stdout.as_raw_fd(),
            stderr.as_raw_fd(),
        ] {
            // SAFETY: these descriptors remain owned and open until
            // the supervisor returns.
            if unsafe {
                libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK)
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
        }
        let limit = if self.file { MAX_BODY } else { MAX_OUTPUT };
        let mut output = Output {
            stdout: Vec::new(),
            stderr: Vec::new(),
            limit,
        };
        let ending = self.watch(
            &mut child,
            &mut output,
            stdin,
            &mut stdout,
            &mut stderr,
        )?;
        reap()?;
        // Every writer is gone now, so this drain reaches end of file
        // and keeps bytes written after the last drain in the loop.
        let status = match ending {
            Ok(status) => status,
            Err(reply) => return Ok(reply),
        };
        output.drain(&mut stdout, &mut stderr)?;
        Ok(if output.is_full() {
            output_limit()
        } else if !self.file {
            Reply::ok(Ran {
                exit_code: status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout)
                    .into(),
                stderr: String::from_utf8_lossy(&output.stderr)
                    .into(),
            })
        } else if status.success() {
            serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|_| Reply::uncertain())
        } else {
            Reply::uncertain()
        })
    }

    /// Polls the process until it exits, or ends it as cancelled,
    /// timed out or over its output limit.
    fn watch(
        &self,
        child: &mut Child,
        output: &mut Output,
        stdin: &mut io::Stdin,
        stdout: &mut ChildStdout,
        stderr: &mut ChildStderr,
    ) -> Result<Result<ExitStatus, Reply>> {
        let started = Instant::now();
        loop {
            output.drain(stdout, stderr)?;
            if output.is_full() {
                return Ok(Err(output_limit()));
            }
            if let Some(status) = child.try_wait()? {
                return Ok(Ok(status));
            }
            let mut cancel = [0];
            match stdin.read(&mut cancel) {
                Ok(_) => {
                    return Ok(Err(Reply::error(
                        Code::Aborted,
                        "Command cancelled. Partial file changes \
                         may remain; inspect them before repeating \
                         it.",
                        false,
                    )));
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
            if started.elapsed() >= self.timeout {
                return Ok(Err(Reply::error(
                    Code::Timeout,
                    "Command timed out. Partial file changes may \
                     remain; inspect them before repeating it.",
                    false,
                )));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// What a process printed, up to `limit` bytes together.
struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    limit: usize,
}

impl Output {
    fn drain(
        &mut self,
        stdout: &mut ChildStdout,
        stderr: &mut ChildStderr,
    ) -> io::Result<()> {
        drain(stdout, &mut self.stdout, self.limit)?;
        drain(stderr, &mut self.stderr, self.limit)
    }

    fn is_full(&self) -> bool {
        self.stdout.len() + self.stderr.len() > self.limit
    }
}

/// Starts socat relaying loopback connections to the worker's proxy
/// and waits until it accepts one.
fn start_relay() -> Result<(), Reply> {
    let unavailable = || {
        Reply::error(
            Code::Unavailable,
            "The network proxy did not start, so the command did \
             not run.",
            false,
        )
    };
    if !fs::symlink_metadata(NETWORK_SOCKET)
        .is_ok_and(|metadata| metadata.file_type().is_socket())
    {
        return Err(unavailable());
    }
    let mut relay = Command::new("/usr/bin/socat")
        .arg(format!(
            "TCP-LISTEN:{PROXY_PORT},bind=127.0.0.1,reuseaddr,fork,\
             max-children=64"
        ))
        .arg(format!("UNIX-CONNECT:{NETWORK_SOCKET}"))
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| unavailable())?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, PROXY_PORT));
    let started = Instant::now();
    loop {
        if TcpStream::connect_timeout(
            &address,
            Duration::from_millis(100),
        )
        .is_ok()
        {
            return Ok(());
        }
        if !matches!(relay.try_wait(), Ok(None))
            || started.elapsed() >= Duration::from_secs(5)
        {
            return Err(unavailable());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Kills every other process in the PID namespace and stays until
/// waitpid proves none is left, including reparented double forks.
fn reap() -> Result<()> {
    loop {
        // SAFETY: only PID 1 calls this. kill(-1) excludes the
        // calling process.
        unsafe {
            libc::kill(-1, libc::SIGKILL);
        }
        let mut status = 0;
        // SAFETY: status points to live writable storage; -1 selects
        // only our children.
        let waited =
            unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if waited < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                return Ok(());
            }
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn output_limit() -> Reply {
    Reply::error(
        Code::OutputLimit,
        "Command stopped at its output limit. Inspect its files \
         before repeating it.",
        false,
    )
}

fn drain(
    stream: &mut impl Read,
    output: &mut Vec<u8>,
    limit: usize,
) -> io::Result<()> {
    let mut bytes = [0; 8192];
    while output.len() <= limit {
        match stream.read(&mut bytes) {
            Ok(0) => break,
            Ok(size) => output.extend_from_slice(&bytes[..size]),
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

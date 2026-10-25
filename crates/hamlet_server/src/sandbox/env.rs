//! The environment of an agent's tools: commands run under its
//! policy, and file operations made after the policy's checks on the
//! paths as the system resolves them.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use hamlet_os::fs::path::{self, text};
use hamlet_os::fs::{
    Directory, FileId, Follow, Kind, LastLink, Mode, Name, Open,
    Reached, Stat, SystemError, Unreached, system_error,
};
use hamlet_os::process::{self, Tree};
use hamlet_workspace_wire::Code;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::Instant;

use super::Sandbox;
use super::policy::{
    Policy, SetupError, canon_text, directory_root, fixed_roots,
    is_readable, is_writable, policy_hash, private_tmp, temp_root,
};
use crate::scrub::Scrubber;

/// How long output may still come once a command ended.
const EXIT_GRACE: Duration = Duration::from_millis(100);

/// Output kept in memory before the rest goes to a file, in bytes and
/// in lines.
const SPILL_BYTES: usize = 50 * 1024;
const SPILL_LINES: usize = 2000;

const ROOT_REFUSAL: &str = "The sandbox root is no longer its real \
                            directory. The operation was refused.";

const MOVED: &str = "The path changed while it was being reached. \
                     The operation was refused.";

/// The mode a tool's new file is made with, as the umask allows.
const NEW_FILE: Mode = Mode::new(0o666);

/// A tool's environment under `policy`, working in `cwd`.
pub(crate) struct Env {
    cwd: String,
    policy: Policy,
    sandbox: Sandbox,
    hash: String,
    tmp: String,
    output: String,
    roots: Vec<Root>,
    private: bool,
}

/// A root of the policy as it was when the environment was made.
#[derive(PartialEq)]
struct Root {
    path: String,
    real: String,
    id: Option<FileId>,
}

/// A failed file operation: its kind and the words pi's tools give
/// for it.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct FileError {
    pub(crate) code: Code,
    pub(crate) message: String,
}

impl FileError {
    fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    File,
    Directory,
    Symlink,
}

/// A file as `lstat` sees it.
#[derive(Debug)]
pub(crate) struct FileInfo {
    pub(crate) name: String,
    pub(crate) kind: FileKind,
    pub(crate) size: u64,
}

/// A command that ended by itself.
#[derive(Debug)]
pub(crate) struct Ran {
    pub(crate) exit_code: i32,
    /// The file the complete output was spilled to.
    pub(crate) spill: Option<String>,
}

/// A command that did not end by itself.
#[derive(Debug)]
pub(crate) enum ExecError {
    /// It could not start.
    Spawn(String),
    Timeout {
        spill: Option<String>,
    },
    /// Its complete output could not be kept.
    Spill(String),
}

#[derive(Clone, Copy)]
pub(crate) enum Launch<'a> {
    Shell(&'a str),
    Program { argv: &'a [String], input: &'a str },
}

#[derive(Clone, Copy)]
enum Reach {
    Read,
    Write,
}

impl Env {
    /// Pins the policy's roots and prepares the private temp folder
    /// and the folder spilled output goes to.
    pub(crate) fn new(
        cwd: &str,
        policy: Policy,
        sandbox: Sandbox,
    ) -> Result<Self, SetupError> {
        Self::open(cwd, policy, sandbox, false)
    }

    /// An environment with a separate scratch folder removed when the
    /// call ends.
    pub(crate) fn private(
        cwd: &str,
        policy: Policy,
        sandbox: Sandbox,
    ) -> Result<Self, SetupError> {
        Self::open(cwd, policy, sandbox, true)
    }

    fn open(
        cwd: &str,
        policy: Policy,
        sandbox: Sandbox,
        private: bool,
    ) -> Result<Self, SetupError> {
        for each in &policy.rw {
            directory_root(&text(each))?;
        }
        policy.check_limits()?;
        let hash = policy_hash(&policy, sandbox.name());
        let mut tmp = private_tmp(&policy)?;
        if private {
            tmp =
                path::join(&[tmp, uuid::Uuid::new_v4().to_string()]);
        }
        let output = path::join(&[tmp.as_str(), "tmp-files"]);
        directory_root(&output)?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&output)?;
        let mut policy = policy;
        policy.fixed.push(temp_root().into());
        policy.fixed.push(output.clone().into());
        let roots = fixed_roots(&policy, &tmp)
            .iter()
            .map(|root| root_at(root))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            cwd: cwd.to_owned(),
            policy,
            sandbox,
            hash,
            tmp,
            output,
            roots,
            private,
        })
    }

    pub(crate) fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The [`policy_hash`] of the policy and the sandbox commands run
    /// under.
    pub(crate) fn policy_hash(&self) -> &str {
        &self.hash
    }

    /// The absolute path `path` names from the working directory.
    pub(crate) fn absolute(&self, path: &str) -> String {
        path::resolve(&self.cwd, path)
    }

    fn root_refusal(&self) -> Option<FileError> {
        let rw = self
            .policy
            .rw
            .iter()
            .map(|p| text(p))
            .collect::<Vec<_>>();
        let moved = self.roots.iter().any(|root| {
            root_at(&root.path).map_or(true, |now| {
                now != *root
                    || (root.id.is_none() && rw.contains(&root.path))
            })
        });
        moved.then(|| {
            FileError::new(Code::PermissionDenied, ROOT_REFUSAL)
        })
    }

    /// Where `path` leads under the policy for `reach`: the folder
    /// its last name is in, held open, so that what the policy
    /// checked is what the operation reaches (S12). With `follow`
    /// the policy sees where a link at the end leads; without, the
    /// link itself, as `lstat` does.
    fn reach(
        &self,
        path: &str,
        reach: Reach,
        follow: Follow,
    ) -> Result<Reached, FileError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(refusal);
        }
        let resolved = self.absolute(path);
        let reached = hamlet_os::fs::reach(&resolved, follow)
            .map_err(|unreached| match unreached {
                Unreached::System(error) => {
                    file_error(&error, "open", &resolved)
                }
                Unreached::Moved => {
                    FileError::new(Code::PermissionDenied, MOVED)
                }
                Unreached::Loop => {
                    described(SystemError::LOOP, "open", &resolved)
                }
            })?;
        let (allowed, verb) = match reach {
            Reach::Read => (
                is_readable(&self.policy, &self.tmp, &reached.real),
                "read",
            ),
            Reach::Write => (
                is_writable(
                    &self.policy,
                    &self.tmp,
                    &reached.real,
                    false,
                ),
                "change",
            ),
        };
        if allowed {
            Ok(reached)
        } else {
            Err(FileError::new(
                Code::PermissionDenied,
                format!(
                    "{path} is out of this agent's reach to {verb}"
                ),
            ))
        }
    }

    pub(crate) fn file_info(
        &self,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        let at = self.reach(path, Reach::Read, Follow::NotLast)?;
        let stat = at.stat().map_err(|error| {
            file_error(&error, "lstat", &at.resolved)
        })?;
        info_of(path::basename(&at.resolved), &stat)
    }

    pub(crate) fn exists(
        &self,
        path: &str,
    ) -> Result<bool, FileError> {
        match self.file_info(path) {
            Ok(_) => Ok(true),
            Err(error) if error.code == Code::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The entries of a folder, but those neither file, folder nor
    /// link.
    pub(crate) fn list_dir(
        &self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError> {
        let at = self.reach(path, Reach::Read, Follow::Last)?;
        let scandir = |error: &io::Error| {
            file_error(error, "scandir", &at.resolved)
        };
        let dir = at.directory().map_err(|error| scandir(&error))?;
        let names = dir.names().map_err(|error| scandir(&error))?;
        let mut infos = Vec::new();
        for name in names {
            let text = name.to_string_lossy();
            let lstat = |error: &io::Error| {
                file_error(
                    error,
                    "lstat",
                    &path::join(&[at.resolved.as_str(), &text]),
                )
            };
            let stat = dir
                .stat(&name)
                .map_err(|error| lstat(&error))?
                .ok_or_else(|| lstat(&ErrorKind::NotFound.into()))?;
            match info_of(&text, &stat) {
                Ok(info) => infos.push(info),
                Err(error) if error.code == Code::Invalid => {}
                Err(error) => return Err(error),
            }
        }
        Ok(infos)
    }

    pub(crate) fn canonical_path(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        let at = self.reach(path, Reach::Read, Follow::Last)?;
        at.stat().map(|_| at.real.clone()).map_err(|error| {
            file_error(&error, "realpath", &at.resolved)
        })
    }

    /// The bytes of a regular file.
    pub(crate) fn read_file(
        &self,
        path: &str,
    ) -> Result<Vec<u8>, FileError> {
        let (mut file, resolved) = self.open_file(path)?;
        let mut bytes = Vec::new();
        io::Read::read_to_end(&mut file, &mut bytes)
            .map_err(|error| file_error(&error, "read", &resolved))?;
        Ok(bytes)
    }

    /// A regular file opened for reading, and its absolute path.
    pub(crate) fn open_file(
        &self,
        path: &str,
    ) -> Result<(fs::File, String), FileError> {
        let at = self.reach(path, Reach::Read, Follow::Last)?;
        let file = at.file(Open::Read).map_err(|error| {
            if error.kind() == ErrorKind::IsADirectory {
                FileError::new(
                    Code::IsDirectory,
                    "EISDIR: illegal operation on a directory, read",
                )
            } else {
                file_error(&error, "open", &at.resolved)
            }
        })?;
        Ok((file.into_file(), at.resolved))
    }

    /// The text of a file, refused unless it is UTF-8, so that an
    /// edit cannot write replacement characters back over it. A
    /// byte-order mark is kept.
    pub(crate) fn read_text(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        let bytes = self.read_file(path)?;
        String::from_utf8(bytes).map_err(|_| {
            FileError::new(
                Code::Invalid,
                format!("{path} is not UTF-8 text"),
            )
        })
    }

    /// Writes `content` over the file, making its folders.
    pub(crate) fn write_file(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        self.put(path, content, Open::Truncate(NEW_FILE))
    }

    fn append_file(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        self.put(path, content, Open::Extend(NEW_FILE))
    }

    /// Writes `content` to the file opened with `how`, making its
    /// folders.
    fn put(
        &self,
        path: &str,
        content: &str,
        how: Open,
    ) -> Result<(), FileError> {
        let mut at = self.reach(path, Reach::Write, Follow::Last)?;
        at.make_folders().map_err(|error| {
            file_error(
                &error,
                "mkdir",
                &path::resolve(&at.resolved, ".."),
            )
        })?;
        let file = at.file(how).map_err(|error| {
            file_error(&error, "open", &at.resolved)
        })?;
        file.file()
            .write_all(content.as_bytes())
            .map_err(|error| file_error(&error, "open", &at.resolved))
    }

    /// A new empty file for spilled output.
    pub(crate) fn create_temp_file(
        &self,
    ) -> Result<String, FileError> {
        let path = path::join(&[
            self.output.as_str(),
            &format!("pi-output-{}.log", uuid::Uuid::new_v4()),
        ]);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&path).map(|_| path).map_err(|error| {
            FileError::new(Code::Unknown, format!("Error: {error}"))
        })
    }

    /// Runs `launch` under the policy, for at most `timeout` and the
    /// policy's own limit. Its output, with `secrets` taken out, goes
    /// to `output` as it comes and, past the spill limits, all of it
    /// to a file.
    pub(crate) async fn exec(
        &self,
        launch: Launch<'_>,
        timeout: Duration,
        secrets: &[String],
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ran, ExecError> {
        let mut scrubber = Scrubber::new(secrets);
        let mut spill = Spill::default();
        let mut kept = |text: &str, spill: &mut Spill| {
            if !text.is_empty() {
                spill.push(self, text);
                output(text);
            }
        };
        let ran = self
            .spawned(launch, timeout, &mut |text| {
                kept(&scrubber.push(text), &mut spill);
            })
            .await;
        kept(&scrubber.flush(), &mut spill);
        let spilled = spill
            .finish()
            .map_err(|error| ExecError::Spill(error.message))?;
        match ran? {
            Ended::Exited(status) => Ok(Ran {
                exit_code: exit_code(&status),
                spill: spilled,
            }),
            Ended::TimedOut => {
                Err(ExecError::Timeout { spill: spilled })
            }
        }
    }

    async fn spawned(
        &self,
        launch: Launch<'_>,
        timeout: Duration,
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ended, ExecError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(ExecError::Spawn(refusal.message));
        }
        let argv = match launch {
            Launch::Shell(command) => {
                vec![
                    "bash".to_owned(),
                    "-c".into(),
                    command.to_owned(),
                ]
            }
            Launch::Program { argv, .. } => argv.to_vec(),
        };
        if !self.is_on_path("bash") {
            return Err(ExecError::Spawn(
                "bash: command not found".into(),
            ));
        }
        let limits = &self.policy.limits;
        let confined = self
            .sandbox
            .command(
                &self.policy,
                &process::limited(limits, &argv),
                &self.cwd,
                &self.tmp,
            )
            .map_err(|missing| {
                ExecError::Spawn(missing.to_string())
            })?;
        let timeout =
            timeout.min(Duration::from_secs(limits.wall_secs));
        if fs::metadata(&self.cwd).is_err() {
            return Err(ExecError::Spawn(format!(
                "Working directory does not exist: {}\nCannot \
                 execute bash commands.",
                self.cwd
            )));
        }
        let stdin = match launch {
            Launch::Shell(_) => Stdio::null(),
            Launch::Program { input, .. } => self
                .open_file(input)
                .map(|(file, _)| Stdio::from(file))
                .map_err(|error| ExecError::Spawn(error.message))?,
        };
        let program = &confined.argv[0];
        let (mut tree, mut child) = Tree::spawn(
            process::Command {
                argv: &confined.argv,
                env: &confined.env,
                cwd: &self.cwd,
                stdin,
            },
            limits,
        )
        .map_err(|error| {
            ExecError::Spawn(format!(
                "spawn {program} {}",
                errno_name(&error)
            ))
        })?;
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let mut decoders = [Decoder::default(), Decoder::default()];
        let mut buffers =
            [vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]];
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let mut timed_out = false;
        let mut status = None;
        let mut grace = Instant::now();
        loop {
            if status.is_some()
                && stdout.is_none()
                && stderr.is_none()
            {
                break;
            }
            let [out_buffer, err_buffer] = &mut buffers;
            tokio::select! {
                read = read_some(&mut stdout, out_buffer),
                    if stdout.is_some() =>
                {
                    match read {
                        Ok(0) | Err(_) => stdout = None,
                        Ok(n) => {
                            output(&decoders[0].decode(&out_buffer[..n]));
                            grace = Instant::now() + EXIT_GRACE;
                        }
                    }
                }
                read = read_some(&mut stderr, err_buffer),
                    if stderr.is_some() =>
                {
                    match read {
                        Ok(0) | Err(_) => stderr = None,
                        Ok(n) => {
                            output(&decoders[1].decode(&err_buffer[..n]));
                            grace = Instant::now() + EXIT_GRACE;
                        }
                    }
                }
                ended = child.wait(), if status.is_none() => {
                    status = Some(ended);
                    grace = Instant::now() + EXIT_GRACE;
                }
                () = &mut deadline, if !timed_out && status.is_none() => {
                    timed_out = true;
                    tree.kill();
                }
                () = tokio::time::sleep_until(grace), if status.is_some() => {
                    break;
                }
            }
        }
        // What runs on after the command ended stays, as it does
        // where the command was not stopped.
        tree.release();
        for decoder in &mut decoders {
            output(&decoder.finish());
        }
        match (timed_out, status) {
            (false, Some(Ok(status))) => Ok(Ended::Exited(status)),
            (false, Some(Err(error))) => {
                Err(ExecError::Spawn(error.to_string()))
            }
            _ => Ok(Ended::TimedOut),
        }
    }

    /// True when `program` names an executable file on the policy's
    /// `PATH`.
    fn is_on_path(&self, program: &str) -> bool {
        let dirs =
            self.policy.env.get("PATH").map_or("", String::as_str);
        dirs.split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| path::resolve(dir, program))
            .any(|candidate| process::is_executable(&candidate))
    }
}

enum Ended {
    Exited(ExitStatus),
    TimedOut,
}

async fn read_some(
    stream: &mut Option<impl AsyncRead + Unpin>,
    buffer: &mut [u8],
) -> io::Result<usize> {
    match stream {
        Some(stream) => stream.read(buffer).await,
        None => std::future::pending().await,
    }
}

/// UTF-8 of one stream decoded chunk by chunk as all of it at once:
/// an incomplete character is held back, an invalid one replaced, and
/// a byte-order mark at the start dropped.
#[derive(Default)]
struct Decoder {
    held: Vec<u8>,
    started: bool,
}

impl Decoder {
    fn decode(&mut self, bytes: &[u8]) -> String {
        self.held.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest = &self.held[..];
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    out.push_str(valid);
                    rest = &[];
                    break;
                }
                Err(error) => {
                    let (valid, after) =
                        rest.split_at(error.valid_up_to());
                    out.push_str(
                        std::str::from_utf8(valid).unwrap_or(""),
                    );
                    match error.error_len() {
                        Some(len) => {
                            out.push('\u{fffd}');
                            rest = &after[len..];
                        }
                        None => {
                            rest = after;
                            break;
                        }
                    }
                }
            }
        }
        self.held = rest.to_vec();
        self.started(out)
    }

    fn finish(&mut self) -> String {
        let out = if self.held.is_empty() {
            String::new()
        } else {
            String::from_utf8_lossy(&std::mem::take(&mut self.held))
                .into_owned()
        };
        self.started(out)
    }

    fn started(&mut self, out: String) -> String {
        if self.started || out.is_empty() {
            return out;
        }
        self.started = true;
        match out.strip_prefix('\u{feff}') {
            Some(rest) => rest.to_owned(),
            None => out,
        }
    }
}

/// The complete output, kept in memory only up to the spill limits.
#[derive(Default)]
struct Spill {
    buffer: String,
    bytes: usize,
    newlines: usize,
    spilling: bool,
    path: Option<String>,
    error: Option<FileError>,
}

impl Spill {
    fn push(&mut self, env: &Env, text: &str) {
        let text = if self.spilling {
            text.to_owned()
        } else {
            self.buffer.push_str(text);
            self.bytes += text.len();
            self.newlines += text.matches('\n').count();
            let lines =
                self.newlines + usize::from(!text.ends_with('\n'));
            if self.bytes <= SPILL_BYTES && lines <= SPILL_LINES {
                return;
            }
            self.spilling = true;
            std::mem::take(&mut self.buffer)
        };
        if self.error.is_some() {
            return;
        }
        let written = match &self.path {
            Some(path) => Ok(path.clone()),
            None => env.create_temp_file(),
        }
        .and_then(|path| {
            self.path = Some(path.clone());
            env.append_file(&path, &text)
        });
        if let Err(error) = written {
            self.error = Some(error);
        }
    }

    fn finish(self) -> Result<Option<String>, FileError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.path),
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        if self.private {
            let _ = fs::remove_dir_all(&self.tmp);
        }
    }
}

fn root_at(path: &str) -> Result<Root, SetupError> {
    let stat = own_stat(&path::resolve(".", path))?;
    if stat.is_some_and(|stat| {
        !matches!(stat.kind, Kind::Directory | Kind::File)
    }) {
        return Err(SetupError::NotReal(path.into()));
    }
    Ok(Root {
        path: path.to_owned(),
        real: canon_text(path),
        id: stat.map(|stat| stat.id),
    })
}

fn own_stat(path: &str) -> io::Result<Option<Stat>> {
    let Ok(name) = Name::new(path::basename(path)) else {
        return Directory::open(Path::new(path), LastLink::Refuse)?
            .status()
            .map(Some);
    };
    match Directory::open(
        Path::new(&path::dirname(path)),
        LastLink::Follow,
    ) {
        Ok(parent) => parent.stat(&name),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// `stat` of the file called `name`, as [`FileInfo`] keeps it.
fn info_of(name: &str, stat: &Stat) -> Result<FileInfo, FileError> {
    let kind = match stat.kind {
        Kind::File => FileKind::File,
        Kind::Directory => FileKind::Directory,
        Kind::Link => FileKind::Symlink,
        Kind::Special => {
            return Err(FileError::new(
                Code::Invalid,
                "Unsupported file type",
            ));
        }
    };
    Ok(FileInfo {
        name: name.to_owned(),
        kind,
        size: stat.size,
    })
}

/// The exit code of a command, or 128 and the signal that ended it.
fn exit_code(status: &ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    if let Some(signal) =
        std::os::unix::process::ExitStatusExt::signal(status)
    {
        return 128 + signal;
    }
    1
}

/// How a probe ended: its exit code, or the signal that ended it.
pub(super) fn status_word(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        return code.to_string();
    }
    #[cfg(unix)]
    if let Some(signal) =
        std::os::unix::process::ExitStatusExt::signal(status)
    {
        return match signal {
            1 => "SIGHUP".into(),
            2 => "SIGINT".into(),
            6 => "SIGABRT".into(),
            9 => "SIGKILL".into(),
            11 => "SIGSEGV".into(),
            13 => "SIGPIPE".into(),
            15 => "SIGTERM".into(),
            other => format!("SIG{other}"),
        };
    }
    "ended without an exit code or signal".into()
}

pub(super) fn errno_name(error: &io::Error) -> &'static str {
    system_error(error).map_or("UNKNOWN", |error| error.name)
}

/// `error` of `syscall` on `path` in pi's tools' words, and its
/// kind.
fn file_error(
    error: &io::Error,
    syscall: &str,
    path: &str,
) -> FileError {
    match system_error(error) {
        Some(system) => described(system, syscall, path),
        None if hamlet_os::fs::NotRegular::of(error).is_some() => {
            FileError::new(Code::Invalid, "Not a regular file")
        }
        None => FileError::new(Code::Unknown, error.to_string()),
    }
}

/// `error` of `syscall` on `path`, as pi's tools give it.
fn described(
    error: SystemError,
    syscall: &str,
    path: &str,
) -> FileError {
    let code = match error.name {
        "ENOENT" => Code::NotFound,
        "EACCES" | "EPERM" => Code::PermissionDenied,
        "ENOTDIR" => Code::NotDirectory,
        "EISDIR" => Code::IsDirectory,
        "EINVAL" => Code::Invalid,
        _ => Code::Unknown,
    };
    FileError::new(
        code,
        format!(
            "{}: {}, {syscall} '{path}'",
            error.name, error.words
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use hamlet_testing::Scratch;
    use hamlet_testing::links::symlink;

    use super::*;
    use crate::sandbox::reach::open_policy;

    /// Keeps `link` pointing at `inside` and `outside` by turns,
    /// each turn an atomic replacement, until `stop` is set.
    fn swapping(
        link: &Path,
        inside: &Path,
        outside: &Path,
        stop: &AtomicBool,
    ) {
        let fresh = link.with_extension("fresh");
        let mut turn = 0usize;
        while !stop.load(Ordering::Relaxed) {
            let target = if turn.is_multiple_of(2) {
                outside
            } else {
                inside
            };
            turn += 1;
            let _ = fs::remove_file(&fresh);
            symlink(target, &fresh).unwrap();
            fs::rename(&fresh, link).unwrap();
        }
    }

    #[test]
    fn a_link_swapped_under_a_file_operation_cannot_carry_it_past_the_policy()
     {
        let scratch = Scratch::new("env-swap");
        let home = scratch.join("home");
        let house = home.join("houses/home/0");
        let folder = scratch.join("folder");
        let inside = folder.join("inside");
        let outside = home.join("hidden");
        for each in [&house, &inside, &outside] {
            fs::create_dir_all(each).unwrap();
        }
        fs::write(outside.join("secret.txt"), "hidden\n").unwrap();
        fs::write(inside.join("secret.txt"), "shown\n").unwrap();
        let env = Env::new(
            &text(&folder),
            open_policy(&folder, &house, &home, &[]),
            Sandbox::platform(),
        )
        .unwrap();
        let link = folder.join("link");
        symlink(&inside, &link).unwrap();
        env.write_file("link/nested/new.txt", "allowed\n").unwrap();
        assert_eq!(
            env.read_text("link/nested/new.txt").unwrap(),
            "allowed\n"
        );
        assert_eq!(
            fs::read_to_string(inside.join("nested/new.txt"))
                .unwrap(),
            "allowed\n"
        );
        let stop = AtomicBool::new(false);
        let mut written = 0;
        let mut read = 0;
        let mut leaked = false;
        std::thread::scope(|scope| {
            scope.spawn(|| swapping(&link, &inside, &outside, &stop));
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(2) {
                if env.write_file("link/out.txt", "escaped\n").is_ok()
                {
                    written += 1;
                }
                if let Ok(text) = env.read_text("link/secret.txt") {
                    leaked |= text == "hidden\n";
                    read += 1;
                }
            }
            stop.store(true, Ordering::Relaxed);
        });

        assert!(
            !leaked,
            "a read through a swapped link reached the hidden home"
        );
        assert!(
            !outside.join("out.txt").exists(),
            "a write through a swapped link landed outside the \
             policy"
        );
        assert!(written > 0 && read > 0);
    }

    #[test]
    fn a_root_replaced_by_another_folder_is_refused() {
        let scratch = Scratch::new("env-root");
        let home = scratch.join("home");
        let house = home.join("houses/home/0");
        let folder = scratch.join("folder");
        for each in [&house, &folder] {
            fs::create_dir_all(each).unwrap();
        }
        let env = Env::new(
            &text(&folder),
            open_policy(&folder, &house, &home, &[]),
            Sandbox::platform(),
        )
        .unwrap();
        env.write_file("file", "kept\n").unwrap();
        fs::rename(&folder, scratch.join("old")).unwrap();
        fs::create_dir(&folder).unwrap();
        assert_eq!(
            env.write_file("file", "lost\n").unwrap_err().message,
            ROOT_REFUSAL
        );
        assert!(!folder.join("file").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_and_a_fifo_are_read_and_written_as_no_regular_file() {
        let scratch = Scratch::new("env-special");
        let home = scratch.join("home");
        let house = home.join("houses/home/0");
        let folder = scratch.join("folder");
        fs::create_dir_all(&house).unwrap();
        fs::create_dir_all(folder.join("inner")).unwrap();
        hamlet_testing::fifo(&folder.join("pipe"));
        let env = Env::new(
            &text(&folder),
            open_policy(&folder, &house, &home, &[]),
            Sandbox::platform(),
        )
        .unwrap();
        let said = |error: FileError| (error.code, error.message);
        assert_eq!(
            said(env.read_file("inner").unwrap_err()),
            (
                Code::IsDirectory,
                "EISDIR: illegal operation on a directory, read"
                    .into()
            )
        );
        assert_eq!(
            said(env.read_file("pipe").unwrap_err()),
            (Code::Invalid, "Not a regular file".into())
        );
        // Read and write, so opening the FIFO waits for no writer.
        let _reader = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(folder.join("pipe"))
            .unwrap();
        assert_eq!(
            said(env.write_file("pipe", "x").unwrap_err()),
            (Code::Invalid, "Not a regular file".into())
        );
    }
}

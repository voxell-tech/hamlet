//! The environment of an agent's tools: commands run under its
//! policy, and file operations made after the policy's checks on the
//! paths as the system resolves them.

use std::collections::VecDeque;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::os::fd::OwnedFd;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use hamlet_workspace_wire::Code;
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, RawMode, Stat, mkdirat,
    openat, readlinkat, statat,
};
use rustix::io::Errno;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::Instant;

use super::path::{self, text};
use super::policy::{
    MAX_LINKS, Policy, SetupError, canon_text, cased, directory_root,
    fixed_roots, is_readable, is_writable, policy_hash, private_tmp,
    temp_root,
};
use super::{Sandbox, limited};
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
    id: Option<(u64, u64)>,
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
        follow: bool,
    ) -> Result<Reached, FileError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(refusal);
        }
        let resolved = self.absolute(path);
        let reached = walk(&resolved, follow)?;
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
        let at = self.reach(path, Reach::Read, false)?;
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
        let at = self.reach(path, Reach::Read, true)?;
        let scandir = |error: &io::Error| {
            file_error(error, "scandir", &at.resolved)
        };
        let dir = at
            .open(OFlags::DIRECTORY | OFlags::RDONLY)
            .map_err(|error| scandir(&error))?;
        let entries = Dir::read_from(&dir)
            .map_err(|error| scandir(&error.into()))?;
        let mut infos = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|error| scandir(&error.into()))?;
            let name =
                entry.file_name().to_string_lossy().into_owned();
            if name == "." || name == ".." {
                continue;
            }
            let stat = statat(&dir, &name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|error| {
                    file_error(
                        &error.into(),
                        "lstat",
                        &path::join(&[at.resolved.as_str(), &name]),
                    )
                })?;
            match info_of(&name, &stat) {
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
        let at = self.reach(path, Reach::Read, true)?;
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
        let at = self.reach(path, Reach::Read, true)?;
        // Nonblocking, so opening a FIFO does not wait for a writer.
        let file =
            at.open(OFlags::RDONLY | OFlags::NONBLOCK).map_err(
                |error| file_error(&error, "open", &at.resolved),
            )?;
        let meta = file.metadata().map_err(|error| {
            file_error(&error, "fstat", &at.resolved)
        })?;
        if meta.is_dir() {
            return Err(FileError::new(
                Code::IsDirectory,
                "EISDIR: illegal operation on a directory, read",
            ));
        }
        if !meta.is_file() {
            return Err(FileError::new(
                Code::Invalid,
                "Not a regular file",
            ));
        }
        Ok((file, at.resolved))
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
        self.put(path, content, OFlags::TRUNC)
    }

    fn append_file(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        self.put(path, content, OFlags::APPEND)
    }

    /// Writes `content` to the file opened with `how`, making its
    /// folders.
    fn put(
        &self,
        path: &str,
        content: &str,
        how: OFlags,
    ) -> Result<(), FileError> {
        let mut at = self.reach(path, Reach::Write, true)?;
        at.make_folders()?;
        let mut file =
            at.open(OFlags::WRONLY | OFlags::CREATE | how).map_err(
                |error| file_error(&error, "open", &at.resolved),
            )?;
        file.write_all(content.as_bytes())
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

    /// Runs `command` with bash under the policy, for at most
    /// `timeout` and the policy's own limit. Its output, with
    /// `secrets` taken out, goes to `output` as it comes and, past
    /// the spill limits, all of it to a file.
    pub(crate) async fn exec(
        &self,
        command: &str,
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
            .spawned(command, timeout, &mut |text| {
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
        command: &str,
        timeout: Duration,
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ended, ExecError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(ExecError::Spawn(refusal.message));
        }
        let argv =
            ["bash".to_owned(), "-c".into(), command.to_owned()];
        // The sandbox's own program always starts, so a missing one
        // is found here.
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
                &limited(limits, &argv),
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
        let program = &confined.argv[0];
        let mut spawn = tokio::process::Command::new(program);
        spawn
            .args(&confined.argv[1..])
            .env_clear()
            .envs(&confined.env)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        spawn.process_group(0);
        let mut child = spawn.spawn().map_err(|error| {
            ExecError::Spawn(format!(
                "spawn {program} {}",
                errno_name(&error)
            ))
        })?;
        let mut group = Group(child.id());
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
                    group.kill();
                }
                () = tokio::time::sleep_until(grace), if status.is_some() => {
                    break;
                }
            }
        }
        // What runs on after the command ended stays, as it does
        // where the command was not stopped.
        group.0 = None;
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
            .any(|candidate| is_executable(&candidate))
    }
}

enum Ended {
    Exited(ExitStatus),
    TimedOut,
}

/// The process group of a running command, killed when it is dropped
/// unless it was let go.
struct Group(Option<u32>);

impl Group {
    fn kill(&mut self) {
        #[cfg(unix)]
        {
            use rustix::process::{
                Pid, Signal, kill_process, kill_process_group,
            };
            if let Some(pid) =
                self.0.and_then(|id| Pid::from_raw(id as i32))
                && kill_process_group(pid, Signal::KILL).is_err()
            {
                let _ = kill_process(pid, Signal::KILL);
            }
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
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
    let meta = match fs::symlink_metadata(path::resolve(".", path)) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if meta
        .as_ref()
        .is_some_and(|meta| !meta.is_dir() && !meta.is_file())
    {
        return Err(SetupError::NotReal(path.into()));
    }
    Ok(Root {
        path: path.to_owned(),
        real: canon_text(path),
        id: meta.as_ref().map(identity),
    })
}

#[cfg(unix)]
fn identity(meta: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn identity(_meta: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}

/// Where a path leads, reached one name at a time and never through
/// a link the policy has not seen: the deepest folder on the way
/// that exists, held open, and the names from there. All but the
/// last are missing. An operation goes on from the folder with the
/// system told to follow no link, so a link swapped in after the
/// check leads nowhere.
struct Reached {
    dir: OwnedFd,
    names: Vec<String>,
    /// The real path, the one the policy checks.
    real: String,
    /// The path as the agent gave it, made absolute, for messages.
    resolved: String,
}

impl Reached {
    fn last(&self) -> &str {
        self.names.last().map_or(".", String::as_str)
    }

    fn missing(&self) -> Option<io::Error> {
        (self.names.len() > 1).then(|| io::Error::from(Errno::NOENT))
    }

    /// The file's own `lstat`.
    fn stat(&self) -> io::Result<Stat> {
        if let Some(missing) = self.missing() {
            return Err(missing);
        }
        statat(&self.dir, self.last(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(Into::into)
    }

    /// The file opened with `flags` and no link followed.
    fn open(&self, flags: OFlags) -> io::Result<fs::File> {
        if let Some(missing) = self.missing() {
            return Err(missing);
        }
        openat(
            &self.dir,
            self.last(),
            flags | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
        )
        .map(fs::File::from)
        .map_err(Into::into)
    }

    /// Makes the missing folders, so that only the file is left.
    fn make_folders(&mut self) -> Result<(), FileError> {
        while self.names.len() > 1 {
            let name = self.names.remove(0);
            let parent = path::resolve(&self.resolved, "..");
            let made = match mkdirat(
                &self.dir,
                &name,
                Mode::from_raw_mode(0o777),
            ) {
                Ok(()) | Err(Errno::EXIST) => {
                    open_dir_at(&self.dir, &name)
                }
                Err(error) => Err(error),
            };
            self.dir = made.map_err(|error| {
                file_error(&error.into(), "mkdir", &parent)
            })?;
        }
        Ok(())
    }
}

/// `resolved`, an absolute path, reached from the root one name at
/// a time, each link on the way read and followed by name. With
/// `follow`, a link at the end is followed too. The names that do
/// not exist stay as written, as [`canon_text`] keeps them.
fn walk(resolved: &str, follow: bool) -> Result<Reached, FileError> {
    let mut rest = resolved
        .split('/')
        .map(str::to_owned)
        .collect::<VecDeque<_>>();
    let mut base = "/".to_owned();
    let mut dir = open_dir(&base).map_err(|error| {
        file_error(&error.into(), "open", resolved)
    })?;
    let mut links = 0;
    // Only the case of the names may differ from the folder held:
    // the system following a link swapped in meanwhile would.
    let reached = |dir, base: String, names: Vec<String>| {
        let real = cased(&base);
        if real.to_lowercase() != base.to_lowercase() {
            return Err(FileError::new(
                Code::PermissionDenied,
                MOVED,
            ));
        }
        let mut parts = vec![real];
        parts.extend(names.iter().cloned());
        Ok(Reached {
            dir,
            real: path::join(&parts),
            names,
            resolved: resolved.to_owned(),
        })
    };
    while let Some(part) = rest.pop_front() {
        if part.is_empty() || part == "." {
            continue;
        }
        // `base` holds no link, so its parent is the folder `..`
        // leads to.
        if part == ".." {
            base = path::dirname(&base);
            dir = open_dir(&base).map_err(|error| {
                file_error(&error.into(), "open", resolved)
            })?;
            continue;
        }
        let kind = statat(&dir, &part, AtFlags::SYMLINK_NOFOLLOW)
            .map(|stat| {
                FileType::from_raw_mode(stat.st_mode as RawMode)
            });
        let last = rest.is_empty();
        let link = match kind {
            Ok(FileType::Symlink) if follow || !last => {
                readlinkat(&dir, &part, Vec::new()).ok()
            }
            _ => None,
        };
        if let Some(link) = link {
            // The system refuses a path through more links, so it
            // leads nowhere.
            links += 1;
            if links > MAX_LINKS {
                return Err(file_error(
                    &io::Error::from(Errno::LOOP),
                    "open",
                    resolved,
                ));
            }
            let link = link.to_string_lossy().into_owned();
            if path::is_absolute(&link) {
                base = "/".into();
                dir = open_dir(&base).map_err(|error| {
                    file_error(&error.into(), "open", resolved)
                })?;
            }
            for piece in link.split('/').rev() {
                rest.push_front(piece.to_owned());
            }
            continue;
        }
        let below = match kind {
            Ok(FileType::Directory) if !last => {
                open_dir_at(&dir, &part).ok()
            }
            _ => None,
        };
        match below {
            Some(below) => {
                dir = below;
                base = path::join(&[base.as_str(), &part]);
            }
            None => {
                // The end, or a name the rest cannot be reached
                // under: the operation fails on it as the system
                // says.
                let mut names = vec![part];
                names.extend(rest);
                return reached(dir, base, names);
            }
        }
    }
    reached(dir, base, Vec::new())
}

/// `real`, a path without links, opened as a folder one name at a
/// time, following none.
fn open_dir(real: &str) -> rustix::io::Result<OwnedFd> {
    let mut dir = openat(
        rustix::fs::CWD,
        "/",
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    for name in real.split('/').filter(|name| !name.is_empty()) {
        dir = open_dir_at(&dir, name)?;
    }
    Ok(dir)
}

fn open_dir_at(
    dir: &OwnedFd,
    name: &str,
) -> rustix::io::Result<OwnedFd> {
    openat(
        dir,
        name,
        OFlags::DIRECTORY
            | OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::empty(),
    )
}

/// `stat` of the file called `name`, as [`FileInfo`] keeps it.
fn info_of(name: &str, stat: &Stat) -> Result<FileInfo, FileError> {
    let kind = match FileType::from_raw_mode(stat.st_mode as RawMode)
    {
        FileType::RegularFile => FileKind::File,
        FileType::Directory => FileKind::Directory,
        FileType::Symlink => FileKind::Symlink,
        _ => {
            return Err(FileError::new(
                Code::Invalid,
                "Unsupported file type",
            ));
        }
    };
    Ok(FileInfo {
        name: name.to_owned(),
        kind,
        size: stat.st_size as u64,
    })
}

#[cfg(unix)]
fn is_executable(path: &str) -> bool {
    rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

#[cfg(not(unix))]
fn is_executable(path: &str) -> bool {
    std::path::Path::new(path).exists()
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

/// The name of the system error behind `error`, and its words, as
/// pi's tools give them.
fn errno(error: &io::Error) -> Option<(&'static str, &'static str)> {
    #[cfg(unix)]
    {
        use rustix::io::Errno;
        let errno = Errno::from_io_error(error)?;
        let known = [
            (Errno::NOENT, "ENOENT", "no such file or directory"),
            (Errno::ACCESS, "EACCES", "permission denied"),
            (Errno::PERM, "EPERM", "operation not permitted"),
            (
                Errno::ISDIR,
                "EISDIR",
                "illegal operation on a directory",
            ),
            (Errno::NOTDIR, "ENOTDIR", "not a directory"),
            (Errno::EXIST, "EEXIST", "file already exists"),
            (
                Errno::LOOP,
                "ELOOP",
                "too many symbolic links encountered",
            ),
            (Errno::NAMETOOLONG, "ENAMETOOLONG", "name too long"),
            (Errno::ROFS, "EROFS", "read-only file system"),
            (Errno::NOSPC, "ENOSPC", "no space left on device"),
            (Errno::INVAL, "EINVAL", "invalid argument"),
            (Errno::BUSY, "EBUSY", "resource busy or locked"),
            (Errno::MFILE, "EMFILE", "too many open files"),
            (Errno::TXTBSY, "ETXTBSY", "text file is busy"),
            (Errno::IO, "EIO", "i/o error"),
            (Errno::FBIG, "EFBIG", "file too large"),
            (
                Errno::AGAIN,
                "EAGAIN",
                "resource temporarily unavailable",
            ),
            (Errno::NXIO, "ENXIO", "no such device or address"),
            (Errno::NOTEMPTY, "ENOTEMPTY", "directory not empty"),
            (Errno::XDEV, "EXDEV", "cross-device link not permitted"),
        ];
        known
            .into_iter()
            .find(|(each, _, _)| *each == errno)
            .map(|(_, name, words)| (name, words))
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        None
    }
}

pub(super) fn errno_name(error: &io::Error) -> &'static str {
    errno(error).map_or("UNKNOWN", |(name, _)| name)
}

/// `error` of `syscall` on `path` in pi's tools' words, and its
/// kind.
fn file_error(
    error: &io::Error,
    syscall: &str,
    path: &str,
) -> FileError {
    let Some((name, words)) = errno(error) else {
        return FileError::new(Code::Unknown, error.to_string());
    };
    let code = match name {
        "ENOENT" => Code::NotFound,
        "EACCES" | "EPERM" => Code::PermissionDenied,
        "ENOTDIR" => Code::NotDirectory,
        "EISDIR" => Code::IsDirectory,
        "EINVAL" => Code::Invalid,
        _ => Code::Unknown,
    };
    FileError::new(
        code,
        format!("{name}: {words}, {syscall} '{path}'"),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use hamlet_testing::Scratch;

    use super::*;
    use crate::sandbox::reach::open_policy;

    /// Keeps `link` pointing at `inside` and `outside` by turns,
    /// each turn an atomic replacement, until `stop` is set.
    fn swapping(
        link: &std::path::Path,
        inside: &std::path::Path,
        outside: &std::path::Path,
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
            std::os::unix::fs::symlink(target, &fresh).unwrap();
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
        std::os::unix::fs::symlink(&inside, &link).unwrap();
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
}

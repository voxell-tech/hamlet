//! The environment of an agent's tools: commands run under its
//! policy, and file operations made after the policy's checks on the
//! paths as the system resolves them.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::Instant;

use super::path::{self, text};
use super::policy::{
    Policy, canon_text, directory_root, fixed_roots, policy_hash,
    private_tmp, readable, temp_root, writable,
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

/// A failed file operation: Node's error code and the words for it.
#[derive(Debug)]
pub(crate) struct FileError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl FileError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
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
    ) -> Result<Self, String> {
        Self::open(cwd, policy, sandbox, false)
    }

    /// An environment with a separate scratch folder removed when the
    /// call ends.
    pub(crate) fn private(
        cwd: &str,
        policy: Policy,
        sandbox: Sandbox,
    ) -> Result<Self, String> {
        Self::open(cwd, policy, sandbox, true)
    }

    fn open(
        cwd: &str,
        policy: Policy,
        sandbox: Sandbox,
        private: bool,
    ) -> Result<Self, String> {
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
        builder.create(&output).map_err(|error| error.to_string())?;
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
            FileError::new("permission_denied", ROOT_REFUSAL)
        })
    }

    /// Why the policy refuses `path` to `reach`, if it does.
    fn refused(
        &self,
        path: &str,
        reach: Reach,
    ) -> Result<String, FileError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(refusal);
        }
        let real = self.absolute(path);
        let (allowed, verb) = match reach {
            Reach::Read => {
                (readable(&self.policy, &self.tmp, &real), "read")
            }
            Reach::Write => (
                writable(&self.policy, &self.tmp, &real, false),
                "change",
            ),
        };
        if allowed {
            Ok(real)
        } else {
            Err(FileError::new(
                "permission_denied",
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
        let resolved = self.refused(path, Reach::Read)?;
        info_at(&resolved)
    }

    pub(crate) fn exists(
        &self,
        path: &str,
    ) -> Result<bool, FileError> {
        match self.file_info(path) {
            Ok(_) => Ok(true),
            Err(error) if error.code == "not_found" => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The entries of a folder, but those neither file, folder nor
    /// link.
    pub(crate) fn list_dir(
        &self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError> {
        let resolved = self.refused(path, Reach::Read)?;
        let entries = fs::read_dir(&resolved).map_err(|error| {
            node_error(&error, "scandir", &resolved)
        })?;
        let mut infos = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                node_error(&error, "scandir", &resolved)
            })?;
            let at = path::join(&[
                resolved.as_str(),
                &entry.file_name().to_string_lossy(),
            ]);
            match info_at(&at) {
                Ok(info) => infos.push(info),
                Err(error) if error.code == "invalid" => {}
                Err(error) => return Err(error),
            }
        }
        Ok(infos)
    }

    pub(crate) fn canonical_path(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        let resolved = self.refused(path, Reach::Read)?;
        fs::canonicalize(&resolved).map(|real| text(&real)).map_err(
            |error| node_error(&error, "realpath", &resolved),
        )
    }

    /// The bytes of a regular file.
    pub(crate) fn read_file(
        &self,
        path: &str,
    ) -> Result<Vec<u8>, FileError> {
        let (mut file, resolved) = self.open_file(path)?;
        let mut bytes = Vec::new();
        io::Read::read_to_end(&mut file, &mut bytes)
            .map_err(|error| node_error(&error, "read", &resolved))?;
        Ok(bytes)
    }

    /// A regular file opened for reading, and its absolute path.
    pub(crate) fn open_file(
        &self,
        path: &str,
    ) -> Result<(fs::File, String), FileError> {
        let resolved = self.refused(path, Reach::Read)?;
        let mut options = fs::OpenOptions::new();
        options.read(true);
        // Nonblocking, so opening a FIFO does not wait for a writer.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::custom_flags(
            &mut options,
            rustix::fs::OFlags::NONBLOCK.bits() as i32,
        );
        let file = options
            .open(&resolved)
            .map_err(|error| node_error(&error, "open", &resolved))?;
        let meta = file.metadata().map_err(|error| {
            node_error(&error, "fstat", &resolved)
        })?;
        if meta.is_dir() {
            return Err(FileError::new(
                "is_directory",
                "EISDIR: illegal operation on a directory, read",
            ));
        }
        if !meta.is_file() {
            return Err(FileError::new(
                "invalid",
                "Not a regular file",
            ));
        }
        Ok((file, resolved))
    }

    /// The text of a file, refused unless it is UTF-8, so that an
    /// edit cannot write replacement characters back over it. A
    /// byte-order mark is kept.
    pub(crate) fn read_text(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        let resolved = self.refused(path, Reach::Read)?;
        let bytes = fs::read(&resolved).map_err(|error| {
            if error.kind() == ErrorKind::IsADirectory {
                FileError::new(
                    "is_directory",
                    "EISDIR: illegal operation on a directory, read",
                )
            } else {
                node_error(&error, "open", &resolved)
            }
        })?;
        String::from_utf8(bytes).map_err(|_| {
            FileError::new(
                "invalid",
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
        let resolved = self.refused(path, Reach::Write)?;
        make_parent(&resolved)?;
        fs::write(&resolved, content)
            .map_err(|error| node_error(&error, "open", &resolved))
    }

    fn append_file(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        let resolved = self.refused(path, Reach::Write)?;
        make_parent(&resolved)?;
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&resolved)
            .and_then(|mut file| file.write_all(content.as_bytes()))
            .map_err(|error| node_error(&error, "open", &resolved))
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
            FileError::new("unknown", format!("Error: {error}"))
        })
    }

    /// Runs `command` with bash under the policy, for at most
    /// `timeout` seconds and the policy's own limit. Its output, with
    /// `secrets` taken out, goes to `output` as it comes and, past
    /// the spill limits, all of it to a file.
    pub(crate) async fn exec(
        &self,
        command: &str,
        timeout: f64,
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
                kept(&scrubber.push(text), &mut spill)
            })
            .await;
        kept(&scrubber.flush(), &mut spill);
        let spilled = spill.finish().map_err(ExecError::Spill)?;
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
        timeout: f64,
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ended, ExecError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(ExecError::Spawn(refusal.message));
        }
        let argv =
            ["bash".to_owned(), "-c".into(), command.to_owned()];
        // The sandbox's own program always starts, so a missing one
        // is found here.
        if !self.found("bash") {
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
            .map_err(ExecError::Spawn)?;
        let timeout = timeout.min(limits.wall_secs as f64);
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
        let deadline =
            tokio::time::sleep(Duration::from_secs_f64(timeout));
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
    fn found(&self, program: &str) -> bool {
        let dirs =
            self.policy.env.get("PATH").map_or("", String::as_str);
        dirs.split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| path::resolve(dir, program))
            .any(|candidate| executable(&candidate))
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
    error: Option<String>,
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
            self.error = Some(error.message);
        }
    }

    fn finish(self) -> Result<Option<String>, String> {
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

fn root_at(path: &str) -> Result<Root, String> {
    let meta = match fs::symlink_metadata(path::resolve(".", path)) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    if meta
        .as_ref()
        .is_some_and(|meta| !meta.is_dir() && !meta.is_file())
    {
        return Err(format!(
            "The sandbox root must be a real file or directory: \
             {path}"
        ));
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

fn info_at(path: &str) -> Result<FileInfo, FileError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| node_error(&error, "lstat", path))?;
    let kind = if meta.is_file() {
        FileKind::File
    } else if meta.is_dir() {
        FileKind::Directory
    } else if meta.file_type().is_symlink() {
        FileKind::Symlink
    } else {
        return Err(FileError::new(
            "invalid",
            "Unsupported file type",
        ));
    };
    Ok(FileInfo {
        name: path::basename(path).to_owned(),
        kind,
        size: meta.len(),
    })
}

fn make_parent(path: &str) -> Result<(), FileError> {
    let parent = path::resolve(path, "..");
    fs::create_dir_all(&parent)
        .map_err(|error| node_error(&error, "mkdir", &parent))
}

#[cfg(unix)]
fn executable(path: &str) -> bool {
    rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

#[cfg(not(unix))]
fn executable(path: &str) -> bool {
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
    "null".into()
}

/// Node's name of the system error behind `error`, and its words.
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

/// `error` of `syscall` on `path` as Node words and classifies it.
fn node_error(
    error: &io::Error,
    syscall: &str,
    path: &str,
) -> FileError {
    let Some((name, words)) = errno(error) else {
        return FileError::new("unknown", error.to_string());
    };
    let code = match name {
        "ENOENT" => "not_found",
        "EACCES" | "EPERM" => "permission_denied",
        "ENOTDIR" => "not_directory",
        "EISDIR" => "is_directory",
        "EINVAL" => "invalid",
        _ => "unknown",
    };
    FileError::new(
        code,
        format!("{name}: {words}, {syscall} '{path}'"),
    )
}

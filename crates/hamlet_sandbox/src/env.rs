//! The environment of an agent's tools: commands run under its
//! policy, and file operations made after the policy's checks on the
//! paths as the system resolves them.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use hamlet_os::fs::path::{self, text};
use hamlet_os::fs::{
    FileId, Follow, Kind, Mode, Name, Open, Reached, Stat,
    SystemError, Unreached, system_error,
};
use hamlet_os::process::{self, Tree};
use hamlet_workspace_wire::Code;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::Instant;

use super::Sandbox;
use super::policy::{
    Policy, Reach, SetupError, canon_text, directory_root,
    fixed_roots, is_readable, is_writable, policy_hash, private_tmp,
    temp_root,
};
use crate::scrub::Scrubber;

/// How long output may still come once a command ended.
const EXIT_GRACE: Duration = Duration::from_millis(100);

/// The leftovers of every agent's commands, by the agent's id.
#[derive(Clone, Default)]
pub struct AllLeftovers(Arc<Mutex<Vec<(i64, process::Leftovers)>>>);

impl AllLeftovers {
    /// Where the commands of the agent `owner` leave theirs.
    pub fn of(&self, owner: i64) -> Leftovers {
        Leftovers {
            all: self.clone(),
            owner,
        }
    }

    /// Ends the leftovers of every agent `which` picks.
    pub fn end(&self, which: impl Fn(i64) -> bool) {
        let ended = {
            let mut all =
                self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.extract_if(.., |(owner, _)| which(*owner))
                .collect::<Vec<_>>()
        };
        for (_, leftovers) in ended {
            leftovers.end();
        }
    }
}

/// The leftovers of one agent's commands.
#[derive(Clone)]
pub struct Leftovers {
    all: AllLeftovers,
    owner: i64,
}

impl Leftovers {
    pub fn keep(&self, leftovers: process::Leftovers) {
        let mut all =
            self.all.0.lock().unwrap_or_else(PoisonError::into_inner);
        // Dropping a record whose group ended frees its id.
        all.retain(|(_, kept)| kept.is_running());
        all.push((self.owner, leftovers));
    }
}

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
pub struct Env {
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
pub struct FileError {
    pub code: Code,
    pub message: String,
}

impl FileError {
    fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// The file a change would replace, as [`Env::read_for_change`] read
/// it.
#[derive(Debug, PartialEq, Eq)]
pub struct Current {
    /// The real path the change leads to.
    pub real: String,
    /// The SHA-256 of the regular file's bytes, or `None` where no
    /// file is.
    pub digest: Option<[u8; 32]>,
}

/// The outcome of [`Env::replace_file`].
#[derive(Debug, PartialEq, Eq)]
pub enum Replaced {
    /// The file holds the new content. It was not there before when
    /// `created`.
    Done { created: bool },
    /// The file no longer held what was read, so nothing was written:
    /// the SHA-256 of what it holds now, or `None` where no file is.
    Changed(Option<[u8; 32]>),
}

/// The SHA-256 of the regular file `at` names, or `None` where
/// nothing is. Each part of its bytes goes to `part` as it is read.
fn digest(
    at: &Reached,
    part: &mut impl FnMut(&[u8]),
) -> Result<Option<[u8; 32]>, FileError> {
    let mut file = match at.file(Open::Read) {
        Ok(file) => file.into_file(),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => {
            return Err(file_error(&error, "open", &at.resolved));
        }
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 << 10];
    loop {
        match io::Read::read(&mut file, &mut buffer) {
            Ok(0) => return Ok(Some(hasher.finalize().into())),
            Ok(n) => {
                hasher.update(&buffer[..n]);
                part(&buffer[..n]);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(file_error(&error, "read", &at.resolved));
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

/// A file as `lstat` sees it.
#[derive(Debug)]
pub struct FileInfo {
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
}

/// A command that ended by itself.
#[derive(Debug)]
pub struct Ran {
    pub exit_code: i32,
    /// The file the complete output was spilled to.
    pub spill: Option<String>,
}

/// A command that did not end by itself.
#[derive(Debug)]
pub enum ExecError {
    /// It could not start.
    Spawn(String),
    Timeout {
        spill: Option<String>,
    },
    /// Its complete output could not be kept.
    Spill(String),
}

#[derive(Clone, Copy)]
pub enum Launch<'a> {
    /// A shell's argv, which reads nothing.
    Shell(&'a [String]),
    Program {
        argv: &'a [String],
        input: &'a str,
    },
}

impl<'a> Launch<'a> {
    fn argv(self) -> &'a [String] {
        match self {
            Self::Shell(argv) | Self::Program { argv, .. } => argv,
        }
    }
}

impl Env {
    /// Pins the policy's roots and prepares the private temp folder
    /// and the folder spilled output goes to.
    pub fn new(
        cwd: &str,
        policy: Policy,
        sandbox: Sandbox,
    ) -> Result<Self, SetupError> {
        Self::open(cwd, policy, sandbox, false)
    }

    /// An environment with a separate scratch folder removed when the
    /// call ends.
    pub fn private(
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

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The private temp folder commands get.
    pub fn tmp(&self) -> &str {
        &self.tmp
    }

    /// The [`policy_hash`] of the policy and the sandbox commands run
    /// under.
    pub fn policy_hash(&self) -> &str {
        &self.hash
    }

    /// The limits this platform cannot hold for a command, named in
    /// its record, or false when all of them hold.
    pub fn degraded(&self) -> serde_json::Value {
        match process::unenforced(&self.policy.limits) {
            [] => serde_json::Value::Bool(false),
            names => serde_json::json!(names),
        }
    }

    /// The absolute path `path` names from the working directory.
    pub fn absolute(&self, path: &str) -> String {
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

    pub fn file_info(
        &self,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        let at = self.reach(path, Reach::Read, Follow::NotLast)?;
        let stat = at.stat().map_err(|error| {
            file_error(&error, "lstat", &at.resolved)
        })?;
        info_of(path::basename(&at.resolved), &stat)
    }

    pub fn exists(&self, path: &str) -> Result<bool, FileError> {
        match self.file_info(path) {
            Ok(_) => Ok(true),
            Err(error) if error.code == Code::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The entries of a folder, but those neither file, folder nor
    /// link.
    pub fn list_dir(
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

    pub fn canonical_path(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        let at = self.reach(path, Reach::Read, Follow::Last)?;
        at.stat().map(|_| at.real.clone()).map_err(|error| {
            file_error(&error, "realpath", &at.resolved)
        })
    }

    /// The bytes of a regular file.
    pub fn read_file(
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
    pub fn open_file(
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
    pub fn read_text(&self, path: &str) -> Result<String, FileError> {
        let bytes = self.read_file(path)?;
        String::from_utf8(bytes).map_err(|_| {
            FileError::new(
                Code::Invalid,
                format!("{path} is not UTF-8 text"),
            )
        })
    }

    /// Writes `content` over the file, making its folders.
    pub fn write_file(
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

    /// Reads the file a change of `path` would replace: where the
    /// path leads, and the digest of the regular file there, if any,
    /// whose bytes go to `part` as they are read. Refused where the
    /// policy does not let the path be changed.
    pub fn read_for_change(
        &self,
        path: &str,
        part: &mut impl FnMut(&[u8]),
    ) -> Result<Current, FileError> {
        let at = self.reach(path, Reach::Write, Follow::Last)?;
        let digest = digest(&at, part)?;
        Ok(Current {
            real: at.real,
            digest,
        })
    }

    /// Replaces the file at `path` with `content` by way of the new
    /// file `temp` in its folder, renamed over it, so the file holds
    /// all of the old content or all of the new. Only while `path`
    /// still leads to `real` and the file's digest is `was` (`None`:
    /// no file); otherwise the change hears what it is now. A
    /// file a write in place could not open is refused in its words.
    pub fn replace_file(
        &self,
        path: &str,
        real: &str,
        temp: &str,
        was: Option<&[u8; 32]>,
        content: &[u8],
    ) -> Result<Replaced, FileError> {
        let mut at = self.reach(path, Reach::Write, Follow::Last)?;
        if at.real != real {
            return Err(FileError::new(
                Code::PermissionDenied,
                MOVED,
            ));
        }
        let now = digest(&at, &mut |_| ())?;
        if now.as_ref() != was {
            return Ok(Replaced::Changed(now));
        }
        if now.is_some() {
            at.file(Open::Append).map_err(|error| {
                file_error(&error, "open", &at.resolved)
            })?;
        }
        let temp = Name::new(temp).map_err(|_| {
            FileError::new(
                Code::Invalid,
                format!("{temp} is no name"),
            )
        })?;
        at.replace(&temp, content, NEW_FILE).map_err(|error| {
            file_error(&error, "open", &at.resolved)
        })?;
        Ok(Replaced::Done {
            created: now.is_none(),
        })
    }

    /// A new empty file for spilled output.
    pub fn create_temp_file(&self) -> Result<String, FileError> {
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
    /// to a file. What it leaves running goes to `leftovers`.
    pub async fn exec(
        &self,
        launch: Launch<'_>,
        timeout: Duration,
        secrets: &[String],
        leftovers: &Leftovers,
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ran, ExecError> {
        let mut spill = Spill::default();
        let streamed = self
            .stream(
                launch,
                timeout,
                secrets,
                leftovers,
                &mut |text| {
                    spill.push(self, text);
                    output(text);
                },
            )
            .await;
        let spilled = spill
            .finish()
            .map_err(|error| ExecError::Spill(error.message))?;
        match streamed {
            Ok(exit_code) => Ok(Ran {
                exit_code,
                spill: spilled,
            }),
            Err(ExecError::Timeout { .. }) => {
                Err(ExecError::Timeout { spill: spilled })
            }
            Err(error) => Err(error),
        }
    }

    /// [`Env::exec`], with the output, `secrets` taken out, only
    /// given to `output` as it comes: none of it is kept. Returns the
    /// exit code; a timeout spills nothing.
    pub async fn stream(
        &self,
        launch: Launch<'_>,
        timeout: Duration,
        secrets: &[String],
        leftovers: &Leftovers,
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<i32, ExecError> {
        let mut scrubber = Scrubber::new(secrets);
        let mut kept = |text: &str| {
            if !text.is_empty() {
                output(text);
            }
        };
        let ran = self
            .spawned(launch, timeout, leftovers, &mut |text| {
                kept(&scrubber.push(text));
            })
            .await;
        kept(&scrubber.flush());
        match ran? {
            Ended::Exited(status) => Ok(exit_code(&status)),
            Ended::TimedOut => {
                Err(ExecError::Timeout { spill: None })
            }
        }
    }

    async fn spawned(
        &self,
        launch: Launch<'_>,
        timeout: Duration,
        leftovers: &Leftovers,
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ended, ExecError> {
        if let Some(refusal) = self.root_refusal() {
            return Err(ExecError::Spawn(refusal.message));
        }
        let limits = &self.policy.limits;
        let wrapped = process::limited(limits, launch.argv());
        if let Some(program) =
            wrapped.first().filter(|word| !self.is_on_path(word))
        {
            return Err(ExecError::Spawn(format!(
                "{program}: command not found"
            )));
        }
        let confined = self.sandbox.command(
            &self.policy,
            &wrapped,
            &self.cwd,
            &self.tmp,
        );
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
        let (tree, stdout, stderr) = Tree::spawn(
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
        let mut tree = Some(tree);
        let mut stdout = Some(stdout);
        let mut stderr = Some(stderr);
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
                ended = ended(&mut tree), if status.is_none() => {
                    status = Some(ended);
                    grace = Instant::now() + EXIT_GRACE;
                    // A timed-out tree was killed whole, and is
                    // dropped at the end.
                    if !timed_out
                        && let Some(left) = tree.take().and_then(Tree::release)
                    {
                        leftovers.keep(left);
                    }
                }
                () = &mut deadline, if !timed_out && status.is_none() => {
                    timed_out = true;
                    if let Some(tree) = &mut tree {
                        tree.kill();
                    }
                }
                () = tokio::time::sleep_until(grace), if status.is_some() => {
                    break;
                }
            }
        }
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
        std::env::split_paths(dirs)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(|dir| path::resolve(&text(&dir), program))
            .any(|candidate| process::is_executable(&candidate))
    }
}

enum Ended {
    Exited(ExitStatus),
    TimedOut,
}

async fn ended(tree: &mut Option<Tree>) -> io::Result<ExitStatus> {
    match tree {
        Some(tree) => tree.ended().await,
        None => std::future::pending().await,
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
    let stat =
        hamlet_os::fs::stat(Path::new(&path::resolve(".", path)))?;
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
pub fn status_word(status: &ExitStatus) -> String {
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

pub fn errno_name(error: &io::Error) -> &'static str {
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use hamlet_testing::Scratch;
    use hamlet_testing::links::symlink;
    #[cfg(unix)]
    use hamlet_testing::names_in;

    use super::*;
    use crate::policy::host_env;

    /// The machine to read but `home`, and `folder` and `house` to
    /// write.
    fn open_policy(
        folder: &Path,
        house: &Path,
        home: &Path,
        shared: &[PathBuf],
    ) -> Policy {
        Policy {
            ro: shared.to_vec(),
            rw: vec![folder.to_owned(), house.to_owned()],
            open: true,
            hidden: vec![home.to_owned()],
            env: host_env(),
            ..Policy::default()
        }
    }

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
            Sandbox::testing(),
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
            Sandbox::testing(),
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
    fn a_root_in_a_folder_the_server_can_only_search_is_kept() {
        use std::os::unix::fs::PermissionsExt as _;
        let scratch = Scratch::new("env-search-only");
        let home = scratch.join("home");
        let house = home.join("houses/home/0");
        let folder = scratch.join("folder");
        let shared = scratch.join("parent/shared");
        for each in [&house, &folder, &shared] {
            fs::create_dir_all(each).unwrap();
        }
        let parent_mode = |mode| {
            let mode = fs::Permissions::from_mode(mode);
            fs::set_permissions(scratch.join("parent"), mode)
                .unwrap();
        };
        parent_mode(0o311);
        let written = Env::new(
            &text(&folder),
            open_policy(&folder, &house, &home, &[shared]),
            Sandbox::testing(),
        )
        .map(|env| env.write_file("file", "kept\n"));
        parent_mode(0o755);
        written.unwrap().unwrap();
        assert_eq!(
            fs::read_to_string(folder.join("file")).unwrap(),
            "kept\n"
        );
    }

    /// An environment that may change `folder`.
    #[cfg(unix)]
    fn changing(scratch: &Scratch) -> (PathBuf, Env) {
        let home = scratch.join("home");
        let house = home.join("houses/home/0");
        let folder = scratch.join("folder");
        fs::create_dir_all(&house).unwrap();
        fs::create_dir_all(&folder).unwrap();
        let env = Env::new(
            &text(&folder),
            open_policy(&folder, &house, &home, &[]),
            Sandbox::testing(),
        )
        .unwrap();
        (folder, env)
    }

    /// A change reads what it replaces and where the path leads, and
    /// replaces it, or makes it where nothing was.
    #[cfg(unix)]
    #[test]
    fn a_change_replaces_the_file_it_read() {
        let scratch = Scratch::new("env-change");
        let (folder, env) = changing(&scratch);
        fs::write(folder.join("f"), "old").unwrap();

        let mut old = Vec::new();
        let read = env
            .read_for_change("f", &mut |part| {
                old.extend_from_slice(part);
            })
            .unwrap();
        let replaced = env
            .replace_file(
                "f",
                &read.real,
                ".f.new",
                read.digest.as_ref(),
                b"new",
            )
            .unwrap();
        let fresh =
            env.read_for_change("sub/g", &mut |_| ()).unwrap();
        let made = env
            .replace_file(
                "sub/g",
                &fresh.real,
                ".g.new",
                None,
                b"made",
            )
            .unwrap();

        assert_eq!(read.real, text(&folder.join("f")));
        assert_eq!(old, b"old");
        assert_eq!(read.digest, Some(Sha256::digest(b"old").into()));
        assert_eq!(replaced, Replaced::Done { created: false });
        assert_eq!(fresh.real, text(&folder.join("sub/g")));
        assert_eq!(fresh.digest, None);
        assert_eq!(made, Replaced::Done { created: true });
        assert_eq!(
            fs::read_to_string(folder.join("f")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(folder.join("sub/g")).unwrap(),
            "made"
        );
        assert_eq!(names_in(&folder), ["f", "sub"]);
        assert_eq!(names_in(&folder.join("sub")), ["g"]);
    }

    /// A file that no longer holds what was read is not replaced, and
    /// the change hears what it holds now.
    #[cfg(unix)]
    #[test]
    fn a_change_of_a_file_that_changed_is_refused() {
        let scratch = Scratch::new("env-change-stale");
        let (folder, env) = changing(&scratch);
        fs::write(folder.join("f"), "old").unwrap();
        let read = env.read_for_change("f", &mut |_| ()).unwrap();
        let none = env.read_for_change("g", &mut |_| ()).unwrap();
        fs::write(folder.join("f"), "theirs").unwrap();
        fs::write(folder.join("g"), "made meanwhile").unwrap();

        let stale = env
            .replace_file(
                "f",
                &read.real,
                ".f.new",
                read.digest.as_ref(),
                b"new",
            )
            .unwrap();
        let made = env
            .replace_file("g", &none.real, ".g.new", None, b"new")
            .unwrap();

        assert_eq!(
            stale,
            Replaced::Changed(Some(Sha256::digest(b"theirs").into()))
        );
        assert_eq!(
            made,
            Replaced::Changed(Some(
                Sha256::digest(b"made meanwhile").into()
            ))
        );
        assert_eq!(
            fs::read_to_string(folder.join("f")).unwrap(),
            "theirs"
        );
        assert_eq!(names_in(&folder), ["f", "g"]);
    }

    /// A path that leads elsewhere than when it was read is not
    /// replaced, so the new file is never made in a folder the change
    /// did not name.
    #[cfg(unix)]
    #[test]
    fn a_change_whose_path_leads_elsewhere_is_refused() {
        let scratch = Scratch::new("env-change-moved");
        let (folder, env) = changing(&scratch);
        fs::create_dir_all(folder.join("a")).unwrap();
        fs::create_dir_all(folder.join("b")).unwrap();
        symlink("a", folder.join("l")).unwrap();
        let read = env.read_for_change("l/f", &mut |_| ()).unwrap();
        fs::remove_file(folder.join("l")).unwrap();
        symlink("b", folder.join("l")).unwrap();

        let moved = env
            .replace_file("l/f", &read.real, ".f.new", None, b"new")
            .unwrap_err();

        assert_eq!(moved.message, MOVED);
        assert!(names_in(&folder.join("a")).is_empty());
        assert!(names_in(&folder.join("b")).is_empty());
    }

    /// A change goes no further than a write in place would: a path
    /// out of the policy and a file the server may not write are
    /// refused in a write's words.
    #[cfg(unix)]
    #[test]
    fn a_change_is_refused_where_a_write_would_be() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new("env-change-refused");
        let (folder, env) = changing(&scratch);
        let locked = folder.join("locked");
        fs::write(&locked, "old").unwrap();
        fs::set_permissions(
            &locked,
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        let outside = text(&scratch.join("home/x"));

        let read =
            env.read_for_change("locked", &mut |_| ()).unwrap();
        let refused = env
            .replace_file(
                "locked",
                &read.real,
                ".l.new",
                read.digest.as_ref(),
                b"new",
            )
            .unwrap_err();
        let written = env.write_file("locked", "new").unwrap_err();
        let out =
            env.read_for_change(&outside, &mut |_| ()).unwrap_err();

        assert_eq!(refused.message, written.message);
        assert_eq!(refused.code, Code::PermissionDenied);
        assert_eq!(fs::read_to_string(&locked).unwrap(), "old");
        assert_eq!(names_in(&folder), ["locked"]);
        assert_eq!(
            out.message,
            format!(
                "{outside} is out of this agent's reach to change"
            )
        );
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
            Sandbox::testing(),
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

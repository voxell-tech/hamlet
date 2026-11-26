//! `cargo xtask`: builds, installs and checks Hamlet with Cargo
//! alone.
//!
//! It is a workspace of its own, so it builds before the vendored
//! fynix that the main workspace needs has been fetched.

mod idle;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};
use std::{env, fmt, fs, io};

const USAGE: &str =
    "\
Usage: cargo xtask <task> [args]

  build            build the CLI, server, window and the app's \
     launcher (release); args go to cargo build
  all              build every binary of the workspace (release); \
     args go to cargo build
  install          put the CLI, server and window in ~/.cargo/bin
  uninstall        remove all three from ~/.cargo/bin
  dev              try this checkout on a scratch home (HAMLET_HOME \
     picks it); --hot reloads saved shaders and tuning in the window
  check            every check a commit needs: the nine steps below, \
     in order
  format           check that Rust is formatted (fmt formats it)
  clippy           clippy with warnings as errors, on the workspace \
     and on xtask
  cross            clippy with warnings as errors on hamlet_os and \
     the native module for Node for Windows, which needs no C \
     compiler
  deny             cargo deny check: licences and advisories
  test             build every target, then cargo test
  acceptance       the acceptance suite, on the plain CLI and server
  actors           the acceptance suite on `hamlet_server --actors`, \
     with the Node host and its native module, but the scenarios \
     that fail there
  harness          the server's tests that start pi, built with \
     fault points
  doc              the documentation, with warnings as errors
  node             pi's TypeScript, Biome and tests, the native \
     module's declarations (Linux only), the Node host's \
     TypeScript, Biome and tests, then the native module for Node \
     (debug) and the Node host's tests that load it, the demo actor \
     host and the Node host's tests that drive it, and the server's \
     front door, linted and tested with the actors' Node host
  declarations     write the native module's index.d.ts and index.js \
     from its Rust, and fail if either changed
  fmt              format Rust, with the latest nightly, and pi
  workspace-image  build the remote workspace image; args go to \
     docker build
  killkit          build the demo actor host and the Node host's \
     native module, and kill the demo at a scenario's step: \
     [<scenario>] --runs <n>, every scenario when none is named, or \
     --list
  idle             a command tree's memory, in MB of 1,000,000 \
     bytes, and wakeups per second while idle: [--settle <s>] \
     [--window <s>] [--send <line>]... <command> [args], 30 and 60 \
     seconds by default, each line written to its stdin first; \
     Linux only

Every task but help and uninstall first fetches the vendored fynix
when it is missing. Tasks that build, run or check pi install its
packages when they are missing or stale, and node installs the Node
host's too. The installed server finds
pi in this checkout, so keep it where it was installed from, and run
`cargo xtask install` again after pulling.

deny needs cargo-deny: `cargo install --locked cargo-deny`. With CI
set, the steps pass --locked to cargo.";

/// This crate's manifest, outside the main workspace.
const XTASK: &str = "xtask/Cargo.toml";

/// The pi harness's Node package.
const PI: &str = "harnesses/pi";

/// The Node host's package, which holds the TypeScript actor layer.
const NODE_HOST: &str = "hosts/node";

/// The native module for Node, the crate the Node host loads.
const NATIVE: &str = "crates/hamlet_actor_node";

/// The cargo target folder the declarations are built in, apart
/// from `target`: the napi CLI's environment would make every plain
/// build there compile the native module again.
const NAPI_TYPES: &str = "target/napi-types";

const DECLARATIONS: [&str; 2] = ["index.d.ts", "index.js"];

/// The toolchain that formats Rust: `rustfmt.toml` uses unstable
/// options, so stable cannot.
const NIGHTLY: &str = "+nightly";

/// The Windows target `cross` checks, which `rust-toolchain.toml`
/// installs.
const WINDOWS: &str = "x86_64-pc-windows-gnu";

/// The command that installs the tool `deny` runs.
const INSTALL: &str = "cargo install --locked cargo-deny";

/// A step of `check`, also a task of its own under its name.
enum Step {
    Format,
    Clippy,
    Cross,
    Deny,
    Test,
    Acceptance,
    Harness,
    Doc,
    Node,
}

impl Step {
    /// The steps of `check`, in the order it runs them.
    const ALL: [Self; 9] = [
        Self::Format,
        Self::Clippy,
        Self::Cross,
        Self::Deny,
        Self::Test,
        Self::Acceptance,
        // The harness tests rebuild the server with fault points,
        // so they run after the acceptance suite, which needs the
        // plain one.
        Self::Harness,
        Self::Doc,
        Self::Node,
    ];

    /// The step named `name`, as CI calls it.
    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.to_string() == name)
    }

    /// Runs the step in `root`, the folder holding the workspace.
    fn run(self, root: &Path) -> Result<(), Failure> {
        match self {
            Self::Format => format(),
            Self::Clippy => clippy(),
            Self::Cross => cross(root),
            Self::Deny => deny(),
            Self::Test => test(),
            Self::Acceptance => acceptance(root),
            Self::Harness => harness(root),
            Self::Doc => doc(),
            Self::Node => node(root),
        }
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Format => "format",
            Self::Clippy => "clippy",
            Self::Cross => "cross",
            Self::Deny => "deny",
            Self::Test => "test",
            Self::Acceptance => "acceptance",
            Self::Harness => "harness",
            Self::Doc => "doc",
            Self::Node => "node",
        })
    }
}

/// A task's failure, as `main` prints it.
#[derive(Debug, thiserror::Error)]
enum Failure {
    #[error("xtask has no parent folder")]
    NoRoot,
    #[error("{0}")]
    EnterRoot(io::Error),
    #[error("no task `{0}`\n\n{USAGE}")]
    NoTask(String),
    #[error("no `{0}`; run `{INSTALL}`")]
    NoTool(&'static str),
    #[error("{command}: {error}")]
    Spawn { command: String, error: io::Error },
    #[error("{command} failed ({status})")]
    Exit { command: String, status: ExitStatus },
    #[error("idle: {0}")]
    Idle(String),
    #[error("cross: cannot write a placeholder libnode.dll: {0}")]
    Libnode(io::Error),
    #[error("node: cannot put the native module where it loads: {0}")]
    Native(io::Error),
    #[error("declarations: {0}")]
    Declarations(io::Error),
    #[error("declarations: napi build wrote no {0}")]
    NoDeclarations(&'static str),
    #[error(
        "{NATIVE}/{0} did not match the Rust: rewritten, commit the \
         change"
    )]
    StaleDeclarations(String),
}

/// The binaries a chief runs, with the packages that hold them.
const BINARIES: [(&str, &str); 3] = [
    ("hamlet_cli", "hamlet"),
    ("hamlet_server", "hamlet_server"),
    ("hamlet_app", "hamlet_app"),
];

/// What a release adds to `BINARIES`: the program its app opens,
/// which a source install, run from a terminal, has no use for.
const LAUNCHER: (&str, &str) = ("hamlet_cli", "hamlet_launch");

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let task = args.next().unwrap_or_default();
    let rest = args.collect::<Vec<_>>();
    match run(&task, &rest) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(task: &str, rest: &[String]) -> Result<(), Failure> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or(Failure::NoRoot)?
        .to_path_buf();
    env::set_current_dir(&root).map_err(Failure::EnterRoot)?;
    if !matches!(task, "" | "help" | "-h" | "--help" | "uninstall") {
        fynix(&root)?;
    }
    if let Some(step) = Step::parse(task) {
        return step.run(&root);
    }
    match task {
        "build" => {
            prepare(&root)?;
            let mut build =
                cargo(&["build", "--release", "--locked"]);
            for (package, binary) in
                BINARIES.into_iter().chain([LAUNCHER])
            {
                build.args(["-p", package, "--bin", binary]);
            }
            status(build.args(rest))
        }
        "all" => {
            prepare(&root)?;
            status(
                cargo(&[
                    "build",
                    "--release",
                    "--locked",
                    "--workspace",
                ])
                .arg("--bins")
                .args(rest),
            )
        }
        "install" => {
            prepare(&root)?;
            for (package, binary) in BINARIES {
                let path = Path::new("crates").join(package);
                status(
                    cargo(&[
                        "install", "--locked", "--force", "--path",
                    ])
                    .arg(path)
                    .args(["--bin", binary]),
                )?;
            }
            Ok(())
        }
        "uninstall" => {
            for (package, _) in BINARIES {
                // One that was never installed is no reason to stop.
                let _ = status(&mut cargo(&["uninstall", package]));
            }
            Ok(())
        }
        "dev" => {
            prepare(&root)?;
            status(
                cargo(&["run", "-p", "hamlet_cli", "--", "dev"])
                    .args(rest),
            )
        }
        "check" => {
            Step::ALL.into_iter().try_for_each(|step| step.run(&root))
        }
        "fmt" => {
            prepare(&root)?;
            status(&mut cargo(&[NIGHTLY, "fmt"]))?;
            status(&mut cargo(&[
                NIGHTLY,
                "fmt",
                "--manifest-path",
                XTASK,
            ]))?;
            status(&mut npm(PI, &["run", "format"]))
        }
        "declarations" => {
            install(&root, NODE_HOST)?;
            declarations(&root)
        }
        "workspace-image" => workspace_image(rest),
        "actors" => actors(&root),
        "killkit" => {
            install(&root, NODE_HOST)?;
            native(
                &root,
                "killkit",
                "the Node host's native module",
            )?;
            status(&mut cargo(&[
                "build",
                "-p",
                "hamlet_host",
                "--example",
                "demo",
            ]))?;
            status(
                cargo(&["run", "-q", "-p", "hamlet_killkit", "--"])
                    .args(rest),
            )
        }
        "idle" => {
            let options =
                idle::Options::parse(rest).map_err(Failure::Idle)?;
            let reading =
                idle::measure(&options).map_err(Failure::Idle)?;
            println!("{reading}");
            Ok(())
        }
        "" | "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(Failure::NoTask(other.to_owned())),
    }
}

fn format() -> Result<(), Failure> {
    status(&mut cargo(&[NIGHTLY, "fmt", "--check"]))?;
    status(&mut cargo(&[
        NIGHTLY,
        "fmt",
        "--check",
        "--manifest-path",
        XTASK,
    ]))
}

fn clippy() -> Result<(), Failure> {
    status(&mut locked(
        "clippy",
        &["--manifest-path", XTASK, "--", "-D", "warnings"],
    ))?;
    status(&mut locked(
        "clippy",
        &["--all-targets", "--", "-D", "warnings"],
    ))
}

/// Only `hamlet_os` and the test helpers it uses: the server's
/// dependencies compile C, which needs mingw-w64. Then the native
/// module for Node, which links nothing in a check: SQLite is named
/// from the system instead of compiled, and napi-build, which wants
/// Node's `libnode.dll` for a GNU target, gets an empty one.
fn cross(root: &Path) -> Result<(), Failure> {
    status(&mut locked(
        "clippy",
        &[
            "-p",
            "hamlet_os",
            "-p",
            "hamlet_testing",
            "--all-targets",
            "--target",
            WINDOWS,
            "--",
            "-D",
            "warnings",
        ],
    ))?;
    let libnode = root.join("target/cross-libnode");
    fs::create_dir_all(&libnode)
        .and_then(|()| fs::write(libnode.join("libnode.dll"), []))
        .map_err(Failure::Libnode)?;
    status(
        locked(
            "clippy",
            &[
                "-p",
                "hamlet_actor_node",
                "--all-targets",
                "--target",
                WINDOWS,
                "--",
                "-D",
                "warnings",
            ],
        )
        .env("LIBSQLITE3_SYS_USE_PKG_CONFIG", "1")
        .env("LIBNODE_PATH", &libnode),
    )
}

fn deny() -> Result<(), Failure> {
    if let Err(error) =
        Command::new("cargo-deny").arg("--version").output()
        && error.kind() == io::ErrorKind::NotFound
    {
        return Err(Failure::NoTool("cargo-deny"));
    }
    status(&mut locked("deny", &["check"]))
}

fn test() -> Result<(), Failure> {
    status(&mut locked("build", &["--all-targets"]))?;
    status(&mut locked("test", &[]))
}

fn acceptance(root: &Path) -> Result<(), Failure> {
    prepare(root)?;
    status(&mut locked(
        "build",
        &["-p", "hamlet_cli", "-p", "hamlet_server"],
    ))?;
    status(&mut locked(
        "clippy",
        &[
            "-p",
            "hamlet_acceptance",
            "--all-targets",
            "--features",
            "acceptance",
            "--",
            "-D",
            "warnings",
        ],
    ))?;
    status(&mut locked(
        "test",
        &["-p", "hamlet_acceptance", "--features", "acceptance"],
    ))
}

/// The scenarios `actors` skips, each with the place of its test:
/// they fail on `hamlet_server --actors`.
const ACTORS_SKIP: &[(&str, &str)] = &[
    (
        "chats_with_answers_on_stdout_and_the_rest_on_stderr",
        "tests/cli.rs:48",
    ),
    (
        "starts_at_another_hamlet_and_switches_agents_and_hamlets",
        "tests/cli.rs:149",
    ),
    (
        "withdrawing_an_objection_stops_the_turns_calls",
        "tests/oversight.rs:400",
    ),
    (
        "oauth_is_client_owned_and_prompts_cancel_on_disconnect_and_shutdown",
        "tests/oauth.rs:185",
    ),
    (
        "shutdown_waits_for_a_model_calls_started_rotation_to_persist",
        "tests/oauth.rs:266",
    ),
    (
        "concurrent_refresh_persists_after_disconnect_and_retired_tokens_never_reach_either_store",
        "tests/oauth.rs:298",
    ),
];

fn actors(root: &Path) -> Result<(), Failure> {
    prepare(root)?;
    install(root, NODE_HOST)?;
    if !native(root, "actors", "the acceptance suite on the actors")?
    {
        return Ok(());
    }
    status(&mut locked(
        "build",
        &["-p", "hamlet_cli", "-p", "hamlet_server"],
    ))?;
    let server = target(root)
        .join("debug")
        .join(format!("hamlet_server{}", env::consts::EXE_SUFFIX));
    let mut test = locked(
        "test",
        &[
            "-p",
            "hamlet_acceptance",
            "--features",
            "acceptance",
            "--no-fail-fast",
            "--",
            "--exact",
        ],
    );
    for (scenario, _) in ACTORS_SKIP {
        test.args(["--skip", scenario]);
    }
    status(test.env(
        "HAMLET_ACCEPTANCE_SERVER",
        format!("{} --actors", server.display()),
    ))
}

fn harness(root: &Path) -> Result<(), Failure> {
    prepare(root)?;
    status(&mut locked(
        "clippy",
        &[
            "-p",
            "hamlet_server",
            "--all-targets",
            "--features",
            "harness",
            "--",
            "-D",
            "warnings",
        ],
    ))?;
    status(&mut locked(
        "test",
        &["-p", "hamlet_server", "--features", "harness"],
    ))
}

/// `hamlet_actor_core`'s `native-runtime` and `wasm-runtime` features
/// exclude each other, so it is documented at its default features.
fn doc() -> Result<(), Failure> {
    status(
        locked(
            "doc",
            &[
                "--workspace",
                "--exclude",
                "hamlet_actor_core",
                "--all-features",
                "--no-deps",
                "--document-private-items",
            ],
        )
        .env("RUSTDOCFLAGS", "-D warnings"),
    )?;
    status(
        locked(
            "doc",
            &[
                "-p",
                "hamlet_actor_core",
                "--no-deps",
                "--document-private-items",
            ],
        )
        .env("RUSTDOCFLAGS", "-D warnings"),
    )
}

fn node(root: &Path) -> Result<(), Failure> {
    prepare(root)?;
    status(&mut npm(PI, &["run", "check"]))?;
    status(&mut npm(PI, &["test"]))?;
    install(root, NODE_HOST)?;
    if env::consts::OS == "linux" {
        declarations(root)?;
    } else {
        eprintln!(
            "xtask: node: skips the native module's declarations: \
             CI checks them on Linux"
        );
    }
    status(&mut npm(NODE_HOST, &["run", "check"]))?;
    status(&mut npm(NODE_HOST, &["test"]))?;
    if !native(root, "node", "the native module and its tests")? {
        return Ok(());
    }
    status(&mut locked(
        "build",
        &["-p", "hamlet_os", "--example", "bound"],
    ))?;
    status(&mut npm(
        NODE_HOST,
        &["run", "test:native", "--if-present"],
    ))?;
    status(&mut locked(
        "build",
        &["-p", "hamlet_host", "--example", "demo"],
    ))?;
    status(&mut npm(NODE_HOST, &["run", "test:link"]))?;
    front()
}

/// The server's front door: its lints and tests with the `node-host`
/// feature, whose tests start the actors' Node host from the packages
/// and the native module `node` puts in place.
fn front() -> Result<(), Failure> {
    let server = ["-p", "hamlet_server", "--features", "node-host"];
    status(&mut locked(
        "clippy",
        &[&server[..], &["--all-targets", "--", "-D", "warnings"]]
            .concat(),
    ))?;
    status(&mut locked(
        "test",
        &[&server[..], &["--test", "front"]].concat(),
    ))
}

fn unnamed(task: &str, what: &str) {
    eprintln!(
        "xtask: {task}: skips {what}: its loader has no name for \
         {}-{}",
        env::consts::OS,
        env::consts::ARCH,
    );
}

/// The native module's library as cargo builds it on this system, and
/// the name the module's `index.js` loads it by, or `None` where the
/// loader has no name this builds for.
fn native_names() -> Option<(&'static str, &'static str)> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Some((
            "libhamlet_actor_node.so",
            "actor-node.linux-x64-gnu.node",
        )),
        ("linux", "aarch64") => Some((
            "libhamlet_actor_node.so",
            "actor-node.linux-arm64-gnu.node",
        )),
        ("macos", "x86_64") => Some((
            "libhamlet_actor_node.dylib",
            "actor-node.darwin-x64.node",
        )),
        ("macos", "aarch64") => Some((
            "libhamlet_actor_node.dylib",
            "actor-node.darwin-arm64.node",
        )),
        ("windows", "x86_64") => Some((
            "hamlet_actor_node.dll",
            "actor-node.win32-x64-msvc.node",
        )),
        _ => None,
    }
}

/// Builds the native module and puts it in its crate under the name
/// its loader looks for, so the Node host loads this build. Answers
/// false, saying that `task` skips `what`, where the loader has no
/// name for this system.
fn native(
    root: &Path,
    task: &str,
    what: &str,
) -> Result<bool, Failure> {
    let Some((library, loaded)) = native_names() else {
        unnamed(task, what);
        return Ok(false);
    };
    status(&mut locked("build", &["-p", "hamlet_actor_node"]))?;
    let target = target(root);
    let folder = root.join(NATIVE);
    // A new file renamed over the old one, never the old one
    // rewritten: macOS kills a process that loads a signed library
    // changed in place. The partial name ends in .node, which git
    // ignores there.
    let partial = folder.join(format!("partial-{loaded}"));
    fs::copy(target.join("debug").join(library), &partial)
        .and_then(|_| fs::rename(&partial, folder.join(loaded)))
        .map_err(Failure::Native)?;
    Ok(true)
}

/// Cargo's target folder for the workspace in `root`.
fn target(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), PathBuf::from)
}

fn declarations(root: &Path) -> Result<(), Failure> {
    let Some((_, loaded)) = native_names() else {
        unnamed("declarations", "the native module's declarations");
        return Ok(());
    };
    let target = root.join(NAPI_TYPES);
    // napi-derive writes the type file the CLI reads only while the
    // crate compiles, and the CLI writes nothing without it.
    status(
        locked("clean", &["-p", "hamlet_actor_node", "--target-dir"])
            .arg(&target),
    )?;
    let written = target.join("declarations");
    if written.exists() {
        fs::remove_dir_all(&written)
            .map_err(Failure::Declarations)?;
    }
    let tools = root.join(NODE_HOST).join("node_modules");
    let mut napi = Command::new("node");
    napi.arg(tools.join("@napi-rs/cli/scripts/index.js"))
        .args(["build", "--platform", "--dts", "index.d.ts"])
        .args(["--js", "index.js"])
        // The CLI joins this folder to its working directory even
        // when it is absolute.
        .arg(Path::new("../..").join(NAPI_TYPES).join("declarations"))
        .current_dir(root.join(NATIVE))
        .env("CARGO_TARGET_DIR", &target);
    if ci() {
        napi.arg("--cargo-flags=--locked");
    }
    status(&mut napi)?;
    fs::remove_file(written.join(loaded))
        .map_err(Failure::Declarations)?;
    for file in DECLARATIONS {
        if !written.join(file).exists() {
            return Err(Failure::NoDeclarations(file));
        }
    }
    status(
        Command::new("node")
            .arg(tools.join("@biomejs/biome/bin/biome"))
            .args(["format", "--write", "--config-path", NATIVE])
            .args(DECLARATIONS.map(|file| written.join(file))),
    )?;
    let mut stale = Vec::new();
    for file in DECLARATIONS {
        let body = fs::read_to_string(written.join(file))
            .map_err(Failure::Declarations)?;
        let text = format!(
            "// This file was changed for Hamlet. See NOTICE in \
             this crate.\n{body}"
        );
        let committed = root.join(NATIVE).join(file);
        if fs::read_to_string(&committed).ok().as_deref()
            != Some(&text)
        {
            fs::write(&committed, text)
                .map_err(Failure::Declarations)?;
            stale.push(file);
        }
    }
    match stale.as_slice() {
        [] => Ok(()),
        [file] => Err(Failure::StaleDeclarations((*file).to_owned())),
        files => Err(Failure::StaleDeclarations(format!(
            "{{{}}}",
            files.join(",")
        ))),
    }
}

/// Builds the remote workspace image, tagged with the short commit
/// unless `rest` names a tag, and labelled with the full one, so a
/// deployed image tells which commit it holds.
fn workspace_image(rest: &[String]) -> Result<(), Failure> {
    let commit =
        output(Command::new("git").args(["rev-parse", "HEAD"]))?;
    let mut build = Command::new("docker");
    build
        .args(["build", "-f", "crates/hamlet_workspace/Dockerfile"])
        .arg("--label")
        .arg(format!("org.opencontainers.image.revision={commit}"));
    if !rest
        .iter()
        .any(|arg| arg == "-t" || arg.starts_with("--tag"))
    {
        let short = &commit[..commit.len().min(7)];
        build.args(["-t", &format!("hamlet_workspace:{short}")]);
    }
    status(build.args(rest).arg("."))
}

/// Fetches the vendored fynix, a submodule, when it is missing: every
/// cargo command on the workspace needs its manifest.
fn fynix(root: &Path) -> Result<(), Failure> {
    if root.join("vendor/fynix/Cargo.toml").exists() {
        return Ok(());
    }
    status(Command::new("git").args([
        "submodule",
        "update",
        "--init",
        "vendor/fynix",
    ]))
}

/// Installs pi's packages when they are missing or stale.
fn prepare(root: &Path) -> Result<(), Failure> {
    install(root, PI)
}

/// Installs the packages of the Node package in `package` when they
/// are missing or stale.
fn install(root: &Path, package: &str) -> Result<(), Failure> {
    let folder = root.join(package);
    // npm ci rewrites this file, so it is newer than the lockfile it
    // installed from.
    let installed =
        modified(&folder.join("node_modules/.package-lock.json"));
    let wanted = ["package.json", "package-lock.json"]
        .iter()
        .filter_map(|file| modified(&folder.join(file)))
        .max();
    if installed.is_none() || installed < wanted {
        status(&mut npm(package, &["ci"]))?;
    }
    Ok(())
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// Cargo through rustup's proxy on `PATH`, not the toolchain's own
/// binary in `CARGO`, so that a `+toolchain` argument works.
fn cargo(args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command.args(args);
    command
}

/// [`cargo`] running `subcommand`, with `--locked` when `CI` is set,
/// so CI fails on a stale `Cargo.lock` that a local run updates.
fn locked(subcommand: &str, args: &[&str]) -> Command {
    let mut command = cargo(&[subcommand]);
    if ci() {
        command.arg("--locked");
    }
    command.args(args);
    command
}

fn ci() -> bool {
    env::var_os("CI").is_some()
}

/// npm running `args` on the Node package in `package`.
fn npm(package: &str, args: &[&str]) -> Command {
    let mut command = Command::new("npm");
    command.args(args).args(["--prefix", package]);
    command
}

fn status(command: &mut Command) -> Result<(), Failure> {
    let shown = shown(command);
    eprintln!("$ {shown}");
    let status =
        command.status().map_err(|error| Failure::Spawn {
            command: shown.clone(),
            error,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Failure::Exit {
            command: shown,
            status,
        })
    }
}

fn output(command: &mut Command) -> Result<String, Failure> {
    let shown = shown(command);
    let output =
        command.output().map_err(|error| Failure::Spawn {
            command: shown.clone(),
            error,
        })?;
    if !output.status.success() {
        return Err(Failure::Exit {
            command: shown,
            status: output.status,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn shown(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

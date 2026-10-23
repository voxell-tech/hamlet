//! `cargo xtask`: builds, installs and checks Hamlet with Cargo
//! alone.
//!
//! It is a workspace of its own, so it builds before the main
//! workspace's dependencies are fetched.

use std::path::Path;
use std::process::{Command, ExitCode};
use std::{env, fs, io};

const USAGE: &str = "\
Usage: cargo xtask <task> [args]

  build            build the CLI, server and window (release); args \
                     go to cargo build
  all              build every binary of the workspace (release); \
                     args go to cargo build
  install          put the CLI, server and window in ~/.cargo/bin
  uninstall        remove all three from ~/.cargo/bin
  dev              try this checkout on a scratch home (HAMLET_HOME \
                     picks it)
  check            every check a commit needs: the six steps below, \
                     in order
  lints            formatting, clippy, cargo deny and typos
  test             build every target, then cargo test
  acceptance       the acceptance suite, on the plain CLI and server
  harness          the server's tests that start pi, built with \
                     fault points
  doc              the documentation, with warnings as errors
  node             pi's TypeScript, Biome and tests
  fmt              format Rust, with the nightly named in \
                     rust-nightly, and pi
  workspace-image  build the remote workspace image; args go to \
                     docker build

Tasks that build, run or check pi first install its packages when
they are missing or stale. The installed server finds pi in this
checkout, so keep it where it was installed from, and run `cargo
xtask install` again after pulling.

lints needs cargo-deny and typos: `cargo install --locked cargo-deny
typos-cli`. With CI set, the steps pass --locked to cargo.";

/// This crate's manifest, outside the main workspace.
const XTASK: &str = "xtask/Cargo.toml";

/// The file naming the nightly that formats Rust, which CI reads too.
const NIGHTLY: &str = "rust-nightly";

/// The command that installs the tools `lints` runs.
const INSTALL: &str = "cargo install --locked cargo-deny typos-cli";

/// A step of `check`, run in the folder holding the workspace.
type Step = fn(&Path) -> Result<(), String>;

/// The steps of `check`, in the order it runs them.
const STEPS: [(&str, Step); 6] = [
    ("lints", lints),
    ("test", test),
    ("acceptance", acceptance),
    // The harness tests rebuild the server with fault points, so
    // they run after the acceptance suite, which needs the plain
    // one.
    ("harness", harness),
    ("doc", doc),
    ("node", node),
];

/// The binaries a chief runs, with the packages that hold them.
const BINARIES: [(&str, &str); 3] = [
    ("hamlet_cli", "hamlet"),
    ("hamlet_server", "hamlet-server"),
    ("hamlet_app", "hamlet-app"),
];

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

fn run(task: &str, rest: &[String]) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?
        .to_path_buf();
    env::set_current_dir(&root).map_err(|error| error.to_string())?;
    if let Some((_, step)) =
        STEPS.iter().find(|(name, _)| *name == task)
    {
        return step(&root);
    }
    match task {
        "build" => {
            prepare(&root)?;
            let mut build =
                cargo(&["build", "--release", "--locked"]);
            for (package, binary) in BINARIES {
                build.args(["-p", package, "--bin", binary]);
            }
            status(build.args(rest))
        }
        "all" => {
            prepare(&root)?;
            // hamlet-harness-driver is left out: it needs the harness
            // feature, and the harness tests build it.
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
        "check" => STEPS.iter().try_for_each(|(_, step)| step(&root)),
        "fmt" => {
            prepare(&root)?;
            let nightly = nightly(&root)?;
            status(&mut cargo(&[&nightly, "fmt"]))?;
            status(&mut cargo(&[
                &nightly,
                "fmt",
                "--manifest-path",
                XTASK,
            ]))?;
            status(&mut npm(&["run", "format"]))
        }
        "workspace-image" => workspace_image(rest),
        "" | "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("no task `{other}`\n\n{USAGE}")),
    }
}

fn lints(root: &Path) -> Result<(), String> {
    for tool in ["cargo-deny", "typos"] {
        if let Err(error) =
            Command::new(tool).arg("--version").output()
            && error.kind() == io::ErrorKind::NotFound
        {
            return Err(format!("no `{tool}`; run `{INSTALL}`"));
        }
    }
    let nightly = nightly(root)?;
    status(&mut cargo(&[&nightly, "fmt", "--check"]))?;
    status(&mut cargo(&[
        &nightly,
        "fmt",
        "--check",
        "--manifest-path",
        XTASK,
    ]))?;
    status(&mut locked(
        "clippy",
        &["--manifest-path", XTASK, "--", "-D", "warnings"],
    ))?;
    status(&mut locked(
        "clippy",
        &["--all-targets", "--", "-D", "warnings"],
    ))?;
    status(&mut locked("deny", &["check"]))?;
    status(&mut Command::new("typos"))
}

fn test(_: &Path) -> Result<(), String> {
    status(&mut locked("build", &["--all-targets"]))?;
    status(&mut locked("test", &[]))
}

fn acceptance(root: &Path) -> Result<(), String> {
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

fn harness(root: &Path) -> Result<(), String> {
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

fn doc(_: &Path) -> Result<(), String> {
    status(
        locked(
            "doc",
            &[
                "--workspace",
                "--all-features",
                "--no-deps",
                "--document-private-items",
            ],
        )
        .env("RUSTDOCFLAGS", "-D warnings"),
    )
}

fn node(root: &Path) -> Result<(), String> {
    prepare(root)?;
    status(&mut npm(&["run", "check"]))?;
    status(&mut npm(&["test"]))
}

/// Builds the remote workspace image, tagged with the short commit
/// unless `rest` names a tag, and labelled with the full one, so a
/// deployed image tells which commit it holds.
fn workspace_image(rest: &[String]) -> Result<(), String> {
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
        build.args(["-t", &format!("hamlet-workspace:{short}")]);
    }
    status(build.args(rest).arg("."))
}

/// Installs pi's packages when they are missing or stale.
fn prepare(root: &Path) -> Result<(), String> {
    let pi = root.join("harnesses/pi");
    // npm ci rewrites this file, so it is newer than the lockfile it
    // installed from.
    let installed =
        modified(&pi.join("node_modules/.package-lock.json"));
    let wanted = ["package.json", "package-lock.json"]
        .iter()
        .filter_map(|file| modified(&pi.join(file)))
        .max();
    if installed.is_none() || installed < wanted {
        status(&mut npm(&["ci"]))?;
    }
    Ok(())
}

/// `+` and the toolchain [`NIGHTLY`] names, for `cargo fmt`.
fn nightly(root: &Path) -> Result<String, String> {
    let name = fs::read_to_string(root.join(NIGHTLY))
        .map_err(|error| format!("{NIGHTLY}: {error}"))?;
    Ok(format!("+{}", name.trim()))
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
    if env::var_os("CI").is_some() {
        command.arg("--locked");
    }
    command.args(args);
    command
}

fn npm(args: &[&str]) -> Command {
    let mut command = Command::new("npm");
    command.args(args).args(["--prefix", "harnesses/pi"]);
    command
}

fn status(command: &mut Command) -> Result<(), String> {
    let shown = shown(command);
    eprintln!("$ {shown}");
    let status = command
        .status()
        .map_err(|error| format!("{shown}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} failed ({status})"))
    }
}

fn output(command: &mut Command) -> Result<String, String> {
    let shown = shown(command);
    let output = command
        .output()
        .map_err(|error| format!("{shown}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{shown} failed ({})", output.status));
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

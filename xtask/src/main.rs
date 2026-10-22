//! `cargo xtask`: builds, installs and checks Hamlet with Cargo
//! alone.
//!
//! It is a workspace of its own, so it builds before the main
//! workspace's dependencies are fetched.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs};

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
  check            every check a commit needs
  fmt              format Rust, with the nightly named in \
                     rust-nightly, and pi
  workspace-image  build the remote workspace image; args go to \
                     docker build

Every task first installs pi's packages when they are missing or
stale. The installed server finds pi in this checkout, so keep it
where it was installed from, and run `cargo xtask install` again
after pulling.";

/// This crate's manifest, outside the main workspace.
const XTASK: &str = "xtask/Cargo.toml";

/// The file naming the nightly that formats Rust, which CI reads too.
const NIGHTLY: &str = "rust-nightly";

/// The binaries a chief runs, with the packages that hold them.
const BINARIES: [(&str, &str); 3] = [
    ("hamlet_cli", "hamlet"),
    ("hamlet_server", "hamlet-server"),
    ("hamlet", "hamlet-app"),
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
                let path = if package == "hamlet" {
                    PathBuf::from(".")
                } else {
                    Path::new("crates").join(package)
                };
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
            prepare(&root)?;
            check(&root)
        }
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

fn check(root: &Path) -> Result<(), String> {
    let nightly = nightly(root)?;
    let steps: [&[&str]; 10] = [
        &[&nightly, "fmt", "--check"],
        &[&nightly, "fmt", "--check", "--manifest-path", XTASK],
        &["clippy", "--manifest-path", XTASK, "--", "-D", "warnings"],
        &["build", "--all-targets"],
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        &["test"],
        &[
            "clippy",
            "-p",
            "hamlet_acceptance",
            "--all-targets",
            "--features",
            "acceptance",
            "--",
            "-D",
            "warnings",
        ],
        &[
            "test",
            "-p",
            "hamlet_acceptance",
            "--features",
            "acceptance",
        ],
        &[
            "clippy",
            "-p",
            "hamlet_server",
            "--all-targets",
            "--features",
            "harness",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "-p", "hamlet_server", "--features", "harness"],
    ];
    for step in steps {
        status(&mut cargo(step))?;
    }
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

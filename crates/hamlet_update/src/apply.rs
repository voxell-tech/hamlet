//! Putting a downloaded update in place, the way its platform
//! installs Hamlet.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::Install;

/// What became of an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// It is in place: the programs at the old paths are the new
    /// ones.
    Installed,
    /// An installer took it over, and opens Hamlet when it is done.
    Handed,
}

/// Puts the update in `file` in place over `install`. The server must
/// already be stopped.
pub fn apply(file: &Path, install: &Install) -> Result<Applied> {
    match install {
        Install::App(bundle) => app(file, bundle),
        Install::Setup => setup(file),
        Install::Deb => package(&["apt-get", "install", "-y"], file),
        Install::Rpm => package(&["dnf", "install", "-y"], file),
        Install::Manual => {
            bail!(
                "This copy of Hamlet was not installed by an \
                 installer."
            )
        }
    }
}

/// Runs `command`, refusing when it does not succeed.
fn run(command: &mut Command) -> Result<()> {
    let program =
        command.get_program().to_string_lossy().into_owned();
    let output = command
        .output()
        .with_context(|| format!("Cannot run {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// The new app from the zip `file` takes the place of `bundle`, once
/// macOS finds its signature whole and, when the old one was signed
/// by a developer, signed by the same one.
fn app(file: &Path, bundle: &Path) -> Result<Applied> {
    let parent = bundle.parent().context("The app has no folder")?;
    // Beside the app, so the new one is moved in, never copied.
    let work = parent.join(".hamlet-update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir(&work).with_context(|| {
        format!(
            "Hamlet cannot write in {}, so it cannot replace itself",
            parent.display()
        )
    })?;
    let replaced = replace(file, bundle, &work);
    let _ = std::fs::remove_dir_all(&work);
    replaced.map(|()| Applied::Installed)
}

/// Unpacks `file` in `work`, checks its signature against the
/// bundle's, and swaps it in for `bundle`.
fn replace(file: &Path, bundle: &Path, work: &Path) -> Result<()> {
    run(Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(file)
        .arg(work))?;
    let new = std::fs::read_dir(work)?
        .filter_map(|entry| {
            Some(entry.ok()?.path().join("Hamlet.app"))
        })
        .find(|app| app.is_dir())
        .context("The update holds no Hamlet.app")?;
    run(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&new))?;
    if let Some(signer) = team(bundle)
        && team(&new).as_ref() != Some(&signer)
    {
        bail!(
            "The update is not signed by {signer}, who signed Hamlet"
        );
    }
    let old = work.join("old.app");
    std::fs::rename(bundle, &old)?;
    if let Err(error) = std::fs::rename(&new, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(error)
            .context("Cannot move the new app in place");
    }
    Ok(())
}

/// The developer team that signed `app`, if one did.
fn team(app: &Path) -> Option<String> {
    let output = Command::new("codesign")
        .args(["--display", "--verbose=2"])
        .arg(app)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_owned)
}

/// Hands the installer `file` the update, silently. It closes what
/// holds Hamlet's files open, and `RELAUNCH` has it open Hamlet once
/// it is done (`hamlet.iss`).
fn setup(file: &Path) -> Result<Applied> {
    Command::new(file)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/RELAUNCH=yes",
        ])
        .spawn()
        .context("Cannot run the installer")?;
    Ok(Applied::Handed)
}

/// Installs the package `file` with `tool`, as root through pkexec,
/// which asks for the password in a window of the desktop's.
fn package(tool: &[&str], file: &Path) -> Result<Applied> {
    let status = Command::new("pkexec")
        .args(tool)
        .arg(file)
        .status()
        .context(
            "Hamlet updates through pkexec, which is not installed",
        )?;
    match status.code() {
        Some(0) => Ok(Applied::Installed),
        // pkexec's codes for a password not given and one refused.
        Some(126 | 127) => {
            bail!("The update was not installed: it was not allowed.")
        }
        _ => bail!(
            "The update was not installed: {} {status}.",
            tool[0]
        ),
    }
}

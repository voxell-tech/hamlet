//! What a packaged Hamlet runs when it is opened from a menu, the
//! Finder or a shortcut: the home's server, started unless it serves,
//! then the window on it. Opened that way it has no terminal, so a
//! failure is shown in a dialog too. Given `--update <file>`, it
//! first stops the server and puts that update in place
//! (docs/releases.md#updates).

use std::path::{Path, PathBuf};

use anyhow::Result;
use hamlet_update::{Applied, Install};
use rfd::{MessageButtons, MessageDialog, MessageLevel};
use tokio::runtime::Runtime;

use crate::process::{self, Isolation, NotReady};

/// The longest text a dialog shows; the rest stays in the log.
const LONGEST: usize = 1500;

/// Opens Hamlet, and gives the process's exit code.
pub fn run() -> i32 {
    let mut args = std::env::args_os().skip(1);
    let update = match (args.next(), args.next(), args.next()) {
        (None, ..) => None,
        (Some(flag), Some(file), None) if flag == "--update" => {
            Some(PathBuf::from(file))
        }
        _ => {
            eprintln!("usage: hamlet_launch [--update <file>]");
            return 2;
        }
    };
    let opened = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| {
            if let Some(file) = update {
                match self::update(&runtime, &file) {
                    Ok(Applied::Handed) => return Ok(()),
                    Ok(Applied::Installed) => {}
                    // The copy that is there opens all the same.
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        show("Hamlet could not update", &error);
                    }
                }
            }
            open(runtime)
        });
    match opened {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Error: {error:#}");
            show("Hamlet could not start", &error);
            1
        }
    }
}

/// Stops the home's server, which holds Hamlet's programs open, and
/// puts the update in `file` in place.
fn update(runtime: &Runtime, file: &Path) -> Result<Applied> {
    runtime.block_on(process::stop_home())?;
    hamlet_update::apply(file, &Install::here())
}

/// Starts the home's server unless it serves, and opens the window on
/// it.
fn open(runtime: Runtime) -> Result<()> {
    let address =
        runtime.block_on(process::ensure(Isolation::Sandboxed));
    runtime.shutdown_background();
    let address = address?;
    // On Unix the window takes this process's place, so the Dock and
    // the desktop's launcher see the app they opened become the
    // window.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut window = process::window_for(&address)?;
        Err(anyhow::Error::from(window.exec())
            .context("Cannot open the window"))
    }
    #[cfg(windows)]
    process::open_detached(&address)
}

/// Tells the chief what went wrong, in the system's own dialog.
fn show(title: &str, error: &anyhow::Error) {
    let text = match error.downcast_ref::<NotReady>() {
        Some(NotReady {
            reason: Some(reason),
            log,
            ..
        }) => format!(
            "{reason}\n\nWhat the server wrote is in {}.",
            log.display()
        ),
        _ => format!("{error:#}"),
    };
    let text = match text.char_indices().nth(LONGEST) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    };
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title(title)
        .set_description(text)
        .set_buttons(MessageButtons::Ok)
        .show();
}

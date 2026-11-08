//! What a packaged Hamlet runs when it is opened from a menu, the
//! Finder or a shortcut: the home's server, started unless it serves,
//! then the window on it. Opened that way it has no terminal, so a
//! failure is shown in a dialog too.

use anyhow::Result;
use rfd::{MessageButtons, MessageDialog, MessageLevel};

use crate::process::{self, Isolation, NotReady};

/// The longest text a dialog shows; the rest stays in the log.
const LONGEST: usize = 1500;

/// Opens Hamlet, and gives the process's exit code.
pub fn run() -> i32 {
    match open() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Error: {error:#}");
            show(&error);
            1
        }
    }
}

fn open() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
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

/// Tells the chief what stopped Hamlet, in the system's own dialog.
fn show(error: &anyhow::Error) {
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
        .set_title("Hamlet could not start")
        .set_description(text)
        .set_buttons(MessageButtons::Ok)
        .show();
}

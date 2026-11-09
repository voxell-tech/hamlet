//! Where the web engine, CEF, is on this machine. The window and its
//! helper both read this file, so they agree on it.

use std::path::PathBuf;

/// The version of CEF the window is built for, as CEF's own builds
/// name it, such as `154.0.34`.
pub fn version() -> String {
    let full = String::from_utf8_lossy(cef::sys::CEF_VERSION);
    full.split(['+', '\0'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The folder CEF of this [`version`] is fetched into, under the
/// user's caches.
pub fn fetched() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/Caches/Hamlet/cef")
            .join(version()),
    )
}

/// The folder of CEF's framework as fetched, by the machine's kind.
pub fn fetched_framework() -> Option<PathBuf> {
    let kind = format!("cef_macos_{}", std::env::consts::ARCH);
    Some(fetched()?.join(kind))
}

/// Where CEF is: where `CEF_PATH` or `CEF_FRAMEWORK_DIR` says, else
/// where it was fetched, once it has been.
pub fn located() -> Option<PathBuf> {
    std::env::var_os("CEF_FRAMEWORK_DIR")
        .or_else(|| std::env::var_os("CEF_PATH"))
        .map(PathBuf::from)
        .or_else(|| {
            fetched_framework().filter(|framework| {
                framework
                    .join("Chromium Embedded Framework.framework")
                    .exists()
            })
        })
}

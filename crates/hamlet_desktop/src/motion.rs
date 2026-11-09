//! Whether the computer asks for less motion.

/// Whether the computer's accessibility settings ask for less
/// motion: Reduce motion on macOS, animations turned off on GNOME.
/// False where it does not say.
pub fn reduced() -> bool {
    #[cfg(target_os = "macos")]
    return said(
        "defaults",
        &["read", "com.apple.universalaccess", "reduceMotion"],
    )
    .is_some_and(|said| said == "1");
    #[cfg(target_os = "linux")]
    return said(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    )
    .is_some_and(|said| said == "false");
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    false
}

/// What `program` run with `args` prints, trimmed; none when it
/// cannot run or fails.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn said(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

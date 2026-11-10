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

/// How long `program` may take to answer before it is stopped; the
/// window waits for it before it opens.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const PATIENCE: std::time::Duration =
    std::time::Duration::from_secs(2);

/// What `program` run with `args` prints, trimmed; none when it
/// cannot run, fails or takes longer than [`PATIENCE`].
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn said(program: &str, args: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + PATIENCE;
    let pause = std::time::Duration::from_millis(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(pause);
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out.trim().to_string())
}

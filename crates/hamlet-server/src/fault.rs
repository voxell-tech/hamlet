//! Points where a test stops this server, so it can be killed there.
//! They do nothing unless the `faults` feature is on.

/// Stops for good at `point` when `HAMLET_FAULT` names it, once per
/// `HAMLET_FAULT_DIR`: the stop writes `server-paused` there, and a
/// server started after it passes the point.
#[cfg(feature = "faults")]
pub(crate) fn point(point: &str) {
    let (Ok(fault), Some(dir)) = (
        std::env::var("HAMLET_FAULT"),
        std::env::var_os("HAMLET_FAULT_DIR"),
    ) else {
        return;
    };
    let paused = std::path::Path::new(&dir).join("server-paused");
    if fault != point || paused.exists() {
        return;
    }
    if std::fs::write(&paused, point).is_ok() {
        loop {
            std::thread::park();
        }
    }
}

#[cfg(not(feature = "faults"))]
pub(crate) fn point(_point: &str) {}

/// Whether `HAMLET_FAULT_SPLIT_MARKER` asks for a change and its
/// marker committed apart.
#[cfg(feature = "faults")]
pub(crate) fn split_marker() -> bool {
    std::env::var_os("HAMLET_FAULT_SPLIT_MARKER").is_some()
}

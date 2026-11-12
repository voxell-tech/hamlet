//! Points where a kill test stops the host, so it can be killed
//! there. They do nothing unless the `faults` feature is on, which
//! the crate's own tests and examples turn on.

/// Stops the calling thread for good at `point` when `HAMLET_FAULT`
/// names it, once per `HAMLET_FAULT_DIR`. `HAMLET_FAULT` is `<point>`
/// to stop at the first pass, or `<point>:<pass>` to stop at that
/// pass, counted from 1. The stop writes `host-paused` in
/// `HAMLET_FAULT_DIR`, holding the point and the process id, and a
/// host started after it passes the point.
#[cfg(feature = "faults")]
pub fn point(point: &str) {
    use std::sync::atomic::{AtomicU32, Ordering};

    static PASSES: AtomicU32 = AtomicU32::new(0);
    let (Ok(fault), Some(dir)) = (
        std::env::var("HAMLET_FAULT"),
        std::env::var_os("HAMLET_FAULT_DIR"),
    ) else {
        return;
    };
    let (name, pass) = match fault.split_once(':') {
        Some((name, pass)) => (name, pass.parse().unwrap_or(1)),
        None => (fault.as_str(), 1),
    };
    if name != point
        || PASSES.fetch_add(1, Ordering::SeqCst) + 1 != pass
    {
        return;
    }
    let dir = std::path::Path::new(&dir);
    let marker = dir.join("host-paused");
    if marker.exists() {
        return;
    }
    // Renamed into place, so a reader never sees half a marker.
    let partial = dir.join("host-paused.partial");
    let text = format!("{point} {}\n", std::process::id());
    if std::fs::write(&partial, text).is_ok()
        && std::fs::rename(&partial, &marker).is_ok()
    {
        loop {
            std::thread::park();
        }
    }
}

#[cfg(not(feature = "faults"))]
pub fn point(_point: &str) {}

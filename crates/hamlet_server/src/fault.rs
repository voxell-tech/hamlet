//! Points where a test stops this server, so it can be killed there,
//! or makes it panic. They do nothing unless the `faults` feature is
//! on.

#[cfg(all(feature = "faults", not(debug_assertions)))]
compile_error!(
    "The `faults` feature is for tests: it stops the server at \
     fault points. A release build must not carry it."
);

/// Stops for good at `point` when `HAMLET_FAULT` names it, once per
/// `HAMLET_FAULT_DIR`: the stop writes `server-paused` there, and a
/// server started after it passes the point.
#[cfg(feature = "faults")]
pub(crate) fn point(point: &str) {
    if let Pass::Faults = mark(point, "server-paused") {
        loop {
            std::thread::park();
        }
    }
}

#[cfg(not(feature = "faults"))]
pub(crate) fn point(_point: &str) {}

/// Panics at `point` when `HAMLET_FAULT` names it, once per
/// `HAMLET_FAULT_DIR`: the panic writes `server-panicked` there, and
/// the next pass of the point goes on.
#[cfg(feature = "faults")]
pub(crate) fn panic_at(point: &str) {
    if let Pass::Faults = mark(point, "server-panicked") {
        panic!("the fault point {point} panics");
    }
}

#[cfg(not(feature = "faults"))]
pub(crate) fn panic_at(_point: &str) {}

/// A fault point's course on this pass.
#[cfg(feature = "faults")]
enum Pass {
    /// It stops or panics, its marker written.
    Faults,
    /// It goes on.
    Through,
}

/// Writes `marker` in `HAMLET_FAULT_DIR` when `HAMLET_FAULT` names
/// `point` and the marker is not there yet, and the point faults
/// when it did.
#[cfg(feature = "faults")]
fn mark(point: &str, marker: &str) -> Pass {
    let (Ok(fault), Some(dir)) = (
        std::env::var("HAMLET_FAULT"),
        std::env::var_os("HAMLET_FAULT_DIR"),
    ) else {
        return Pass::Through;
    };
    let marker = std::path::Path::new(&dir).join(marker);
    if fault == point
        && !marker.exists()
        && std::fs::write(&marker, point).is_ok()
    {
        Pass::Faults
    } else {
        Pass::Through
    }
}

/// Whether `HAMLET_FAULT_SPLIT_MARKER` asks for a change and its
/// marker committed apart.
#[cfg(feature = "faults")]
pub(crate) fn is_marker_split() -> bool {
    std::env::var_os("HAMLET_FAULT_SPLIT_MARKER").is_some()
}

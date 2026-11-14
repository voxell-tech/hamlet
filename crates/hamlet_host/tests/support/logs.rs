//! What is logged on the thread that records it.

use std::sync::Arc;

use parking_lot::Mutex;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// The lines logged while [`Logs::record`]'s guard lives, each span
/// as it opens among them.
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Logs {
    /// Records what is logged at `level` or above on this thread
    /// until the guard drops.
    pub fn record(
        &self,
        level: LevelFilter,
    ) -> tracing::subscriber::DefaultGuard {
        tracing_subscriber::fmt()
            .with_max_level(level)
            .with_span_events(FmtSpan::NEW)
            .with_ansi(false)
            .with_writer({
                let logs = self.clone();
                move || logs.clone()
            })
            .finish()
            .set_default()
    }

    /// What was logged so far.
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().clone()).unwrap()
    }
}

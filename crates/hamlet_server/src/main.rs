//! `hamlet_server`: serves a claimed realm over the client protocol.

use std::io::IsTerminal;

use bevy_app::{App, AppExit};
use bevy_log::{BoxedFmtLayer, LogPlugin};
use hamlet_server::ServerPlugin;
use hamlet_server::options::Options;

#[expect(
    clippy::print_stderr,
    reason = "a command line the server cannot read is its only \
              output for whoever started it"
)]
fn main() -> AppExit {
    match Options::parse(std::env::args_os().skip(1)) {
        Ok(options) => App::new()
            .add_plugins((
                LogPlugin {
                    fmt_layer: log_lines,
                    ..Default::default()
                },
                ServerPlugin { options },
            ))
            .run(),
        Err(error) => {
            eprintln!("Error: {error:?}");
            AppExit::error()
        }
    }
}

/// The log on stderr: Bevy's readable lines in a terminal, else one
/// JSON object per line, so words with a newline or a terminal escape
/// cannot break or forge a line of `server.log`.
fn log_lines(_: &mut App) -> Option<BoxedFmtLayer> {
    if std::io::stderr().is_terminal() {
        return None;
    }
    Some(Box::new(
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(std::io::stderr),
    ))
}

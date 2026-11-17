//! `hamlet_server`: serves a claimed realm over the client protocol.

use bevy_app::AppExit;
use hamlet_server::app::options::Options;
use hamlet_server::create_app;

#[expect(
    clippy::print_stderr,
    reason = "a command line the server cannot read is its only \
              output for whoever started it"
)]
fn main() -> AppExit {
    match Options::parse(std::env::args_os().skip(1)) {
        Ok(options) => create_app(options).run(),
        Err(error) => {
            eprintln!("Error: {error:?}");
            AppExit::error()
        }
    }
}

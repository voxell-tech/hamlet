//! `hamlet_server`: serves a claimed realm over the client protocol.

use bevy_app::{App, AppExit};
use hamlet_server::ServerPlugin;
use hamlet_server::options::Options;

#[expect(
    clippy::print_stderr,
    reason = "a command line the server cannot read is its only \
              output for whoever started it"
)]
fn main() -> AppExit {
    match Options::parse(std::env::args_os().skip(1)) {
        Ok(options) => {
            App::empty().add_plugins(ServerPlugin { options }).run()
        }
        Err(error) => {
            eprintln!("Error: {error:?}");
            AppExit::error()
        }
    }
}

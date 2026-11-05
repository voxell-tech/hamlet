//! The `hamlet_server` executable.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let _ = writeln!(
        std::io::stderr(),
        "hamlet_server: the rebuilt server does not serve yet"
    );
    ExitCode::FAILURE
}

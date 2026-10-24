//! Compiles a built plugin for the engine, so it starts without
//! compiling:
//!
//! ```sh
//! cargo run --release -p hamlet_engine_harness --bin precompile_plugins -- <plugin.wasm> <plugin folder>
//! ```
//!
//! The result is kept in the plugin folder's `compiled` folder, which
//! the engine reads before it compiles. `scripts/build_plugins.sh`
//! runs this for each plugin it lays out.

use std::path::PathBuf;
use std::process::ExitCode;

use hamlet_engine_wasm::{SHIPPED, precompile};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(wasm), Some(folder)) = (args.next(), args.next())
    else {
        eprintln!("precompile_plugins <plugin.wasm> <plugin folder>");
        return ExitCode::FAILURE;
    };
    match precompile(
        &PathBuf::from(wasm),
        &PathBuf::from(folder).join(SHIPPED),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("failed: {why}");
            ExitCode::FAILURE
        }
    }
}

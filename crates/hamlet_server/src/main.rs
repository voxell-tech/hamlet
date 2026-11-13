//! `hamlet_server`: serves a claimed realm over the client protocol.

use anyhow::Result;
use hamlet_server::serve::{Options, run};

fn main() -> Result<()> {
    run(Options::parse(std::env::args_os().skip(1))?)
}

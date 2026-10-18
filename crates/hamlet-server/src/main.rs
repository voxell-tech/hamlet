//! `hamlet-server`: serves a claimed home over protocol v0.

use anyhow::Result;
use hamlet_server::runtime::{Options, serve};

fn main() -> Result<()> {
    let options = Options::parse(std::env::args_os().skip(1))?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve(options))
}

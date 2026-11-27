//! `hamlet_server`: serves a claimed realm over the client protocol.

use anyhow::Result;
use hamlet_server::serve::{Options, serve};
use tokio::runtime::Builder;

fn main() -> Result<()> {
    let options = Options::parse(std::env::args_os().skip(1))?;
    // The actors block a worker thread while they run synchronous
    // code, which only a multi-thread runtime allows.
    Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(options))
}

//! `hamlet-harness-driver`, the harness tests' driver; its body is
//! `hamlet_server::harness_driver`.

fn main() -> anyhow::Result<()> {
    hamlet_server::harness_driver::run()
}

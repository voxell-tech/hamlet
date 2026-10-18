//! Times a built plugin: its compile, its `.cwasm`, instantiating it
//! and one `step`.
//!
//! `cargo run --release -p hamlet_wasm --example bench --
//! <plugin.wasm>`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use hamlet_wasm::engine::Host;
use hamlet_wasm::instance::Instance;
use hamlet_wasm::interface::exports::hamlet::plugin::plugin::Incoming;
use hamlet_wasm::limits::Limits;
use wasmtime::component::Component;

const INSTANTIATIONS: u32 = 500;
const STEPS: u32 = 20_000;

fn average(total: Duration, runs: u32) -> Duration {
    total / runs
}

fn main() {
    let Some(file) = std::env::args().nth(1).map(PathBuf::from)
    else {
        eprintln!("usage: bench <plugin.wasm>");
        return;
    };
    let limits = Limits::default();
    let host = Host::new(&limits).expect("the engine builds");
    let engine = &host.0;

    let start = Instant::now();
    let component = Component::from_file(engine, &file)
        .expect("the plugin compiles");
    println!("compile              {:?}", start.elapsed());

    let cwasm = component.serialize().expect("the plugin serializes");
    println!("cwasm size           {} bytes", cwasm.len());
    println!(
        "wasm size            {} bytes",
        std::fs::metadata(&file).map_or(0, |m| m.len())
    );

    let start = Instant::now();
    // SAFETY: the bytes were made by this engine a moment ago.
    let _ = unsafe { Component::deserialize(engine, &cwasm) }
        .expect("the cwasm loads");
    println!("deserialize          {:?}", start.elapsed());

    let start = Instant::now();
    for _ in 0..INSTANTIATIONS {
        drop(Instance::new(engine, &component, &limits).unwrap());
    }
    println!(
        "instantiate + drop   {:?}",
        average(start.elapsed(), INSTANTIATIONS)
    );

    let mut instance =
        Instance::new(engine, &component, &limits).unwrap();
    instance.entry(&limits).unwrap();
    let inbox = [Incoming::Tick(1)];
    let start = Instant::now();
    for _ in 0..STEPS {
        instance.step(&limits, &inbox).unwrap();
    }
    println!(
        "step round trip      {:?}",
        average(start.elapsed(), STEPS)
    );
}

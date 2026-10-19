//! Builds the sample native plugin and loads it, which a plain
//! `cargo test` does not: `cargo test -p hamlet_native --test sample
//! -- --ignored`.

use std::process::Command;

use bevy::app::App;
use hamlet_native::{library, load};
use hamlet_sdk::plugin::Declared;

#[test]
#[ignore = "builds a library"]
fn the_sample_builds_and_loads_through_the_gate() {
    let built = Command::new(env!("CARGO"))
        // This test is built from the same few crates as the library,
        // so the features of the engine match. The app's own build is
        // matched with `--workspace` instead.
        .args(["build", "-p", "native_spark"])
        .status()
        .expect("cargo runs");
    assert!(built.success());
    // The test runs from target/<profile>/deps; the library is in
    // target/<profile>.
    let profile = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(|deps| deps.parent())
        .map(|profile| profile.to_path_buf())
        .unwrap();
    let file = profile.join(format!(
        "{}native_spark.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    ));

    let entry = library::open(&file).unwrap_or_else(|why| {
        panic!("the sample was turned down: {why}")
    });
    let mut app = App::new();
    app.add_plugins(bevy::MinimalPlugins);
    app.update();
    assert!(load::with_app(app.world_mut(), entry).unwrap());
    assert!(app.world().contains_resource::<Declared>());
    // Its system runs with the others.
    app.update();
}

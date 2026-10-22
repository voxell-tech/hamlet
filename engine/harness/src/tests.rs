use std::path::Path;
use std::process::Command;

use crate::{Harness, Steps};

/// Builds `engine/samples/hello`, `notes` and `board` and runs each
/// one's `steps.toml`: hello counts its presses and its command sets
/// the count back, the board shows the notes plugin's note as it
/// changes, counts its words with its service, and the two exchange
/// events.
#[test]
#[ignore = "builds the samples, with the wasm32-wasip2 target"]
fn the_samples_pass_their_steps() {
    let samples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples");
    for sample in ["hello", "notes", "board"] {
        let folder = samples.join(sample);
        let built = Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(&folder)
            .status()
            .expect("cargo runs");
        assert!(built.success(), "{sample} does not build");
    }
    for sample in ["hello", "notes", "board"] {
        let folder = samples.join(sample);
        let steps = Steps::read(&folder.join("steps.toml")).unwrap();
        let home = std::env::temp_dir().join(format!(
            "hamlet_engine_test_harness_{sample}_{}",
            std::process::id()
        ));
        let mut harness = Harness::new(&home);
        for plugin in std::iter::once(&folder).chain(&steps.with) {
            harness.install(plugin).unwrap();
        }
        let passed = steps.run(&mut harness, |_| {});
        let log = harness.log().join("\n");
        let _ = std::fs::remove_dir_all(&home);
        if let Err(why) = passed {
            panic!("{sample}: {why}\n{log}");
        }
    }
}

//! Checks built plugins without a window:
//!
//! ```sh
//! cargo run -p hamlet_engine_harness --bin plugin_check -- <folder> [<folder>...] [--steps <file>]
//! ```
//!
//! Each folder holds a plugin's `plugin.toml`, and its `plugin.wasm`
//! or its crate's release build. The steps are the first folder's
//! `steps.toml` unless `--steps` names a file. Without steps, each
//! plugin is let in with all it asks and its panels are printed. The
//! home is a scratch folder, taken away after.

use std::path::PathBuf;
use std::process::ExitCode;

use bevy::ecs::query::With;
use hamlet_engine_grants::Answer;
use hamlet_engine_harness::{Harness, Steps};
use hamlet_engine_plugins::ContributedBy;
use hamlet_engine_shell::Panel;

fn main() -> ExitCode {
    let mut folders = Vec::new();
    let mut steps = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--steps" {
            steps = args.next().map(PathBuf::from);
        } else {
            folders.push(PathBuf::from(arg));
        }
    }
    let Some(first) = folders.first().cloned() else {
        eprintln!(
            "plugin_check <folder> [<folder>...] [--steps <file>]"
        );
        return ExitCode::FAILURE;
    };
    let steps = steps.or_else(|| {
        let beside = first.join("steps.toml");
        beside.is_file().then_some(beside)
    });
    let home = std::env::temp_dir()
        .join(format!("plugin_check_{}", std::process::id()));
    let outcome = check(&home, folders, steps);
    let _ = std::fs::remove_dir_all(&home);
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("failed: {why}");
            ExitCode::FAILURE
        }
    }
}

fn check(
    home: &std::path::Path,
    mut folders: Vec<PathBuf>,
    steps: Option<PathBuf>,
) -> Result<(), String> {
    let steps = steps.map(|path| Steps::read(&path)).transpose()?;
    if let Some(steps) = &steps {
        folders.extend(steps.with.iter().cloned());
    }
    let mut harness = Harness::new(home);
    let mut ids = Vec::new();
    for folder in &folders {
        let id = harness.install(folder)?;
        println!("installed {id} from {}", folder.display());
        ids.push(id);
    }
    let Some(steps) = steps else {
        for id in &ids {
            harness.allow(id.as_str(), Answer::Allow)?;
        }
        harness.until(|harness| {
            ids.iter().all(|id| harness.running(id.as_str()))
        });
        harness.frames(10);
        let world = harness.world();
        let panels = world
            .query_filtered::<&Panel, With<ContributedBy>>()
            .iter(world)
            .map(|panel| panel.id.to_string())
            .collect::<Vec<_>>();
        for panel in panels {
            // The first call only builds a panel's content, which the
            // plugin fills on a later frame.
            let mut shown = None;
            harness.until(|harness| {
                shown = harness.outline(&panel);
                shown.as_ref().is_some_and(|shown| !shown.is_empty())
            });
            if let Some(shown) = shown {
                println!("{panel}\n{shown}\n");
            }
        }
        return Ok(());
    };
    let passed =
        steps.run(&mut harness, |step| println!("ok: {step:?}"));
    if passed.is_err() {
        for line in harness.log() {
            eprintln!("log: {line}");
        }
    }
    passed?;
    println!("all {} steps passed", steps.steps.len());
    Ok(())
}

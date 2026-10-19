use bevy::prelude::*;
use hamlet_base::Tokio;
use hamlet_sdk::plugin::Status;

use super::tree::items;
use crate::cache::{self, Origin};
use crate::engine::Host;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::limits::Limits;
use crate::{fixture, install};

fn component(count: &'static str) -> Vec<u8> {
    fixture::component(&fixture::drawing(
        &items(count),
        &items(count),
    ))
}

#[test]
fn the_cache_is_used_while_it_is_valid_and_ignored_when_it_is_not() {
    let scratch = Scratch::new("cache");
    let folder = scratch.plugins().join("probe");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("plugin.wasm");
    let limits = Limits::default();
    let host = Host::new(&limits).unwrap();
    let origin = |file: &std::path::Path| {
        cache::component(&host.0, file, limits.wasm_bytes)
            .unwrap()
            .1
    };
    let entries = || {
        std::fs::read_dir(
            scratch.home().join(cache::FOLDER).join("probe"),
        )
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>()
    };

    std::fs::write(&file, component("first")).unwrap();
    assert!(!cache::cached(&host.0, &file, limits.wasm_bytes));
    assert_eq!(origin(&file), Origin::Compiled);
    assert!(cache::cached(&host.0, &file, limits.wasm_bytes));
    assert_eq!(origin(&file), Origin::Cache);
    assert!(cache::component(&host.0, &file, 16).is_err());

    // An entry that cannot be read is compiled again.
    std::fs::write(&entries()[0], b"not a module").unwrap();
    assert_eq!(origin(&file), Origin::Compiled);
    assert_eq!(origin(&file), Origin::Cache);

    // Another file has an entry of its own, and the old one goes.
    std::fs::write(&file, component("second")).unwrap();
    assert_eq!(origin(&file), Origin::Compiled);
    assert_eq!(entries().len(), 1);
}

#[test]
fn a_plugin_compiled_off_the_frame_starts_when_the_compile_is_done() {
    let scratch = Scratch::new("compiled_off_frame");
    home::put(&scratch, &[Plugin::new("probe", component("first"))]);
    let mut app = home::open(&scratch);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    app.insert_resource(Tokio(runtime.handle().clone()));

    app.update();
    let status = |app: &mut App| {
        app.world_mut()
            .query::<&Status>()
            .single(app.world())
            .copied()
            .unwrap()
    };
    for _ in 0..2000 {
        if status(&mut app) == Status::Running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.update();
    }
    assert_eq!(status(&mut app), Status::Running);
}

#[test]
fn a_cache_that_came_with_the_plugin_is_never_read() {
    let scratch = Scratch::new("shipped_cache");
    let folder = scratch.plugins().join("probe");
    let shipped = folder.join(cache::SHIPPED);
    std::fs::create_dir_all(&shipped).unwrap();
    std::fs::write(shipped.join("any.cwasm"), b"native code")
        .unwrap();
    std::fs::write(folder.join("manifest.toml"), "id = \"probe\"\n")
        .unwrap();

    let _ = install::read(&folder);
    assert!(!shipped.exists());
}

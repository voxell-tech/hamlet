use std::path::Path;

use wasmtime::component::Component;

use crate::engine::Host;
use crate::fixture;
use crate::fixture::home::Scratch;
use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::Intent;
use crate::limits::Limits;
use crate::storage::{Caps, Storage};

fn kept(folder: &Path) -> usize {
    let limits = Limits::default().patient();
    let host = Host::new(&limits).unwrap();
    let component = Component::new(
        &host.0,
        fixture::component(&fixture::keeping()),
    )
    .unwrap();
    let mut plugin = Instance::new(&host.0, &component, &limits)
        .unwrap()
        .stored(folder.into(), &limits);
    plugin.entry(&limits).unwrap();
    match &plugin.step(&limits, &[]).unwrap().intents[..] {
        [Intent::LogLine(line)] => line.len(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_value_stored_is_there_for_a_new_engine_on_the_same_home() {
    let scratch = Scratch::new("storage");
    let folder = scratch.plugins().join("probe/data");

    assert_eq!(kept(&folder), 0);
    assert_eq!(kept(&folder), 11);
}

#[test]
fn a_write_over_a_cap_or_under_a_bad_key_is_an_error_and_stays_in_the_folder()
 {
    let scratch = Scratch::new("storage_caps");
    let folder = scratch.plugins().join("probe/data");
    let storage = Storage::new(
        folder.clone(),
        Caps {
            bytes: 10,
            keys: 2,
            key_bytes: 8,
            value_bytes: 6,
        },
    );

    assert!(storage.set("a", b"123456").is_ok());
    assert!(storage.set("b", b"1234").is_ok());
    assert!(storage.set("c", b"1").is_err(), "a third key");
    assert!(storage.set("a", b"1234567").is_err(), "a long value");
    assert!(storage.set("b", b"12345").is_err(), "a full store");
    assert!(storage.set("a", b"12").is_ok());
    for key in ["../x", "a/b", "", ".h", "Up", "toolongkey"] {
        assert!(storage.set(key, b"x").is_err(), "{key}");
        assert_eq!(storage.get(key), None);
    }

    assert_eq!(storage.keys(""), ["a", "b"]);
    assert_eq!(storage.get("a"), Some(b"12".to_vec()));
    assert!(!folder.parent().unwrap().join("x").exists());
    let files = std::fs::read_dir(&folder).unwrap().count();
    assert_eq!(files, 2);
}

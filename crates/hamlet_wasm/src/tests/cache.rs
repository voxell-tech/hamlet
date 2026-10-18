use super::tree::items;
use crate::cache::{self, Origin};
use crate::engine::Host;
use crate::fixture;
use crate::fixture::home::Scratch;
use crate::limits::Limits;

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
    let host = Host::new(&Limits::default()).unwrap();
    let origin = |file: &std::path::Path| {
        cache::component(&host.0, file).unwrap().1
    };
    let entries = || {
        std::fs::read_dir(folder.join(cache::FOLDER))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>()
    };

    std::fs::write(&file, component("first")).unwrap();
    assert_eq!(origin(&file), Origin::Compiled);
    assert_eq!(origin(&file), Origin::Cache);

    // An entry that cannot be read is compiled again.
    std::fs::write(&entries()[0], b"not a module").unwrap();
    assert_eq!(origin(&file), Origin::Compiled);
    assert_eq!(origin(&file), Origin::Cache);

    // Another file has an entry of its own, and the old one goes.
    std::fs::write(&file, component("second")).unwrap();
    assert_eq!(origin(&file), Origin::Compiled);
    assert_eq!(entries().len(), 1);
}

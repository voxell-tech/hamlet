use bevy::prelude::*;
use hamlet_agents_api::oversight::Choice as Answer;
use hamlet_sdk::capability::{AskToRemove, Store, Subject};
use hamlet_sdk::plugin::PluginInfo;

use super::permissions::{answer, petitions};
use super::tree::items;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::instance::Instance;

fn probe() -> Plugin {
    let wasm = fixture::component(&fixture::drawing(
        &items("a"),
        &items("b"),
    ));
    Plugin::new("probe", wasm)
}

/// A home with `probe` that kept a value, whose removal the chief
/// answered as `choice`. Returns the home's app.
fn removed(scratch: &Scratch, choice: Answer) -> App {
    let mut app = home::app(scratch, &[probe()]);
    app.update();
    let data = scratch.plugins().join("probe/data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("key-note"), b"kept").unwrap();
    let db = scratch.plugins().join("probe/db");
    std::fs::create_dir_all(&db).unwrap();
    std::fs::write(db.join("items.tbl"), b"kept").unwrap();

    assert!(cache_of(scratch).is_dir());

    app.world_mut().trigger(AskToRemove {
        plugin: "probe".into(),
    });
    app.world_mut().flush();
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(_, Subject::Remove { plugin })] if plugin == "probe"
    ));
    answer(&mut app, asked[0].0, choice);
    app
}

fn cache_of(scratch: &Scratch) -> std::path::PathBuf {
    scratch
        .plugins()
        .parent()
        .unwrap()
        .join(crate::cache::FOLDER)
        .join("probe")
}

fn plugins(app: &mut App) -> usize {
    app.world_mut()
        .query::<&PluginInfo>()
        .iter(app.world())
        .count()
}

#[test]
fn removing_a_plugin_and_keeping_its_data_leaves_the_data_to_be_found_again()
 {
    let scratch = Scratch::new("uninstall_keep");
    let mut app = removed(&scratch, Answer::KeepData);

    assert_eq!(plugins(&mut app), 0);
    assert!(!cache_of(&scratch).exists());
    let folder = scratch.plugins().join("probe");
    let mut left = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    left.sort();
    assert_eq!(left, ["data", "db"]);
    assert!(Store::open(&scratch.plugins()).get("probe").is_none());

    // A folder of nothing but data is not a plugin, and installing
    // the plugin again finds the data where it was.
    let mut quiet = home::open(&scratch);
    quiet.update();
    assert!(petitions(&mut quiet).is_empty());
    home::put(&scratch, &[probe()]);
    let mut again = home::open(&scratch);
    again.update();
    let mut running = again.world_mut().query::<&Instance>();
    assert_eq!(running.iter(again.world()).count(), 1);
    assert_eq!(
        std::fs::read(folder.join("data/key-note")).unwrap(),
        b"kept"
    );
    assert_eq!(
        std::fs::read(folder.join("db/items.tbl")).unwrap(),
        b"kept"
    );
}

#[test]
fn removing_a_plugin_with_its_data_deletes_the_folder() {
    let scratch = Scratch::new("uninstall_erase");
    let mut app = removed(&scratch, Answer::EraseData);

    assert_eq!(plugins(&mut app), 0);
    assert!(!scratch.plugins().join("probe").exists());
    assert!(Store::open(&scratch.plugins()).get("probe").is_none());
}

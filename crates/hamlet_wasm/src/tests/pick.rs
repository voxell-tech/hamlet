use std::fs;

use bevy::prelude::*;
use hamlet_base::Tokio;
use hamlet_desktop::file::{FileDialog, unseen};
use hamlet_sdk::capability::{Capability, Subject};

use super::acts::{does, open, probe};
use super::permissions::petitions;
use crate::fixture::home::Scratch;
use crate::interface::exports::hamlet::plugin::plugin::{
    Intent, PickRequest,
};
use crate::limits::Limits;
use crate::pick::Picking;

fn pick() -> Vec<Intent> {
    vec![Intent::PickFile(PickRequest {
        id: 1,
        title: "a tide table".into(),
    })]
}

#[test]
fn a_file_the_chief_picks_is_copied_into_the_plugins_folder_within_the_cap()
 {
    let scratch = Scratch::new("pick_copied");
    let mut app = open(
        &scratch,
        probe().asking(&["fs.pick"]).granted(&["fs.pick"]),
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    app.insert_resource(FileDialog(unseen))
        .insert_resource(Tokio(runtime.handle().clone()))
        .insert_resource(Limits {
            file_bytes: 16,
            ..Limits::default().patient()
        });
    let outside = scratch.home().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join(".Tide table!.txt"), "high water")
        .unwrap();
    fs::write(outside.join("big.txt"), "x".repeat(17)).unwrap();
    let choose = |app: &mut App, name: Option<&str>| {
        does(app, pick());
        let picking = app.world().resource::<Picking>();
        assert!(picking.is_open());
        picking.choose(name.map(|name| outside.join(name)));
        for _ in 0..500 {
            app.update();
            if !app.world().resource::<Picking>().under_way() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the copy never finished");
    };

    choose(&mut app, Some(".Tide table!.txt"));
    choose(&mut app, Some("big.txt"));
    choose(&mut app, Some(".Tide table!.txt"));
    choose(&mut app, None);
    // A plugin whose dialog was closed with nothing rests.
    does(&mut app, pick());
    assert!(!app.world().resource::<Picking>().is_open());

    let picked = scratch.plugins().join("probe/files/picked");
    let read =
        |name: &str| fs::read_to_string(picked.join(name)).unwrap();
    assert_eq!(read("Tide table_.txt"), "high water");
    assert_eq!(read("Tide table_-2.txt"), "high water");
    assert!(!picked.join("big.txt").exists());
}

#[test]
fn no_dialog_opens_until_the_chief_allows_fs_pick() {
    let scratch = Scratch::new("pick_asked");
    let mut app = open(
        &scratch,
        probe().asking(&["fs.pick"]).asking_next_time(&["fs.pick"]),
    );
    app.insert_resource(FileDialog(unseen));

    does(&mut app, pick());

    assert!(!app.world().resource::<Picking>().is_open());
    assert!(matches!(
        &petitions(&mut app)[..],
        [(
            _,
            Subject::Use {
                capability: Capability::FsPick,
                ..
            }
        )]
    ));
}

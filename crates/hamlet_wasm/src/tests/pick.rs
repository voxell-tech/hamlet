use std::fs;

use bevy::prelude::*;
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
    app.insert_resource(FileDialog(unseen))
        .insert_resource(Limits {
            file_bytes: 16,
            ..Limits::default().patient()
        });
    let outside = scratch.home().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join(".Tide table!.txt"), "high water")
        .unwrap();
    fs::write(outside.join("big.txt"), "x".repeat(17)).unwrap();
    let choose = |app: &mut App, name: &str| {
        does(app, pick());
        let picking = app.world().resource::<Picking>();
        assert!(picking.is_open());
        picking.choose(Some(outside.join(name)));
        app.update();
    };

    choose(&mut app, ".Tide table!.txt");
    choose(&mut app, "big.txt");

    let picked = scratch.plugins().join("probe/files/picked");
    assert_eq!(
        fs::read_to_string(picked.join("Tide table_.txt")).unwrap(),
        "high water"
    );
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

use std::fs;

use bevy::prelude::*;
use hamlet_agents_api::oversight::{Choice as Answer, Waiting};
use hamlet_desktop::file::{FileDialog, unseen};

use super::acts::{does, open, probe};
use super::permissions::answer;
use crate::fixture::home::Scratch;
use crate::folder::AskedWrite;
use crate::interface::exports::hamlet::plugin::plugin::{
    FolderFile, FolderOp, FolderRequest, Intent, PickRequest,
};
use crate::pick::Picking;

fn write(path: &str) -> Vec<Intent> {
    vec![Intent::Folder(FolderRequest {
        id: 2,
        op: FolderOp::Write(FolderFile {
            path: path.into(),
            data: b"high water".to_vec(),
        }),
    })]
}

/// The petitions that put a write to the chief.
fn asked(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, (With<Waiting>, With<AskedWrite>)>()
        .iter(app.world())
        .collect()
}

#[test]
fn a_write_in_the_folder_the_chief_chose_waits_for_the_chief() {
    let scratch = Scratch::new("folder_write");
    let work = std::env::temp_dir()
        .join(format!("hamlet_wasm_work_{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let mut app = open(&scratch, probe().asking(&["fs.folder"]));
    app.insert_resource(FileDialog(unseen));
    let pick = |app: &mut App, folder| {
        does(
            app,
            vec![Intent::PickFolder(PickRequest {
                id: 1,
                title: "the tide tables".into(),
            })],
        );
        app.world().resource::<Picking>().choose(Some(folder));
        app.update();
        app.update();
    };

    does(&mut app, write("tides.md"));
    assert!(asked(&mut app).is_empty());

    // Hamlet's home is never one.
    pick(&mut app, scratch.home());
    does(&mut app, write("tides.md"));
    assert!(asked(&mut app).is_empty());

    pick(&mut app, work.clone());
    does(&mut app, write("../tides.md"));
    does(&mut app, write("tides.md"));
    app.update();
    let asked = asked(&mut app);
    assert_eq!(asked.len(), 1);
    assert!(!work.join("tides.md").exists());

    answer(&mut app, asked[0], Answer::Allow);
    app.update();
    assert_eq!(
        fs::read_to_string(work.join("tides.md")).unwrap(),
        "high water"
    );
    assert!(!scratch.home().join("tides.md").exists());
    let _ = fs::remove_dir_all(&work);
}

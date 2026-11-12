use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::files::{
    FilesKept, FilesRefused, Gather, Gathered, UploadFiles,
};
use hamlet_client::outbox::Outbox;
use hamlet_client::{ClientPlugin, Tokio, receive};
use hamlet_protocol::client as protocol;

fn reply(to: &protocol::RequestId) -> protocol::Reply {
    protocol::Reply {
        to: to.clone(),
        kind: protocol::ReplyKind::Notice,
        text: String::new(),
        upload: None,
        file: None,
        files: Vec::new(),
        image: None,
        images: Vec::new(),
    }
}

fn sent(app: &mut App) -> Vec<protocol::ClientMessage> {
    app.world_mut().resource_mut::<Outbox>().drain().collect()
}

fn file_id(digit: char) -> protocol::FileId {
    digit.to_string().repeat(64).parse().unwrap()
}

#[test]
fn a_folder_is_gathered_then_sent_file_by_file_under_its_names() {
    let folder = hamlet_testing::Scratch::new("files");
    let notes = folder.join("notes");
    std::fs::create_dir_all(notes.join("old")).unwrap();
    std::fs::write(notes.join("a.txt"), b"hello").unwrap();
    std::fs::write(notes.join("old/empty"), b"").unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None })
        .insert_resource(Tokio(runtime.handle().clone()));
    let gathered = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::new(Mutex::new(Vec::new()));
    let refused = Arc::new(Mutex::new(Vec::new()));
    let g = gathered.clone();
    app.add_observer(move |e: On<Gathered>| {
        g.lock().unwrap().push((e.key, e.files, e.bytes));
    });
    let k = kept.clone();
    app.add_observer(move |e: On<FilesKept>| {
        k.lock().unwrap().push(e.files.clone());
    });
    let r = refused.clone();
    app.add_observer(move |e: On<FilesRefused>| {
        r.lock().unwrap().push(e.text.clone());
    });

    // The walk is off the frame, so the count arrives in a later one.
    app.world_mut().trigger(Gather {
        key: 3,
        paths: vec![notes],
    });
    let start = Instant::now();
    while gathered.lock().unwrap().is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the folder was not counted"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*gathered.lock().unwrap(), [(3, 2, 5)]);
    assert!(sent(&mut app).is_empty());

    app.world_mut().trigger(UploadFiles { key: 3 });
    app.update();
    let begun = sent(&mut app);
    let names = |at: &protocol::ClientMessage| match at {
        protocol::ClientMessage::PutFile(put) => put.size,
        other => panic!("expected PutFile, got {other:?}"),
    };
    assert_eq!(begun.iter().map(names).collect::<Vec<_>>(), [5, 0]);
    let [
        protocol::ClientMessage::PutFile(text),
        protocol::ClientMessage::PutFile(empty),
    ] = &begun[..]
    else {
        panic!("two files were announced: {begun:?}");
    };

    // An empty file is kept at once; the other needs its part.
    let mut done = reply(&empty.id);
    done.file = Some(file_id('b'));
    receive(app.world_mut(), protocol::ServerMessage::Reply(done));
    let mut begin = reply(&text.id);
    begin.upload = Some("up".into());
    receive(app.world_mut(), protocol::ServerMessage::Reply(begin));
    let start = Instant::now();
    let part = loop {
        app.update();
        let mut out = sent(&mut app);
        if let Some(message) = out.pop() {
            break message;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the part was not sent"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let protocol::ClientMessage::PutFilePart(part) = part else {
        panic!("expected the part, got {part:?}");
    };
    assert_eq!((part.offset, part.upload.as_str()), (0, "up"));
    let mut done = reply(&part.id);
    done.file = Some(file_id('a'));
    receive(app.world_mut(), protocol::ServerMessage::Reply(done));
    app.update();

    assert!(refused.lock().unwrap().is_empty());
    let names = kept.lock().unwrap()[0]
        .iter()
        .map(|file| (file.name.clone(), file.size))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            ("notes/a.txt".to_string(), 5),
            ("notes/old/empty".into(), 0)
        ]
    );
}

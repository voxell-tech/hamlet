use std::sync::{Arc, Mutex};

use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::images::{ImageKept, ImageRefused, Upload};
use hamlet_client::outbox::Outbox;
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::client as protocol;

fn reply(
    to: &protocol::RequestId,
    kind: protocol::ReplyKind,
) -> protocol::Reply {
    protocol::Reply {
        to: to.clone(),
        kind,
        text: String::new(),
        upload: None,
        file: None,
        files: Vec::new(),
        image: None,
        images: Vec::new(),
    }
}

#[test]
fn an_image_goes_up_in_parts_and_the_kept_id_comes_back() {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    let kept = Arc::new(Mutex::new(Vec::new()));
    let refused = Arc::new(Mutex::new(Vec::new()));
    let (k, r) = (kept.clone(), refused.clone());
    app.add_observer(move |e: On<ImageKept>| {
        k.lock().unwrap().push((e.key, e.image.clone()));
    });
    app.add_observer(move |e: On<ImageRefused>| {
        r.lock().unwrap().push((e.key, e.text.clone()));
    });
    // A little over one part of 512 KiB.
    let bytes = vec![7u8; 512 * 1024 + 10];
    app.world_mut().trigger(Upload { key: 9, bytes });
    let sent = app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .collect::<Vec<_>>();
    let [protocol::ClientMessage::PutImage(begin)] = &sent[..] else {
        panic!("expected one PutImage, got {sent:?}");
    };
    assert_eq!(begin.size, 512 * 1024 + 10);

    let mut notice = reply(&begin.id, protocol::ReplyKind::Notice);
    notice.upload = Some("up".into());
    receive(app.world_mut(), protocol::ServerMessage::Reply(notice));
    app.update();
    let sent = app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .collect::<Vec<_>>();
    let [protocol::ClientMessage::PutImagePart(first)] = &sent[..]
    else {
        panic!("expected the first part, got {sent:?}");
    };
    assert_eq!((first.offset, first.upload.as_str()), (0, "up"));

    receive(
        app.world_mut(),
        protocol::ServerMessage::Reply(reply(
            &first.id,
            protocol::ReplyKind::Notice,
        )),
    );
    app.update();
    let sent = app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .collect::<Vec<_>>();
    let [protocol::ClientMessage::PutImagePart(last)] = &sent[..]
    else {
        panic!("expected the last part, got {sent:?}");
    };
    assert_eq!(last.offset, 512 * 1024);

    let id: protocol::ImageId = "a".repeat(64).parse().unwrap();
    let mut done = reply(&last.id, protocol::ReplyKind::Notice);
    done.image = Some(id.clone());
    receive(app.world_mut(), protocol::ServerMessage::Reply(done));
    app.update();
    assert_eq!(*kept.lock().unwrap(), [(9, id)]);
    assert!(refused.lock().unwrap().is_empty());
}

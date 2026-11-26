//! A realm working in a remote workspace, served by a loopback
//! double of the worker that keeps its files in a scratch folder: the
//! agents' tools write their remote lodge, the server brings it home,
//! and a path outside the workspace is refused there.

use std::fs;

use hamlet_acceptance::{Client, Faux, call, chat, words};
use hamlet_protocol::client::{
    ClientMessage, Edit, EditSettingsSet, EditSettingsSetWorkspace,
    Keeper, MoveLodges, ReplyKind,
};
use hamlet_testing::Server;
use serde_json::json;

use super::Realm;
use super::worker::{TOKEN, Worker};

/// A realm whose chief chose `worker` and moved the lodges there,
/// served again so that it works there.
async fn moved_to(
    worker: &Worker,
    faux: Faux,
) -> (Realm, Server, Client) {
    let mut realm = Realm::new();
    let (mut server, mut chief) = realm.serve(Faux::new()).await;
    // What the Hand learned before, which the move must carry.
    let hand = chief.hand();
    let reply = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(hand),
                path: "tea.md".into(),
                text: "---\ndescription: Tea\n---\nEarl grey.\n"
                    .into(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let url = worker.url.clone();
    let reply = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    allow_net: None,
                    character: None,
                    effort: None,
                    endpoints: None,
                    model: None,
                    newcomers: None,
                    reflect_every: None,
                    workspace: Some(Some(EditSettingsSetWorkspace {
                        token: TOKEN.into(),
                        url,
                    })),
                },
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let reply = chief
        .request(|id| ClientMessage::MoveLodges(MoveLodges { id }))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    drop(chief);
    assert_eq!(server.stop().await, Some(0));
    let (server, chief) = realm.serve(faux).await;
    (realm, server, chief)
}

#[tokio::test]
async fn the_hands_tools_write_its_remote_lodge_and_the_server_brings_it_home()
 {
    let worker = Worker::start();
    let write = call(
        "write",
        json!({"path": "memory/remote.md", "content": "Kept remotely.\n"}),
    );
    let host = call("read", json!({"path": "/etc/hosts"}));
    let (realm, _server, mut chief) = moved_to(
        &worker,
        Faux::new().responses([write, host, words("Written.")]),
    )
    .await;
    let tea = worker
        .files
        .join("realm/lodges")
        .join(format!("{}/memory/tea.md", chief.hand().0));
    assert!(tea.exists(), "the lodges were moved");

    let hand = chief.hand();
    let hand_chat = chat(hand);
    chief.open(&hand_chat).await;
    chief
        .tell(hand, "Remember this, then read the hosts.")
        .await;
    chief.shows(&hand_chat, "Written.").await;

    // The write ran remotely and came home beside what was kept.
    let home = realm.path("hand/memory/remote.md");
    assert_eq!(fs::read_to_string(home).unwrap(), "Kept remotely.\n");
    assert!(realm.path("hand/memory/tea.md").exists());
    // A host path is refused there, never read here.
    let tools = chief
        .items(&hand_chat)
        .into_iter()
        .filter_map(|item| item.tool)
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 2, "{tools:?}");
    assert_eq!(tools[0].failure, None, "{tools:?}");
    assert!(tools[1].failure.is_some(), "{tools:?}");
    let requests = worker.requests();
    // The library goes over once, then by its digest alone.
    let carried = requests
        .iter()
        .filter(|r| r["library"]["files"].is_array())
        .count();
    assert_eq!(carried, 1, "{requests:#?}");
}

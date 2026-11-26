use std::path::PathBuf;
use std::time::Duration;

use hamlet_actors::realm::ParcelStore;
use hamlet_messages::agent::{Input, InputId, LineFile, LineImage};
use hamlet_messages::bytes::Sha256;
use hamlet_messages::realm::client::SayAnswer;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    AttachedFile, AttachedImage, Choice, RequestId,
};

use super::asking::{chief_task, given, objection, open_waits};
use super::parcel_files::{aged, hash};
use super::parcels::fetched;
use super::stand_ins::Callee;
use super::{
    FOUR, MAYOR, Realm, THREE, answer, hamlet_with, paid, say,
    seen_submits, submits_to, tell, to, tool, until,
};

#[tokio::test(flavor = "multi_thread")]
async fn a_line_to_no_agent_names_its_address() {
    let (realm, _) = hamlet_with(0).await;

    let said = realm
        .ask(&say(&RequestId::new(), AgentNumber(9), "Hello."))
        .await
        .unwrap();

    let SayAnswer::Refused { why } = said else {
        panic!("a line to no agent is refused: {said:?}");
    };
    assert_eq!(why.to_string(), "no agent has the address `9`");
    realm.host.shutdown().await.unwrap();
}

/// The first line's delivery is held at the agent while the second
/// is said. The test looks for the second line at the agent for
/// 500 ms, over a hundred times what a delivery that is not held
/// takes here; the order it asserts holds however long that look
/// lasts.
#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_two_lines_to_one_agent_reach_it_in_the_order_said()
{
    let (realm, _) = hamlet_with(1).await;
    let (first, second) = (RequestId::new(), RequestId::new());
    realm.seen.hold_next(Callee::Submit, THREE);

    let (dig, rest) = (
        say(&first, THREE, "dig the well"),
        say(&second, THREE, "then rest"),
    );
    let said_first = realm.ask(&dig);
    let then_second = async {
        until("the first line waits at the agent", || {
            realm.seen.is_waiting(Callee::Submit, THREE)
        })
        .await;
        let said_second = realm.ask(&rest);
        let looked = async {
            let _ = tokio::time::timeout(
                Duration::from_millis(500),
                until("the second line reaches the agent", || {
                    !submits_to(&realm, THREE).is_empty()
                }),
            )
            .await;
            realm.seen.release(Callee::Submit);
        };
        tokio::join!(said_second, looked).0
    };
    let (first_said, second_said) =
        tokio::join!(said_first, then_second);
    until("both lines reach the agent", || {
        submits_to(&realm, THREE).len() == 2
    })
    .await;

    assert!(
        matches!(first_said.unwrap(), SayAnswer::Accepted { .. }),
        "the first line was not accepted"
    );
    assert!(
        matches!(second_said.unwrap(), SayAnswer::Accepted { .. }),
        "the second line was not accepted"
    );
    let ids = submits_to(&realm, THREE)
        .into_iter()
        .map(|submit| submit.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [first, second].map(|id| InputId(id.as_str().to_owned()))
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_held_at_one_agent_does_not_hold_a_line_to_another() {
    let (realm, _) = hamlet_with(2).await;
    realm.seen.hold_next(Callee::Submit, THREE);
    let held = say(&RequestId::new(), THREE, "dig the well");
    let other = say(&RequestId::new(), FOUR, "fetch water");

    let said_held = realm.ask(&held);
    let said_other = async {
        until("the line to hire 3 waits at it", || {
            realm.seen.is_waiting(Callee::Submit, THREE)
        })
        .await;
        let said = realm.ask(&other).await;
        until("the line to hire 4 reaches it", || {
            submits_to(&realm, FOUR).len() == 1
        })
        .await;
        let reached = submits_to(&realm, THREE).len();
        realm.seen.release(Callee::Submit);
        (said, reached)
    };
    let (said_held, (said_other, reached)) =
        tokio::join!(said_held, said_other);

    assert!(matches!(
        said_other.unwrap(),
        SayAnswer::Accepted { .. }
    ));
    assert_eq!(reached, 0);
    assert!(matches!(said_held.unwrap(), SayAnswer::Accepted { .. }));
    realm.host.shutdown().await.unwrap();
}

/// A chief's go on 3's objection tells 3 and gives the mayor the
/// letter 3 sent on that task while it objected. The notice is held
/// at 3; the test looks for the letter at the mayor for 5 s, over a
/// hundred times what a delivery that is not held takes here.
#[tokio::test(flavor = "multi_thread")]
async fn an_acts_delivery_held_at_one_agent_does_not_hold_its_delivery_to_another()
 {
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    realm
        .tool(
            THREE,
            &paid("t1", &task, tell(to(MAYOR), "it is done")),
        )
        .await
        .unwrap();
    let waits = open_waits(&realm, THREE).await;
    let before = submits_to(&realm, THREE).len();
    realm.seen.hold_next(Callee::Submit, THREE);

    realm
        .ask(&answer(&waits[0].0.to_string(), Choice::Go, None))
        .await
        .unwrap();
    until("the notice waits at 3", || {
        realm.seen.is_waiting(Callee::Submit, THREE)
    })
    .await;
    let reached = tokio::time::timeout(
        Duration::from_secs(5),
        until("the letter reaches the mayor", || {
            given(&realm, MAYOR, 0)
                .contains(&"letter: it is done".to_owned())
        }),
    )
    .await;
    realm.seen.release(Callee::Submit);

    assert!(reached.is_ok(), "the letter waited for 3's notice");
    until("the notice reaches 3", || {
        submits_to(&realm, THREE).len() > before
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_that_fails_in_passing_reaches_its_agent_before_the_next()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.fail_once(Callee::Submit, THREE);
    let (first, next) = (RequestId::new(), RequestId::new());

    let failed = realm
        .ask(&say(&first, THREE, "dig the well"))
        .await
        .unwrap();
    let taken =
        realm.ask(&say(&next, THREE, "then rest")).await.unwrap();
    until("the next line reaches the agent", || {
        submits_to(&realm, THREE).len() == 2
    })
    .await;

    assert!(
        matches!(failed, SayAnswer::Accepted { .. }),
        "{failed:?}"
    );
    assert!(matches!(taken, SayAnswer::Accepted { .. }), "{taken:?}");
    let ids = submits_to(&realm, THREE)
        .into_iter()
        .map(|submit| submit.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [first, next].map(|id| InputId(id.as_str().to_owned()))
    );
    realm.host.shutdown().await.unwrap();
}

/// The first bytes of a PNG image: its signature and the head of its
/// header chunk.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x01";

/// Keeps `bytes` in `realm`'s store as a client's upload does, and
/// answers their hash.
fn uploaded(realm: &Realm, bytes: &[u8]) -> Sha256 {
    let store = ParcelStore::at(&realm.scratch).unwrap();
    std::fs::create_dir_all(store.folder()).unwrap();
    let sha256 = hash(bytes);
    std::fs::write(store.file(&sha256), bytes).unwrap();
    sha256
}

fn file(
    sha256: &Sha256,
    name: &str,
    size: u64,
    at: Option<u64>,
) -> AttachedFile {
    AttachedFile {
        at,
        id: sha256.to_string().parse().unwrap(),
        name: name.to_owned(),
        size,
    }
}

fn image(sha256: &Sha256, at: Option<u64>) -> AttachedImage {
    AttachedImage {
        at,
        id: sha256.to_string().parse().unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_gives_its_agent_the_images_and_files_sent_before_it()
{
    let (realm, _) = hamlet_with(1).await;
    let notes = b"the mill turns at dawn";
    let png = uploaded(&realm, PNG);
    let txt = uploaded(&realm, notes);
    let mut said = say(&RequestId::new(), THREE, "see this");
    said.0.images = vec![image(&png, Some(3))];
    said.0.files = vec![file(&txt, "plans/notes.txt", 22, None)];

    let answer = realm.ask(&said).await.unwrap();
    let submits = seen_submits(&realm, THREE, 1).await;
    let Input::Line { images, files, .. } = &submits[0].input else {
        panic!("a line: {submits:?}");
    };

    assert!(
        matches!(answer, SayAnswer::Accepted { .. }),
        "{answer:?}"
    );
    let [
        LineImage {
            image: shown,
            file: kept,
        },
    ] = images.as_slice()
    else {
        panic!("one image: {images:?}");
    };
    assert_eq!(shown, &image(&png, Some(3)));
    assert_eq!(kept.path, PathBuf::from(format!("images/{png}.png")));
    assert_eq!((kept.file, kept.size), (0, PNG.len() as u64));
    let [
        LineFile {
            attached,
            file: sent,
        },
    ] = files.as_slice()
    else {
        panic!("one file: {files:?}");
    };
    assert_eq!(attached.name, "plans/notes.txt");
    assert_eq!(sent.path, PathBuf::from("plans/notes.txt"));
    assert_eq!((sent.parcel, sent.file), (kept.parcel, 1));
    assert_eq!(fetched(&realm, kept).await, PNG);
    assert_eq!(fetched(&realm, sent).await, notes);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_whose_attachments_the_store_lacks_or_misplaces_is_refused()
 {
    let (realm, _) = hamlet_with(1).await;
    let png = uploaded(&realm, PNG);
    let txt = uploaded(&realm, b"words");
    let unsent = hash(b"never sent");
    let refused = async |images, files| {
        let mut said = say(&RequestId::new(), THREE, "é");
        said.0.images = images;
        said.0.files = files;
        let answer = realm.ask(&said).await.unwrap();
        let SayAnswer::Refused { why } = answer else {
            panic!("the line is refused: {answer:?}");
        };
        why.to_string()
    };

    let words = [
        refused(vec![image(&png, Some(1))], Vec::new()).await,
        refused(Vec::new(), vec![file(&unsent, "a.txt", 1, None)])
            .await,
        refused(Vec::new(), vec![file(&txt, "b.txt", 9, None)]).await,
        refused(vec![image(&txt, None)], Vec::new()).await,
        refused(Vec::new(), vec![file(&txt, "../c.txt", 5, None)])
            .await,
    ];

    assert_eq!(
        words,
        [
            "An attachment's place, 1, is not a byte offset on a \
             character boundary of the line, which is 2 bytes."
                .to_owned(),
            "The file `a.txt` was not sent first.".to_owned(),
            "The file `b.txt` is not the size it was sent as."
                .to_owned(),
            format!("no image `{txt}` was sent first"),
            "`../c.txt` is not a name of a file.".to_owned(),
        ]
    );
    assert!(submits_to(&realm, THREE).is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_store_keeps_an_upload_no_line_carried_for_a_day() {
    let (realm, _) = hamlet_with(0).await;
    let old = uploaded(&realm, b"sent long ago");
    let fresh = uploaded(&realm, b"sent just now");
    let store = ParcelStore::at(&realm.scratch).unwrap();
    aged(&store.file(&old));

    let realm = realm.restart().await;

    assert!(!store.file(&old).exists(), "the old upload is swept");
    assert!(store.file(&fresh).exists(), "the fresh upload stays");
    realm.host.shutdown().await.unwrap();
}

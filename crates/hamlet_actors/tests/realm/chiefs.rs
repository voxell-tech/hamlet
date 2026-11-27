use std::path::{Path, PathBuf};

use hamlet_messages::address::{Agent, Party};
use hamlet_messages::realm::hamlets::{
    FoundAnswer, FoundRefusal, MoveHamlet, NameHamlet, ReadRecord,
    Record, SetFolder, SetMode,
};
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, HamletId, HamletRefusal, Seat, Seated,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::{Mode, Project, Reach};
use hamlet_protocol::client::RequestId;

use super::{
    MAYOR, Realm, THREE, found, hamlet_with, named_as, to, tool,
    until, words, world,
};

pub(super) async fn chief<T: ChiefRequest>(
    realm: &Realm,
    act: T,
) -> T::Answer {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act,
        })
        .await
        .unwrap()
}

fn set_folder(hamlet: &HamletId, folder: Option<&Path>) -> SetFolder {
    SetFolder {
        hamlet: hamlet.clone(),
        folder: folder.map(Path::to_path_buf),
    }
}

fn folder_in_place(
    realm: &Realm,
    agent: AgentNumber,
) -> Option<Option<PathBuf>> {
    let places = realm.seen.places.lock().unwrap();
    places
        .iter()
        .filter(|(sent, _)| *sent == agent)
        .max_by_key(|(_, place)| place.version)
        .map(|(_, place)| place.folder.clone())
}

fn project_in_reach(
    realm: &Realm,
    agent: AgentNumber,
) -> Option<Option<Project>> {
    let reaches = realm.seen.reaches.lock().unwrap();
    reaches
        .iter()
        .filter(|set| set.agent == agent)
        .max_by_key(|set| set.version)
        .map(|set| match &set.reach {
            Reach::Agent(footing) => footing.project.clone(),
            _ => None,
        })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hamlet_is_named_and_moved() {
    let (realm, hamlet) = hamlet_with(0).await;

    let named = chief(
        &realm,
        NameHamlet {
            hamlet: hamlet.clone(),
            name: "Weir".to_owned(),
        },
    )
    .await;
    let moved = chief(
        &realm,
        MoveHamlet {
            hamlet: hamlet.clone(),
            x: 3.0,
            z: 4.0,
        },
    )
    .await;
    let nowhere = chief(
        &realm,
        NameHamlet {
            hamlet: HamletId("x".to_owned()),
            name: "Weir".to_owned(),
        },
    )
    .await;

    assert_eq!(
        (named, moved, nowhere),
        (
            Done::Done,
            Done::Done,
            Done::Refused {
                why: HamletRefusal::NoSuchHamlet {
                    id: HamletId("x".to_owned())
                }
            }
        )
    );
    world::seen_until(&realm, "the hamlet is named Weir", |seen| {
        seen.hamlets.iter().any(|shown| {
            shown.hamlet == hamlet && shown.name == "Weir"
        })
    })
    .await;
    let (site, kinds) = realm
        .stopped_rows(|db| {
            let site = db.query_row(
                "SELECT x, z FROM hamlet WHERE id = ?1",
                [&hamlet.0],
                |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?))
                },
            )?;
            let kinds = db
                .prepare(
                    "SELECT kind FROM decision WHERE hamlet = ?1 \
                     AND by = 'chief' ORDER BY id",
                )?
                .query_map([&hamlet.0], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((site, kinds))
        })
        .await;
    assert_eq!(site, (3.0, 4.0));
    assert_eq!(
        kinds,
        ["hired", "founded", "named-hamlet", "placed-hamlet"]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_that_does_not_exist_is_refused() {
    let (realm, hamlet) = hamlet_with(0).await;
    let missing = realm.scratch.join("no-such-folder");
    let file = realm.scratch.join("a-file");
    std::fs::write(&file, "not a folder").unwrap();

    let refused =
        chief(&realm, set_folder(&hamlet, Some(&missing))).await;
    let a_file =
        chief(&realm, set_folder(&hamlet, Some(&file))).await;

    assert_eq!(
        (refused, a_file),
        (
            Done::Refused {
                why: HamletRefusal::NoSuchFolder { path: missing }
            },
            Done::Refused {
                why: HamletRefusal::NoSuchFolder { path: file }
            }
        )
    );
    let kept = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT (SELECT folder FROM hamlet WHERE id = ?1), \
                 (SELECT COUNT(*) FROM hamlet_folder)",
                [&hamlet.0],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i64>(1)?,
                    ))
                },
            )
        })
        .await;
    assert_eq!(kept, (None, 0));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_reaches_every_place_of_its_hamlet() {
    let (realm, hamlet) = hamlet_with(1).await;
    let (a, b) = (realm.scratch.join("a"), realm.scratch.join("b"));
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();

    for folder in [Some(&a), Some(&b), Some(&a), None] {
        let folder = folder.map(PathBuf::as_path);
        let set = chief(&realm, set_folder(&hamlet, folder)).await;
        assert_eq!(set, Done::Done);
        let folder = folder.map(Path::to_path_buf);
        until(&format!("the places hold {folder:?}"), || {
            [MAYOR, THREE].iter().all(|&agent| {
                folder_in_place(&realm, agent) == Some(folder.clone())
            })
        })
        .await;
    }

    let mut remembered = realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT folder FROM hamlet_folder WHERE hamlet = ?1",
            )?
            .query_map([&hamlet.0], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;
    remembered.sort();
    let shown = |path: &Path| path.to_string_lossy().into_owned();
    assert_eq!(remembered, [shown(&a), shown(&b)]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_reaches_the_places_of_the_hamlets_workers() {
    let (realm, hamlet) = hamlet_with(1).await;
    realm
        .write(&format!(
            "INSERT INTO agent (id, kind, status, at) VALUES (9, \
             'worker', 'live', 0), (10, 'worker', 'live', 0); \
             INSERT INTO worker (agent, starter, hamlet, brief, \
             tools, task, at) VALUES (9, 3, NULL, 'dig', '[]', 1, \
             0), (10, NULL, '{}', 'dig', '[]', 1, 0);",
            hamlet.0
        ))
        .await;
    for worker in [9, 10] {
        let address = Agent(AgentNumber(worker)).address().unwrap();
        realm.host.get_or_start(&address).await.unwrap();
    }
    let folder = realm.scratch.join("work");
    std::fs::create_dir(&folder).unwrap();

    let set = chief(&realm, set_folder(&hamlet, Some(&folder))).await;

    assert_eq!(set, Done::Done);
    until("the workers' places hold the folder", || {
        [AgentNumber(9), AgentNumber(10)].iter().all(|&agent| {
            folder_in_place(&realm, agent)
                == Some(Some(folder.clone()))
        })
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mode_reaches_every_agent_of_its_hamlet_and_their_workers()
{
    let (realm, hamlet) = hamlet_with(1).await;
    realm
        .write(
            "INSERT INTO agent (id, kind, status, at) VALUES (9, \
             'worker', 'live', 0); INSERT INTO worker (agent, \
             starter, brief, tools, task, at) VALUES (9, 3, 'dig', \
             '[]', 1, 0);",
        )
        .await;
    let folder = realm.scratch.join("work");
    std::fs::create_dir(&folder).unwrap();
    assert!(!realm.holds_reach(AgentNumber(9)));

    let set = chief(&realm, set_folder(&hamlet, Some(&folder))).await;
    let auto = chief(
        &realm,
        SetMode {
            hamlet: hamlet.clone(),
            mode: Mode::Auto,
        },
    )
    .await;

    assert_eq!((set, auto), (Done::Done, Done::Done));
    let project = Some(Project {
        folder,
        mode: Mode::Auto,
    });
    until("the reaches carry the folder in auto mode", || {
        [MAYOR, THREE].iter().all(|&agent| {
            project_in_reach(&realm, agent) == Some(project.clone())
        })
    })
    .await;
    realm.reach_held(AgentNumber(9)).await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_found_with_no_purpose_is_refused_to_the_hand() {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;

    let refused = realm
        .tool(Hand::NUMBER, &tool("f1", found("Pond")))
        .await
        .unwrap();

    assert_eq!(
        refused,
        FoundAnswer::Refused {
            why: FoundRefusal::NoPurpose
        }
    );
    let hamlets = realm
        .stopped_rows(|db| {
            db.query_row("SELECT COUNT(*) FROM hamlet", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .await;
    assert_eq!(hamlets, 0);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_name_is_refused_in_its_words() {
    let (realm, hamlet) = hamlet_with(0).await;

    let named = chief(
        &realm,
        NameHamlet {
            hamlet: hamlet.clone(),
            name: "Mill: east".to_owned(),
        },
    )
    .await;
    let nowhere = chief(
        &realm,
        MoveHamlet {
            hamlet: HamletId("x".to_owned()),
            x: 1.0,
            z: 2.0,
        },
    )
    .await;

    assert_eq!(
        [words(named), words(nowhere)],
        [
            "`Mill: east` is not a name: use one line of 1 to 40 \
             characters with no colon",
            "no hamlet has the id `x`",
        ]
    );
    let name = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT name FROM hamlet WHERE id = ?1",
                [&hamlet.0],
                |row| row.get::<_, String>(0),
            )
        })
        .await;
    let trimmed = chief(
        &realm,
        NameHamlet {
            hamlet: hamlet.clone(),
            name: "  Weir  ".to_owned(),
        },
    )
    .await;
    let renamed = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT name FROM hamlet WHERE id = ?1",
                [&hamlet.0],
                |row| row.get::<_, String>(0),
            )
        })
        .await;
    assert_eq!(name, "Mill");
    assert_eq!((trimmed, renamed.as_str()), (Done::Done, "Weir"));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hand_hears_the_new_mayor_and_its_hamlet() {
    let (realm, hamlet) = hamlet_with(1).await;
    let seat = Seat {
        hire: to(THREE),
        why: None,
    };

    let seated =
        realm.tool(Hand::NUMBER, &tool("s1", seat)).await.unwrap();

    assert_eq!(
        seated,
        Seated::Done {
            agent: named_as(3, "hire#3 (Hire 1)"),
            hamlet,
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_seat_records_its_reason() {
    let (realm, hamlet) = hamlet_with(1).await;

    let seated = chief(
        &realm,
        Seat {
            hire: to(THREE),
            why: Some("cy leads now".to_owned()),
        },
    )
    .await;

    assert_eq!(seated, Done::Done);
    let Record::Read { lines } = chief(
        &realm,
        ReadRecord {
            hamlet: Some(hamlet),
        },
    )
    .await
    else {
        panic!("the hamlet's record is read");
    };
    assert_eq!(
        lines.last().map(|line| line.text.as_str()),
        Some(
            "- chief gave the office to hire#3 (Hire 1): cy leads \
             now"
        )
    );
    realm.host.shutdown().await.unwrap();
}

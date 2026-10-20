//! Gift contracts beyond the protocol scenarios, using real house
//! files.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use hamlet_testing::Scratch;

use super::*;
use crate::record::{
    Actor, Admit, Change, DecisionKind, Departure, RecordOperation,
};
use crate::{Home, OperationId, Outcome, Reply, RuntimeInstance};

fn mayor() -> Address {
    Address {
        hamlet: "home".into(),
        agent: "mayor".into(),
    }
}
fn scout() -> Address {
    Address {
        hamlet: "home".into(),
        agent: "scout".into(),
    }
}

fn change(service: &mut Service, change: Change) {
    let operation = RecordOperation {
        hamlet: "home".into(),
        actor: Actor::Chief(service.chief.clone()),
        at: 100,
        reason: "Work ended".into(),
        task: None,
        change,
    };
    let result = service
        .home
        .store()
        .run(
            &OperationId {
                runtime: RuntimeInstance::new("test").unwrap(),
                key: uuid::Uuid::new_v4().to_string(),
            },
            &operation,
        )
        .unwrap();
    assert!(
        matches!(result, Reply::Outcome(Outcome::Done(_))),
        "{result:?}"
    );
}

fn setup() -> (Scratch, Service) {
    let dir =
        Scratch::new(&format!("gifts-{}", uuid::Uuid::new_v4()));
    let mut service =
        Service::new(Home::open(&dir).unwrap(), true).unwrap();
    change(
        &mut service,
        Change::Admit(Admit {
            name: "scout".into(),
            purpose: "Maps".into(),
            model: None,
            character: String::new(),
            roads: Vec::new(),
        }),
    );
    service.houses.build("home", 1).unwrap();
    service.prepare_gifts().unwrap();
    (dir, service)
}

async fn begin(service: &mut Service, to: &Address) -> String {
    match service
        .gift_begin(
            1,
            "gift",
            to,
            &[GiveFilesFiles {
                path: "note".into(),
                size: 1,
            }],
        )
        .await
        .unwrap()
        .unwrap()
    {
        Progress::Waiting(id) => id,
        _ => panic!("expected an upload"),
    }
}

fn part(upload: &str) -> GivePart {
    GivePart {
        id: "part".into(),
        upload: upload.into(),
        path: "note".into(),
        offset: 0,
        data: "QQ==".into(),
    }
}

#[tokio::test]
async fn copies_whole_skills_records_the_giver_and_keeps_modes_across_restart()
 {
    let (dir, mut service) = setup();
    let source = dir.join("houses/home/1/skills/maps");
    fs::create_dir_all(source.join("scripts/empty")).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: maps\ndescription: Read maps.\n---\n",
    )
    .unwrap();
    fs::write(source.join("scripts/run"), [0, 255, 128]).unwrap();
    fs::set_permissions(
        source.join("scripts/run"),
        fs::Permissions::from_mode(0o6751),
    )
    .unwrap();
    assert_eq!(
        service
            .gift_skill(&mayor(), "maps", Some(&scout()))
            .await
            .unwrap()
            .unwrap(),
        "Gave `mayor` the skill `maps`."
    );
    let target = dir.join("houses/home/0/skills/maps");
    assert_eq!(
        fs::read(target.join("scripts/run")).unwrap(),
        [0, 255, 128]
    );
    assert_eq!(
        fs::metadata(target.join("scripts/run"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o751
    );
    assert!(target.join("scripts/empty").is_dir());
    assert!(source.join("scripts/run").is_file());
    drop(service);
    let mut service =
        Service::new(Home::open(&dir).unwrap(), true).unwrap();
    let record =
        service.home.store().record("home").unwrap().unwrap();
    assert!(
        matches!(&record.decisions.last().unwrap().kind, DecisionKind::SkillGiven { to: 0, skill, from: Some(from) } if skill == "maps" && from == "scout")
    );
    assert_eq!(
        service
            .gift_skill(&mayor(), "maps", Some(&scout()))
            .await
            .unwrap()
            .unwrap_err()
            .text(),
        "`founder` already has a skill named `maps`"
    );
}

#[tokio::test]
async fn refuses_departed_recipients_and_a_recipient_that_leaves_during_upload()
 {
    let (dir, mut service) = setup();
    let upload = begin(&mut service, &scout()).await;
    change(
        &mut service,
        Change::Depart {
            name: "scout".into(),
            how: Departure::Removed,
        },
    );
    let farewell = format!(
        "`scout` was removed from the hamlet by the chief {} \
         (\"Work ended\"). Its house is archived and its roads went \
         with it; only a chief can restore it.",
        service.chief
    );
    assert_eq!(
        service
            .gift_skill(&scout(), "weather", None)
            .await
            .unwrap()
            .unwrap_err()
            .text(),
        farewell
    );
    let refused = service
        .gift_begin(
            1,
            "new",
            &scout(),
            &[GiveFilesFiles {
                path: "empty".into(),
                size: 0,
            }],
        )
        .await
        .unwrap();
    assert!(matches!(refused, Err(e) if e.text() == farewell));
    let refused = service.gift_part(1, &part(&upload)).await.unwrap();
    assert!(matches!(refused, Err(e) if e.text() == farewell));
    assert!(!dir.join("houses/home/1/inbox/note").exists());
    assert_eq!(fs::read_dir(dir.join(".gifts")).unwrap().count(), 0);
}

#[tokio::test]
async fn refuses_linked_receivers_and_cleans_abandoned_and_expired_uploads()
 {
    let (dir, mut service) = setup();
    let outside = Scratch::new("gift-canary");
    fs::write(outside.join("note"), "untouched").unwrap();
    let inbox = dir.join("houses/home/0/inbox");
    let upload = begin(&mut service, &mayor()).await;
    fs::remove_dir(&inbox).unwrap();
    symlink(&*outside, &inbox).unwrap();
    let outcome = service.gift_part(1, &part(&upload)).await.unwrap();
    assert!(matches!(outcome, Err(e) if e.text() == DELIVERY));
    assert_eq!(
        fs::read_to_string(outside.join("note")).unwrap(),
        "untouched"
    );
    assert!(
        service
            .gift_begin(
                1,
                "another",
                &mayor(),
                &[GiveFilesFiles {
                    path: "note".into(),
                    size: 1
                }]
            )
            .await
            .unwrap()
            .is_err()
    );
    fs::remove_file(&inbox).unwrap();
    fs::create_dir(&inbox).unwrap();
    let upload = begin(&mut service, &mayor()).await;
    service.gifts.pending.get_mut(&upload).unwrap().touched =
        Instant::now() - Duration::from_secs(61);
    assert!(
        matches!(service.gift_part(1, &part(&upload)).await.unwrap(), Err(e) if e.text() == UNKNOWN)
    );
    begin(&mut service, &mayor()).await;
    service.gifts.gone(1);
    assert_eq!(fs::read_dir(dir.join(".gifts")).unwrap().count(), 0);
    fs::create_dir_all(dir.join(".gifts/abandoned/deep")).unwrap();
    symlink(&*outside, dir.join(".gifts/abandoned/link")).unwrap();
    service.prepare_gifts().unwrap();
    assert_eq!(fs::read_dir(dir.join(".gifts")).unwrap().count(), 0);
    assert_eq!(
        fs::read_to_string(outside.join("note")).unwrap(),
        "untouched"
    );
}

#[tokio::test]
async fn refuses_unsafe_or_overfull_skills_without_partial_delivery()
{
    let (dir, mut service) = setup();
    let source = dir.join("houses/home/1/skills/maps");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "maps").unwrap();
    let outside = Scratch::new("gift-outside");
    symlink(&*outside, source.join("link")).unwrap();
    assert_eq!(
        service
            .gift_skill(&mayor(), "maps", Some(&scout()))
            .await
            .unwrap()
            .unwrap_err()
            .text(),
        "A skill gift cannot include links or special files."
    );
    fs::remove_file(source.join("link")).unwrap();
    let big = fs::File::create(source.join("large")).unwrap();
    big.set_len(MAX_BYTES + 1).unwrap();
    assert_eq!(
        service
            .gift_skill(&mayor(), "maps", Some(&scout()))
            .await
            .unwrap()
            .unwrap_err()
            .text(),
        "The skill gift is larger than 32 MB."
    );
    fs::remove_file(source.join("large")).unwrap();
    for number in 0..4095 {
        fs::create_dir(source.join(format!("folder-{number}")))
            .unwrap();
    }
    assert_eq!(
        service
            .gift_skill(&mayor(), "maps", Some(&scout()))
            .await
            .unwrap()
            .unwrap_err()
            .text(),
        "A skill gift takes at most 4096 files and folders at once."
    );
    fs::remove_dir_all(&source).unwrap();
    fs::write(outside.join("SKILL.md"), "outside").unwrap();
    symlink(&*outside, &source).unwrap();
    assert_eq!(
        service
            .gift_skill(&mayor(), "maps", Some(&scout()))
            .await
            .unwrap()
            .unwrap_err()
            .text(),
        "A skill gift cannot include links or special files."
    );
    assert_eq!(
        fs::read_to_string(outside.join("SKILL.md")).unwrap(),
        "outside"
    );
    assert!(!dir.join("houses/home/0/skills/maps").exists());
    assert_eq!(fs::read_dir(dir.join(".gifts")).unwrap().count(), 0);
    service
        .gift_skill(&mayor(), "weather", None)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn rolls_back_roots_when_numbering_would_collide_with_another_root()
 {
    let (dir, mut service) = setup();
    let inbox = dir.join("houses/home/0/inbox");
    fs::write(inbox.join("a.txt"), "earlier").unwrap();
    let files =
        ["first", "a.txt", "a-2.txt"].map(|path| GiveFilesFiles {
            path: path.into(),
            size: 0,
        });
    assert!(
        matches!(service.gift_begin(1, "gift", &mayor(), &files).await.unwrap(), Err(e) if e.text() == DELIVERY)
    );
    assert_eq!(
        fs::read_to_string(inbox.join("a.txt")).unwrap(),
        "earlier"
    );
    assert_eq!(fs::read_dir(&inbox).unwrap().count(), 1);
    assert_eq!(fs::read_dir(dir.join(".gifts")).unwrap().count(), 0);
}

#[tokio::test]
async fn refuses_malformed_parts_and_preserves_the_owners_upload_on_a_foreign_part()
 {
    let (dir, mut service) = setup();
    for data in ["", "%%", "QQ", "QR==", "QUE="] {
        let upload = begin(&mut service, &mayor()).await;
        assert!(
            matches!(service.gift_part(2, &part(&upload)).await.unwrap(), Err(e) if e.text() == UNKNOWN)
        );
        let mut bad = part(&upload);
        bad.data = data.into();
        assert!(
            matches!(service.gift_part(1, &bad).await.unwrap(), Err(e) if e.text() == MISMATCH)
        );
        assert!(
            matches!(service.gift_part(1, &part(&upload)).await.unwrap(), Err(e) if e.text() == UNKNOWN)
        );
        assert_eq!(
            fs::read_dir(dir.join(".gifts")).unwrap().count(),
            0
        );
    }
    let upload = begin(&mut service, &mayor()).await;
    assert!(
        service.gift_part(2, &part(&upload)).await.unwrap().is_err()
    );
    assert!(matches!(
        service.gift_part(1, &part(&upload)).await.unwrap(),
        Ok(Progress::Delivered { .. })
    ));
    assert_eq!(
        fs::read(dir.join("houses/home/0/inbox/note")).unwrap(),
        b"A"
    );
}

#[tokio::test]
async fn starter_files_equal_the_shipped_assets_and_ignore_library_edits()
 {
    let (dir, mut service) = setup();
    fs::write(
        dir.join("library/web-search/SKILL.md"),
        "edited library",
    )
    .unwrap();
    service
        .gift_skill(&mayor(), "web-search", None)
        .await
        .unwrap()
        .unwrap();
    for path in ["SKILL.md", "scripts/search.py"] {
        let shipped =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/starters/web-search")
                .join(path);
        assert_eq!(
            fs::read(
                dir.join("houses/home/0/skills/web-search")
                    .join(path)
            )
            .unwrap(),
            fs::read(shipped).unwrap()
        );
    }
}

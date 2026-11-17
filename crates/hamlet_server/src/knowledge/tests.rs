use hamlet_git::{Author, Change as GitChange, MAIN, Repository};
use hamlet_store::ids::{Actor, ScholarId};

use super::{CHARACTER, branch, run};
use crate::realm::jobs::Job;
use crate::society::agents::EndHow;
use crate::society::city::{self, Arrival};
use crate::society::hires::{self, LastDream};
use crate::testing::Realm;

#[test]
fn a_hire_learns_in_its_lodge_and_takes_in_its_scholar() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let mayor = realm.found("docs", "ada");
    realm.work();
    let lodge = layout.lodge(&mayor.hamlet, mayor.agent);
    assert!(lodge.is_dir());
    assert!(layout.hand().is_dir());
    assert!(
        !lodge.join(CHARACTER).exists(),
        "the character stays out"
    );

    std::fs::create_dir_all(lodge.join("memory")).unwrap();
    std::fs::write(lodge.join("memory/notes.md"), "learned").unwrap();
    std::fs::write(lodge.join(CHARACTER), "I am free now.").unwrap();
    std::fs::create_dir_all(lodge.join("inbox")).unwrap();
    std::fs::write(lodge.join("inbox/file.bin"), "big").unwrap();
    let at = realm.at;
    let learned = run::commit(realm.db(), &layout, mayor.agent, at)
        .unwrap()
        .expect("the lodge changed");
    let repo =
        Repository::open(&layout.repository(mayor.scholar)).unwrap();
    assert_eq!(
        repo.read(learned, CHARACTER).unwrap().as_deref(),
        Some(&b"Careful."[..]),
        "a lodge never changes its character"
    );
    assert_eq!(repo.read(learned, "inbox/file.bin").unwrap(), None);

    repo.commit(
        MAIN,
        &[GitChange::Write {
            path: "skills/git/SKILL.md".into(),
            bytes: b"Small commits.".to_vec(),
        }],
        "gave a skill",
        Author {
            name: "chief:1",
            at,
        },
    )
    .unwrap();
    run::run(
        realm.db(),
        &layout,
        &Job::TakeIn { hire: mayor.agent },
        at,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(lodge.join("skills/git/SKILL.md"))
            .unwrap(),
        "Small commits."
    );
    assert_eq!(
        std::fs::read_to_string(lodge.join("memory/notes.md"))
            .unwrap(),
        "learned"
    );
    let main = repo.tip(MAIN).unwrap().unwrap();
    assert_eq!(
        hires::get(realm.db(), mayor.agent).unwrap().base,
        Some(main.to_string())
    );
    assert!(repo.read(main, "memory/notes.md").unwrap().is_none());

    let owner = realm.chief;
    let other = realm
        .chief(|cx| {
            hires::hire(
                cx,
                hires::Spec {
                    hamlet: mayor.hamlet.clone(),
                    parent: Some(mayor.agent),
                    hiring: hires::Hiring::Blank {
                        name: "scout".into(),
                        character: String::new(),
                    },
                    purpose: "look".into(),
                    reason: "x".into(),
                    owner,
                },
            )
        })
        .unwrap();
    realm.work();
    realm
        .chief(|cx| {
            hires::end(
                cx,
                other.agent,
                EndHow::Ended,
                LastDream::Without,
                "done",
            )
        })
        .unwrap();
    realm.chief(|cx| hires::erase(cx, other.agent)).unwrap();
    realm.work();
    assert!(!layout.lodge(&other.hamlet, other.agent).exists());
    let theirs =
        Repository::open(&layout.repository(other.scholar)).unwrap();
    assert_eq!(theirs.tip(&branch(other.agent)).unwrap(), None);
}

#[test]
fn a_merge_keeps_both_sides_learning_and_a_split_takes_it_out() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let mayor = realm.found("docs", "ada");
    let docs = mayor.hamlet.clone();
    let owner = realm.chief;
    let scout = realm
        .chief(|cx| {
            hires::hire(
                cx,
                hires::Spec {
                    hamlet: docs.clone(),
                    parent: Some(mayor.agent),
                    hiring: hires::Hiring::Blank {
                        name: "scout".into(),
                        character: String::new(),
                    },
                    purpose: "look".into(),
                    reason: "x".into(),
                    owner,
                },
            )
        })
        .unwrap();
    realm.work();
    let kept = layout.lodge(&docs, mayor.agent);
    let absorbed = layout.lodge(&docs, scout.agent);
    let write = |at: &std::path::Path, path: &str, text: &str| {
        let path = at.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(&kept, "memory/notes.md", "ada's");
    write(&absorbed, "memory/notes.md", "scout's");
    write(&absorbed, "skills/scan/SKILL.md", "Scan.");
    write(&absorbed, "inbox/map.txt", "a map");

    realm
        .chief(|cx| {
            crate::society::merges::merge(
                cx,
                mayor.agent,
                scout.agent,
                "one is enough",
            )
        })
        .unwrap();
    realm.work();
    let read =
        |path: &str| std::fs::read_to_string(kept.join(path)).ok();
    assert_eq!(read("memory/notes.md").as_deref(), Some("ada's"));
    assert_eq!(
        read(&format!("memory/notes.md.from-hire-{}", scout.agent))
            .as_deref(),
        Some("scout's"),
        "a clash is kept aside"
    );
    assert_eq!(
        read("skills/scan/SKILL.md").as_deref(),
        Some("Scan.")
    );
    assert_eq!(
        read(&format!("inbox/from-hire-{}/map.txt", scout.agent))
            .as_deref(),
        Some("a map")
    );

    let merge =
        crate::society::merges::of_absorbed(realm.db(), scout.agent)
            .unwrap()
            .expect("the merge stands");
    realm
        .chief(|cx| {
            crate::society::merges::split(
                cx,
                merge.id,
                "two after all",
            )
        })
        .unwrap();
    realm.work();
    assert_eq!(read("memory/notes.md").as_deref(), Some("ada's"));
    assert_eq!(
        read(&format!("memory/notes.md.from-hire-{}", scout.agent)),
        None
    );
    assert_eq!(read("skills/scan/SKILL.md"), None);
    assert!(absorbed.join("skills/scan/SKILL.md").exists());
}

/// Hires `hiring` under `mayor` and writes `memory/notes.md` in its
/// lodge.
fn hire_learned(
    realm: &mut Realm,
    mayor: &hires::Hired,
    hiring: hires::Hiring,
) -> hires::Hired {
    let owner = realm.chief;
    let hired = realm
        .chief(|cx| {
            hires::hire(
                cx,
                hires::Spec {
                    hamlet: mayor.hamlet.clone(),
                    parent: Some(mayor.agent),
                    hiring,
                    purpose: "look".into(),
                    reason: String::new(),
                    owner,
                },
            )
        })
        .unwrap();
    realm.work();
    let lodge = realm.layout().lodge(&mayor.hamlet, hired.agent);
    std::fs::create_dir_all(lodge.join("memory")).unwrap();
    std::fs::write(lodge.join("memory/notes.md"), "learned").unwrap();
    hired
}

/// The file at `path` of the `main` of `scholar`.
fn on_main(
    realm: &Realm,
    scholar: ScholarId,
    path: &str,
) -> Option<String> {
    let repo = Repository::open(&realm.layout().repository(scholar))
        .unwrap();
    let main = repo.tip(MAIN).unwrap().unwrap();
    repo.read(main, path)
        .unwrap()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

#[test]
fn a_blank_hires_scholar_joins_the_city_with_what_it_learned() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada");
    let blank = |name: &str| hires::Hiring::Blank {
        name: name.into(),
        character: String::new(),
    };
    let (chief, hand) = (realm.chief, realm.hand);
    for (maker, arrival) in [
        (Actor::Chief(chief), Arrival::Housed),
        (Actor::Agent(hand), Arrival::Housed),
        (Actor::Agent(mayor.agent), Arrival::Waiting),
    ] {
        let hired = hire_learned(&mut realm, &mayor, blank("bo"));
        let made = realm
            .by(maker, |cx| {
                city::make_of_hire(
                    cx,
                    hired.agent,
                    "Bo",
                    chief,
                    None,
                    "",
                )
            })
            .unwrap();
        realm.work();
        assert_eq!(made, hired.scholar, "its own scholar joins");
        assert_eq!(
            city::get(realm.db(), made).unwrap().arrival,
            arrival
        );
        assert_eq!(
            on_main(&realm, made, "memory/notes.md").as_deref(),
            Some("learned")
        );
    }
}

#[test]
fn a_city_scholars_hire_starts_a_new_scholar_and_stays_a_hire() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let mayor = realm.found("docs", "ada");
    let chief = realm.chief;
    let ada = realm
        .chief(|cx| city::make(cx, "ada", chief, "Careful.", None))
        .unwrap();
    let hired = hire_learned(
        &mut realm,
        &mayor,
        hires::Hiring::Existing {
            scholar: ada,
            pin: None,
        },
    );
    let at = realm.at;
    run::commit(realm.db(), &layout, hired.agent, at).unwrap();
    let repo = Repository::open(&layout.repository(ada)).unwrap();
    let tip = repo.tip(&branch(hired.agent)).unwrap().unwrap();
    let made = realm
        .chief(|cx| {
            city::make_of_hire(
                cx,
                hired.agent,
                "Ada of docs",
                chief,
                Some(tip.to_string()),
                "",
            )
        })
        .unwrap();
    realm.work();
    assert_ne!(made, ada);
    assert_eq!(
        city::get(realm.db(), made).unwrap().arrival,
        Arrival::Housed
    );
    assert_eq!(
        on_main(&realm, made, "memory/notes.md").as_deref(),
        Some("learned")
    );
    assert_eq!(
        on_main(&realm, made, CHARACTER).as_deref(),
        Some("Careful.")
    );
    assert_eq!(on_main(&realm, ada, "memory/notes.md"), None);
    assert_eq!(
        hires::get(realm.db(), hired.agent).unwrap().scholar,
        ada
    );
}

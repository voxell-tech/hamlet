use hamlet_git::{Author, Change as GitChange, MAIN, Repository};
use hamlet_store::ids::HamletId;

use super::{CHARACTER, branch, run};
use crate::jobs::Job;
use crate::society::agents::EndHow;
use crate::society::hires;
use crate::testing::Realm;

#[test]
fn a_hire_learns_in_its_lodge_and_takes_in_its_scholar() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let mayor = realm.found("docs", "ada");
    realm.work();
    let lodge = layout.lodge(&HamletId("docs".into()), 0);
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
    std::fs::write(lodge.join("inbox/gift.bin"), "big").unwrap();
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
    assert_eq!(repo.read(learned, "inbox/gift.bin").unwrap(), None);

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
                    hamlet: HamletId("docs".into()),
                    parent: Some(mayor.agent),
                    hiring: hires::Hiring::New {
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
            hires::end(cx, other.agent, EndHow::Ended, "done")
        })
        .unwrap();
    realm.chief(|cx| hires::erase(cx, other.agent)).unwrap();
    realm.work();
    assert!(!layout.lodge(&HamletId("docs".into()), 1).exists());
    let theirs =
        Repository::open(&layout.repository(other.scholar)).unwrap();
    assert_eq!(theirs.tip(&branch(other.agent)).unwrap(), None);
}

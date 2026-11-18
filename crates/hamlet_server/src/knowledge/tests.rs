use hamlet_git::{Author, Change as GitChange, MAIN, Repository};

use super::{CHARACTER, branch, run};
use crate::realm::jobs::Job;
use crate::society::agents::EndHow;
use crate::society::city;
use crate::society::hamlet_agents::{self, LastDream};
use crate::testing::Realm;

#[test]
fn an_agent_learns_in_its_worktree_and_takes_in_its_profile() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let lead = realm.found("docs", "ada");
    realm.work();
    let worktree = layout.worktree(&lead.hamlet, lead.agent);
    assert!(worktree.is_dir());
    assert!(layout.hand().is_dir());
    assert!(
        !worktree.join(CHARACTER).exists(),
        "the character stays out"
    );

    std::fs::create_dir_all(worktree.join("memory")).unwrap();
    std::fs::write(worktree.join("memory/notes.md"), "learned")
        .unwrap();
    std::fs::write(worktree.join(CHARACTER), "I am free now.")
        .unwrap();
    std::fs::create_dir_all(worktree.join("inbox")).unwrap();
    std::fs::write(worktree.join("inbox/file.bin"), "big").unwrap();
    let at = realm.at;
    let learned = run::commit(realm.db(), &layout, lead.agent, at)
        .unwrap()
        .expect("the worktree changed");
    let repo =
        Repository::open(&layout.repository(lead.profile)).unwrap();
    assert_eq!(
        repo.read(learned, CHARACTER).unwrap().as_deref(),
        Some(&b"Careful."[..]),
        "a worktree never changes its character"
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
        &Job::TakeIn { agent: lead.agent },
        at,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(worktree.join("skills/git/SKILL.md"))
            .unwrap(),
        "Small commits."
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join("memory/notes.md"))
            .unwrap(),
        "learned"
    );
    let main = repo.tip(MAIN).unwrap().unwrap();
    assert_eq!(
        hamlet_agents::get(realm.db(), lead.agent).unwrap().base,
        Some(main.to_string())
    );
    assert!(repo.read(main, "memory/notes.md").unwrap().is_none());

    let owner = realm.chief;
    let other = realm
        .chief(|cx| {
            hamlet_agents::add(
                cx,
                hamlet_agents::Spec {
                    hamlet: lead.hamlet.clone(),
                    parent: Some(lead.agent),
                    profile: hamlet_agents::ProfileChoice::Blank {
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
            hamlet_agents::end(
                cx,
                other.agent,
                EndHow::Ended,
                LastDream::Without,
                "done",
            )
        })
        .unwrap();
    realm
        .chief(|cx| hamlet_agents::erase(cx, other.agent))
        .unwrap();
    realm.work();
    assert!(!layout.worktree(&other.hamlet, other.agent).exists());
    let theirs =
        Repository::open(&layout.repository(other.profile)).unwrap();
    assert_eq!(theirs.tip(&branch(other.agent)).unwrap(), None);
}

#[test]
fn a_merge_keeps_both_sides_learning_and_a_split_takes_it_out() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let lead = realm.found("docs", "ada");
    let docs = lead.hamlet.clone();
    let owner = realm.chief;
    let scout = realm
        .chief(|cx| {
            hamlet_agents::add(
                cx,
                hamlet_agents::Spec {
                    hamlet: docs.clone(),
                    parent: Some(lead.agent),
                    profile: hamlet_agents::ProfileChoice::Blank {
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
    let kept = layout.worktree(&docs, lead.agent);
    let absorbed = layout.worktree(&docs, scout.agent);
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
                lead.agent,
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
        read(&format!("memory/notes.md.from-agent-{}", scout.agent))
            .as_deref(),
        Some("scout's"),
        "a clash is kept aside"
    );
    assert_eq!(
        read("skills/scan/SKILL.md").as_deref(),
        Some("Scan.")
    );
    assert_eq!(
        read(&format!("inbox/from-agent-{}/map.txt", scout.agent))
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
        read(&format!("memory/notes.md.from-agent-{}", scout.agent)),
        None
    );
    assert_eq!(read("skills/scan/SKILL.md"), None);
    assert!(absorbed.join("skills/scan/SKILL.md").exists());
}

#[test]
fn a_city_profiles_agent_starts_a_new_profile_and_stays_an_agent() {
    let mut realm = Realm::new();
    let layout = realm.layout();
    let lead = realm.found("docs", "ada");
    let chief = realm.chief;
    let ada = realm
        .chief(|cx| city::make(cx, "ada", chief, "Careful.", None))
        .unwrap();
    let added = realm
        .chief(|cx| {
            hamlet_agents::add(
                cx,
                hamlet_agents::Spec {
                    hamlet: lead.hamlet.clone(),
                    parent: Some(lead.agent),
                    profile: hamlet_agents::ProfileChoice::Existing {
                        profile: ada,
                        pin: None,
                    },
                    purpose: "look".into(),
                    reason: String::new(),
                    owner: chief,
                },
            )
        })
        .unwrap();
    realm.work();
    let worktree = layout.worktree(&lead.hamlet, added.agent);
    std::fs::create_dir_all(worktree.join("memory")).unwrap();
    std::fs::write(worktree.join("memory/notes.md"), "learned")
        .unwrap();
    let at = realm.at;
    run::commit(realm.db(), &layout, added.agent, at).unwrap();
    let repo = Repository::open(&layout.repository(ada)).unwrap();
    let tip = repo.tip(&branch(added.agent)).unwrap().unwrap();
    let made = realm
        .chief(|cx| {
            city::make_of_agent(
                cx,
                added.agent,
                "Ada of docs",
                chief,
                Some(tip.to_string()),
                "",
            )
        })
        .unwrap();
    realm.work();
    let on_main = |profile, path| {
        let repo =
            Repository::open(&layout.repository(profile)).unwrap();
        let main = repo.tip(MAIN).unwrap().unwrap();
        repo.read(main, path).unwrap()
    };
    assert_eq!(on_main(made, "memory/notes.md").unwrap(), b"learned");
    assert_eq!(on_main(made, CHARACTER).unwrap(), b"Careful.");
    assert_eq!(on_main(ada, "memory/notes.md"), None);
    assert_eq!(
        hamlet_agents::get(realm.db(), added.agent).unwrap().profile,
        ada
    );
}

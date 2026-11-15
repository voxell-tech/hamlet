//! Dreams through a host: the realm prepares one, the test writes in
//! its folders as the dream's agent would, and the realm ends it.

use hamlet_messages::profile::{
    DreamEnd, DreamId, DreamOf, DreamOpen, EndDream, Erase, Pin,
    PrepareDream, Prepared, Purge, PurgeAnswer,
};
use hamlet_messages::realm::Done;

use super::*;

fn prepare(dream: u32, of: DreamOf) -> PrepareDream {
    PrepareDream {
        dream: DreamId(dream),
        of,
    }
}

fn end(dream: u32, done: bool) -> EndDream {
    EndDream {
        dream: DreamId(dream),
        done,
        summary: "tidied".into(),
    }
}

fn hire_7() -> DreamOf {
    DreamOf::Hire {
        agent: AgentNumber(7),
    }
}

/// The commits by `by` on `branch`'s first parents.
fn made_by(dir: &Path, branch: &str, by: &str) -> usize {
    let tip = tip(dir, branch).unwrap();
    let commits = repository(dir, 1).first_parents(tip, None, 100);
    commits.unwrap().iter().filter(|c| c.author == by).count()
}

fn read(dir: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(path)).ok()
}

fn commit_on(dir: &Path, branch: &str, changes: &[Change]) {
    let by = hamlet_git::Author {
        name: "agent:7",
        at: 5000,
    };
    repository(dir, 1)
        .commit(branch, changes, "saved", by)
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_s_dream_brings_its_learning_home_and_keeps_its_side()
{
    let dir = Scratch::new("profile-dream-hire");
    let told = Arc::new(Told::default());
    let shared = write("memory/shared.md", "base\n");
    let host = hired(&dir, &[shared], told.clone()).await;
    host.get_or_start(&agent()).await.unwrap();
    commit_on(&dir, "hires/7", &[write("learned.md", "learned")]);

    let asked = prepare(5, hire_7());
    let ready = call_as(&host, &realm(), &asked).await.unwrap();
    assert_eq!(
        ready,
        Prepared::Ready {
            main: "dreams/5/main".into(),
            hire: Some("dreams/5/hire".into()),
        }
    );
    assert_eq!(
        read(&dir, "dreams/5/main/memory/shared.md"),
        Some("base\n".into())
    );
    assert_eq!(read(&dir, "dreams/5/main/CHARACTER.md"), None);
    assert_eq!(
        read(&dir, "dreams/5/hire/learned.md"),
        Some("learned".into())
    );
    assert_eq!(
        call_as(&host, &realm(), &asked).await.unwrap(),
        ready
    );

    // The dream's agent: the learning goes home, and both sides
    // change the shared notes.
    let checkouts = dir.join("dreams/5");
    std::fs::write(
        checkouts.join("main/memory/learned.md"),
        "learned",
    )
    .unwrap();
    std::fs::write(
        checkouts.join("main/memory/shared.md"),
        "main side\n",
    )
    .unwrap();
    std::fs::remove_file(checkouts.join("hire/learned.md")).unwrap();
    std::fs::write(
        checkouts.join("hire/memory/shared.md"),
        "hire side\n",
    )
    .unwrap();
    // A chief's edit made while the dream runs.
    let edit = hamlet_messages::profile::Edit {
        id: RequestId::new(),
        path: "memory/edited.md".into(),
        text: "edited".into(),
        by: By::Chief { chief: 3 },
    };
    call_as(&host, &realm(), &edit).await.unwrap();

    let ended =
        call_as(&host, &realm(), &end(5, true)).await.unwrap();
    let main = main_tip(&dir);
    assert_eq!(
        ended,
        DreamEnd::Dreamt {
            main: Some(commit_id(main))
        }
    );
    assert_eq!(made_by(&dir, MAIN, "dream:5"), 1);
    let at = commit_id(main);
    assert_eq!(
        text(&dir, at, "memory/learned.md").as_deref(),
        Some("learned")
    );
    assert_eq!(
        text(&dir, at, "memory/shared.md").as_deref(),
        Some("main side\n")
    );
    assert_eq!(
        text(&dir, at, "memory/edited.md").as_deref(),
        Some("edited")
    );
    assert_eq!(text(&dir, at, "learned.md"), None);

    let hire = tip(&dir, "hires/7").unwrap();
    assert!(
        repository(&dir, 1).merge_base(hire, main).unwrap()
            == Some(main)
    );
    let at = commit_id(hire);
    assert_eq!(
        text(&dir, at, "memory/shared.md").as_deref(),
        Some("hire side\n")
    );
    assert_eq!(
        text(&dir, at, "memory/learned.md").as_deref(),
        Some("learned")
    );
    assert_eq!(text(&dir, at, "learned.md"), None);
    assert!(!checkouts.exists());
    told_at(&told, 7, hire).await;

    let again =
        call_as(&host, &realm(), &end(5, true)).await.unwrap();
    assert_eq!(again, ended);
    assert_eq!(main_tip(&dir), main);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_s_dream_is_one_commit_on_main_taken_in_by_unpinned_hires()
 {
    let dir = Scratch::new("profile-dream-profile");
    let host = hired(&dir, &[], Arc::default()).await;
    hire(&host, 8).await;
    call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
        .await
        .unwrap();
    let pin = Pin {
        id: RequestId::new(),
        hire: AgentNumber(8),
        to: Some(VersionName("v1".into())),
        by: By::Chief { chief: 3 },
    };
    call_as(&host, &realm(), &pin).await.unwrap();
    let pinned = tip(&dir, "hires/8");

    let ready =
        call_as(&host, &realm(), &prepare(6, DreamOf::Profile))
            .await
            .unwrap();
    assert_eq!(
        ready,
        Prepared::Ready {
            main: "dreams/6/main".into(),
            hire: None,
        }
    );
    std::fs::write(dir.join("dreams/6/main/tidy.md"), "tidy")
        .unwrap();
    let ended =
        call_as(&host, &realm(), &end(6, true)).await.unwrap();
    let main = main_tip(&dir);
    assert_eq!(
        ended,
        DreamEnd::Dreamt {
            main: Some(commit_id(main))
        }
    );
    assert_eq!(made_by(&dir, MAIN, "dream:6"), 1);
    assert_eq!(tip(&dir, "hires/7"), Some(main));
    assert_eq!(tip(&dir, "hires/8"), pinned);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dream_not_done_commits_nothing_and_removes_its_folders() {
    let dir = Scratch::new("profile-dream-undone");
    let host = hired(&dir, &[], Arc::default()).await;
    commit_on(&dir, "hires/7", &[write("learned.md", "learned")]);
    let hire = tip(&dir, "hires/7");
    let main = main_tip(&dir);
    call_as(&host, &realm(), &prepare(7, hire_7()))
        .await
        .unwrap();
    std::fs::write(dir.join("dreams/7/main/new.md"), "new").unwrap();

    let ended =
        call_as(&host, &realm(), &end(7, false)).await.unwrap();
    assert_eq!(ended, DreamEnd::Nothing);
    assert_eq!(main_tip(&dir), main);
    assert_eq!(tip(&dir, "hires/7"), hire);
    assert!(!dir.join("dreams/7").exists());
    let again =
        call_as(&host, &realm(), &end(7, true)).await.unwrap();
    assert_eq!(again, DreamEnd::Nothing);
    assert_eq!(main_tip(&dir), main);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_branch_main_holds_has_nothing_to_dream() {
    let dir = Scratch::new("profile-dream-nothing");
    let host = hired(&dir, &[], Arc::default()).await;
    let asked = prepare(8, hire_7());
    let nothing = call_as(&host, &realm(), &asked).await.unwrap();
    assert_eq!(nothing, Prepared::Nothing);
    assert!(!dir.join("dreams/8").exists());
    assert_eq!(
        call_as(&host, &realm(), &asked).await.unwrap(),
        nothing
    );
    let ended =
        call_as(&host, &realm(), &end(8, true)).await.unwrap();
    assert_eq!(ended, DreamEnd::Nothing);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_purge_and_an_erase_wait_for_an_open_dream() {
    let dir = Scratch::new("profile-dream-open");
    let host = made(&dir).await;
    call_as(&host, &realm(), &prepare(9, DreamOf::Profile))
        .await
        .unwrap();
    let dreaming = DreamOpen::Dreaming { dream: DreamId(9) };
    let purge = Purge {
        id: RequestId::new(),
        path: "CHARACTER.md".into(),
        by: By::Chief { chief: 3 },
    };
    assert_eq!(
        call_as(&host, &realm(), &purge).await.unwrap(),
        PurgeAnswer::Refused {
            why: dreaming.clone().into()
        }
    );
    let erase = Erase {
        id: RequestId::new(),
    };
    assert_eq!(
        call_as(&host, &realm(), &erase).await.unwrap(),
        Done::Refused { why: dreaming }
    );
    assert!(dir.join("scholars/1.git").exists());

    call_as(&host, &realm(), &end(9, false)).await.unwrap();
    let erase = Erase {
        id: RequestId::new(),
    };
    assert_eq!(
        call_as(&host, &realm(), &erase).await.unwrap(),
        Done::Done
    );
    host.shutdown().await.unwrap();
}

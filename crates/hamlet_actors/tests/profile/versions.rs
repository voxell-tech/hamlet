//! A profile's history through a host: the realm's reverts, edits,
//! purges, reads of its papers, and its erase.

use hamlet_messages::profile::{
    BadCommit, BadPath, Edit, EditRefusal, Erase, MOST_READ, Missing,
    Paper, PaperList, Papers, Pin, Protected, Purge, PurgeAnswer,
    Read, ReadAnswer, Revert, RevertAnswer, RevertRefusal, Revision,
    TooLong,
};
use hamlet_messages::realm::Done;

use super::*;

fn chief() -> By {
    By::Chief { chief: 3 }
}

fn revert(id: &RequestId, what: Revision) -> Revert {
    Revert {
        id: id.clone(),
        what,
        reason: None,
        by: chief(),
    }
}

fn of_commit(commit: hamlet_git::CommitId) -> Revision {
    Revision::Commit {
        commit: commit_id(commit),
    }
}

fn edit(id: &RequestId, path: &str, text: &str) -> Edit {
    Edit {
        id: id.clone(),
        path: path.into(),
        text: text.into(),
        message: None,
        by: chief(),
    }
}

async fn edited(host: &Host, path: &str, text: &str) {
    let asked = edit(&RequestId::new(), path, text);
    let done = call_as(host, &realm(), &asked).await.unwrap();
    assert_eq!(done, Done::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revert_puts_main_back_and_a_hire_that_follows_takes_it_in()
{
    let dir = Scratch::new("profile-revert");
    let told = Arc::new(Told::default());
    let notes = write("memory/notes.md", "one\n");
    let host = hired(&dir, &[notes], told.clone()).await;
    host.get_or_start(&agent()).await.unwrap();
    let tagged =
        call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
            .await
            .unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }));
    edited(&host, "memory/notes.md", "two\n").await;
    let undone = main_tip(&dir);
    edited(&host, "memory/later.md", "later").await;

    let id = RequestId::new();
    let reverted =
        call_as(&host, &realm(), &revert(&id, of_commit(undone)))
            .await
            .unwrap();
    let RevertAnswer::Reverted { commit, clashes } = reverted.clone()
    else {
        panic!("{reverted:?}");
    };
    assert!(clashes.is_empty());
    let main = main_tip(&dir);
    assert_eq!(commit_id(main), commit);
    assert_eq!(
        text(&dir, commit, "memory/notes.md").as_deref(),
        Some("one\n")
    );
    assert_eq!(
        text(&dir, commit, "memory/later.md").as_deref(),
        Some("later")
    );
    assert_eq!(tip(&dir, "hires/7"), Some(main));
    told_at(&told, 7, main).await;

    let again =
        call_as(&host, &realm(), &revert(&id, of_commit(undone)))
            .await
            .unwrap();
    assert_eq!(again, reverted);
    assert_eq!(main_tip(&dir), main);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revert_is_refused_a_commit_main_does_not_hold() {
    let dir = Scratch::new("profile-revert-refused");
    let host = hired(&dir, &[], Arc::default()).await;
    let main = main_tip(&dir);
    let by = hamlet_git::Author {
        name: "agent:7",
        at: 5000,
    };
    let on_hire = repository(&dir, 1)
        .commit("hires/7", &[write("a.md", "a")], "saved", by)
        .unwrap();
    let unknown = CommitId(Sha1([1; 20]));
    let v9 = VersionName("v9".into());
    let cases = [
        (
            Revision::Version {
                version: v9.clone(),
            },
            RevertRefusal::from(Missing::NoVersion {
                profile: ProfileId(1),
                version: v9,
            }),
        ),
        (
            Revision::Commit { commit: unknown },
            BadCommit::NoCommit {
                profile: ProfileId(1),
                commit: unknown,
            }
            .into(),
        ),
        (
            of_commit(on_hire),
            BadCommit::NotOnMain {
                commit: commit_id(on_hire),
            }
            .into(),
        ),
    ];
    for (what, why) in cases {
        let asked = revert(&RequestId::new(), what);
        let answer = call_as(&host, &realm(), &asked).await.unwrap();
        assert_eq!(answer, RevertAnswer::Refused { why });
    }
    assert_eq!(main_tip(&dir), main);
    let words = BadCommit::NoCommit {
        profile: ProfileId(1),
        commit: unknown,
    }
    .to_string();
    assert_eq!(
        words,
        format!("`scholar#1` has no version `{}`", unknown.0)
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revert_cut_off_before_its_marker_is_still_taken_in() {
    let dir = Scratch::new("profile-revert-cut");
    let told = Arc::new(Told::default());
    let host = hired(&dir, &[], told.clone()).await;
    edited(&host, "memory/notes.md", "notes").await;
    let undone = main_tip(&dir);
    host.shutdown().await.unwrap();
    drop(host);
    // Left by a kill after the revert's commit and before its
    // marker: `main` reverted, the hire not yet moved.
    let by = hamlet_git::Author {
        name: "chief:3",
        at: 6000,
    };
    let reverted = repository(&dir, 1)
        .revert(MAIN, undone, "Reverted", by)
        .unwrap()
        .commit;
    assert_eq!(tip(&dir, "hires/7"), Some(undone));

    let host = open_with(&dir, told.clone());
    host.get_or_start(&agent()).await.unwrap();
    let asked = revert(&RequestId::new(), of_commit(undone));
    let answer = call_as(&host, &realm(), &asked).await.unwrap();
    assert_eq!(answer, RevertAnswer::Unchanged);
    assert_eq!(main_tip(&dir), reverted);
    assert_eq!(tip(&dir, "hires/7"), Some(reverted));
    told_at(&told, 7, reverted).await;
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_commits_once_on_main_and_refuses_what_it_may_not_write()
 {
    let dir = Scratch::new("profile-edit");
    let told = Arc::new(Told::default());
    let host = hired(&dir, &[], told.clone()).await;
    host.get_or_start(&agent()).await.unwrap();
    let first = main_tip(&dir);

    let id = RequestId::new();
    let bolder = edit(&id, "CHARACTER.md", "A bolder reader.");
    let done = call_as(&host, &realm(), &bolder).await.unwrap();
    assert_eq!(done, Done::Done);
    let main = main_tip(&dir);
    let made = repository(&dir, 1)
        .first_parents(main, Some(first), 10)
        .unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].author, "chief:3");
    assert_eq!(
        text(&dir, commit_id(main), "CHARACTER.md").as_deref(),
        Some("A bolder reader.")
    );
    assert_eq!(
        call_as(&host, &realm(), &bolder).await.unwrap(),
        done
    );
    assert_eq!(main_tip(&dir), main);
    assert_eq!(tip(&dir, "hires/7"), Some(main));
    assert_eq!(
        moved_to(&told, 7).await.character,
        Some(Character {
            version: 2,
            text: "A bolder reader.".into()
        })
    );

    edited(&host, "memory/deep/notes.md", "notes").await;
    let main = main_tip(&dir);
    assert_eq!(
        text(&dir, commit_id(main), "memory/deep/notes.md")
            .as_deref(),
        Some("notes")
    );

    let path = |path: &str| path.to_owned();
    let cases = [
        (
            edit(&RequestId::new(), "../a.md", "a"),
            EditRefusal::from(BadPath::NotAPath {
                path: path("../a.md"),
            }),
        ),
        (
            edit(&RequestId::new(), "notes.md", "a"),
            Protected::Withheld {
                path: path("notes.md"),
            }
            .into(),
        ),
        (
            edit(&RequestId::new(), "inbox/a.md", "a"),
            Protected::Withheld {
                path: path("inbox/a.md"),
            }
            .into(),
        ),
        (
            edit(
                &RequestId::new(),
                "CHARACTER.md",
                &"a".repeat(4001),
            ),
            TooLong::CharacterTooLong { most: 4000 }.into(),
        ),
    ];
    for (asked, why) in cases {
        let answer = call_as(&host, &realm(), &asked).await.unwrap();
        assert_eq!(answer, Done::Refused { why });
    }
    assert_eq!(main_tip(&dir), main);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_s_edits_and_reverts_carry_today_s_words() {
    let dir = Scratch::new("profile-edit-words");
    let host = made(&dir).await;
    let chief = || "chief:3".to_owned();
    edited(&host, "CHARACTER.md", "A bolder reader.").await;
    assert_eq!(
        last_words(&dir, 1, MAIN),
        ("Edited the character".to_owned(), chief())
    );
    let bolder = main_tip(&dir);
    edited(&host, "memory/a.md", "a").await;
    assert_eq!(
        last_words(&dir, 1, MAIN),
        ("Edited the memory".to_owned(), chief())
    );

    edited(&host, "memory/a.md", "").await;
    let main = main_tip(&dir);
    assert_eq!(text(&dir, commit_id(main), "memory/a.md"), None);
    assert_eq!(
        last_words(&dir, 1, MAIN),
        ("Edited the memory".to_owned(), chief())
    );
    edited(&host, "memory/never.md", "").await;

    let short = &bolder.to_string()[..8];
    let asked = revert(&RequestId::new(), of_commit(bolder));
    let answer = call_as(&host, &realm(), &asked).await.unwrap();
    assert!(matches!(answer, RevertAnswer::Reverted { .. }));
    assert_eq!(
        last_words(&dir, 1, MAIN),
        (format!("Reverted {short}:"), chief())
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hand_s_revert_carries_today_s_words() {
    let dir = Scratch::new("profile-revert-hand");
    let host = made(&dir).await;
    edited(&host, "memory/a.md", "a").await;
    let asked = Revert {
        reason: Some("undo it".into()),
        by: By::Agent {
            agent: AgentNumber(1),
        },
        ..revert(&RequestId::new(), of_commit(main_tip(&dir)))
    };
    let answer = call_as(&host, &realm(), &asked).await.unwrap();
    assert!(matches!(answer, RevertAnswer::Reverted { .. }));
    assert_eq!(
        last_words(&dir, 1, MAIN),
        ("Reverted: undo it".to_owned(), "agent:1".to_owned())
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hand_s_edit_writes_any_file_and_empty_text_removes_it() {
    let dir = Scratch::new("profile-edit-hand");
    let host = made(&dir).await;
    let hand = |path: &str, text: &str| Edit {
        by: By::Agent {
            agent: AgentNumber(1),
        },
        ..edit(&RequestId::new(), path, text)
    };
    let words =
        ("Edited by the Hand".to_owned(), "agent:1".to_owned());
    for path in ["skills/tidy/SKILL.md", "notes.md"] {
        let asked = hand(path, "written");
        let done = call_as(&host, &realm(), &asked).await.unwrap();
        assert_eq!(done, Done::Done);
        let main = commit_id(main_tip(&dir));
        assert_eq!(
            text(&dir, main, path).as_deref(),
            Some("written")
        );
        assert_eq!(last_words(&dir, 1, MAIN), words);
    }
    let asked = Edit {
        message: Some(String::new()),
        ..hand("notes.md", "again")
    };
    call_as(&host, &realm(), &asked).await.unwrap();
    assert_eq!(last_words(&dir, 1, MAIN), words);
    let asked = Edit {
        message: Some("Took out the tidy skill".into()),
        ..hand("skills/tidy/SKILL.md", "")
    };
    call_as(&host, &realm(), &asked).await.unwrap();
    let main = commit_id(main_tip(&dir));
    assert_eq!(text(&dir, main, "skills/tidy/SKILL.md"), None);
    assert_eq!(
        last_words(&dir, 1, MAIN),
        ("Took out the tidy skill".to_owned(), "agent:1".to_owned())
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_purge_takes_a_path_out_of_every_branch_and_version() {
    let dir = Scratch::new("profile-purge");
    let told = Arc::new(Told::default());
    let files = [
        write("secret.md", "secret"),
        write("memory/kept.md", "kept"),
    ];
    let host = hired(&dir, &files, told.clone()).await;
    for number in [8, 9] {
        hire(&host, number).await;
    }
    for number in [7, 8, 9] {
        host.get_or_start(&agent_at(number)).await.unwrap();
    }
    let tagged =
        call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
            .await
            .unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }));
    let pin = Pin {
        id: RequestId::new(),
        hire: AgentNumber(8),
        to: Some(VersionName("v1".into())),
        by: chief(),
    };
    call_as(&host, &realm(), &pin).await.unwrap();
    edited(&host, "memory/later.md", "later").await;
    told.0.lock().unwrap().clear();

    let id = RequestId::new();
    let purge = Purge {
        id: id.clone(),
        path: "secret.md".into(),
        by: chief(),
    };
    let purged = call_as(&host, &realm(), &purge).await.unwrap();
    assert_eq!(
        purged,
        PurgeAnswer::Purged {
            branches: 4,
            still_hold: vec![AgentNumber(8), AgentNumber(9)],
        }
    );
    let repository = repository(&dir, 1);
    let branches = repository.branches().unwrap();
    let versions = repository.versions().unwrap();
    let tips = branches.iter().map(|(_, tip)| *tip);
    for at in tips.chain(versions.iter().map(|v| v.commit)) {
        for commit in repository.history(at, 100).unwrap() {
            let paths = repository.entries(commit.id).unwrap();
            assert!(paths.iter().all(|e| e.path != "secret.md"));
        }
        assert!(
            repository.read(at, "memory/kept.md").unwrap().is_some()
        );
    }
    assert_eq!(tip(&dir, "hires/8"), Some(versions[0].commit));
    for (branch, tip) in &branches {
        if let Some(number) = branch.strip_prefix("hires/") {
            told_at(&told, number.parse().unwrap(), *tip).await;
        }
    }

    let again = call_as(&host, &realm(), &purge).await.unwrap();
    assert_eq!(again, purged);
    let refused = Purge {
        id: RequestId::new(),
        path: "../secret.md".into(),
        by: chief(),
    };
    assert_eq!(
        call_as(&host, &realm(), &refused).await.unwrap(),
        PurgeAnswer::Refused {
            why: BadPath::NotAPath {
                path: "../secret.md".into()
            }
            .into()
        }
    );
    host.shutdown().await.unwrap();
    drop(host);

    let store = rusqlite::Connection::open(
        dir.join("actors/profile/1.sqlite"),
    )
    .unwrap();
    let pinned: String = store
        .query_row(
            "SELECT pin_commit FROM hire WHERE agent = 8",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pinned, versions[0].commit.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn papers_are_the_character_memory_and_skills_at_main() {
    let dir = Scratch::new("profile-papers");
    let host = made(&dir).await;
    commit_main(
        &dir,
        &[
            write(
                "memory/tests.md",
                "---\ndescription: How the tests run\n---\nRun them.",
            ),
            write(
                "skills/fixtures/SKILL.md",
                "---\nname: fixtures\ndescription: Writes \
                 fixtures\n---\n",
            ),
            write(
                "skills/fixtures/notes.md",
                "not a skill's own file",
            ),
            write("other.md", "neither"),
        ],
    );
    let papers = call_as(&host, &realm(), &Papers).await.unwrap();
    assert_eq!(
        papers,
        PaperList {
            character: CHARACTER.into(),
            memory: vec![Paper {
                path: "memory/tests.md".into(),
                description: "How the tests run".into(),
            }],
            skills: vec![Paper {
                path: "skills/fixtures/SKILL.md".into(),
                description: "Writes fixtures".into(),
            }],
        }
    );
    host.shutdown().await.unwrap();
}

fn read(at: Option<Revision>, path: Option<&str>) -> Read {
    Read {
        at,
        path: path.map(str::to_owned),
    }
}

fn listed(paths: &[&str]) -> ReadAnswer {
    ReadAnswer::Listed {
        paths: paths
            .iter()
            .map(|path| hamlet_messages::profile::Path {
                path: (*path).to_owned(),
            })
            .collect(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_lists_the_files_of_a_revision_or_gives_one_file() {
    let dir = Scratch::new("profile-read");
    let host = made(&dir).await;
    let first = main_tip(&dir);
    commit_main(&dir, &[write("memory/b.md", "b")]);
    let tagged =
        call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
            .await
            .unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }));
    commit_main(
        &dir,
        &[
            write("inbox/a.md", "a"),
            write("memory/a.md", "the notes"),
        ],
    );

    let at_main = call_as(&host, &realm(), &read(None, None)).await;
    assert_eq!(
        at_main.unwrap(),
        listed(&[
            "CHARACTER.md",
            "inbox/a.md",
            "memory/a.md",
            "memory/b.md"
        ])
    );
    let v1 = Revision::Version {
        version: VersionName("v1".into()),
    };
    let at_v1 = call_as(&host, &realm(), &read(Some(v1), None)).await;
    assert_eq!(
        at_v1.unwrap(),
        listed(&["CHARACTER.md", "memory/b.md"])
    );
    let at_first =
        call_as(&host, &realm(), &read(Some(of_commit(first)), None))
            .await;
    assert_eq!(at_first.unwrap(), listed(&["CHARACTER.md"]));
    let notes =
        call_as(&host, &realm(), &read(None, Some("memory/a.md")))
            .await;
    assert_eq!(
        notes.unwrap(),
        ReadAnswer::Text {
            text: "the notes".into(),
            cut: false
        }
    );
    let v1 = Revision::Version {
        version: VersionName("v1".into()),
    };
    let gone = read(Some(v1), Some("memory/a.md"));
    assert_eq!(
        call_as(&host, &realm(), &gone).await.unwrap(),
        ReadAnswer::Refused {
            why: BadPath::NotAPath {
                path: "memory/a.md".into()
            }
            .into()
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_cuts_a_long_file_at_a_character_boundary() {
    let dir = Scratch::new("profile-read-cut");
    let host = made(&dir).await;
    let most = MOST_READ as usize;
    let whole = "a".repeat(most);
    let long = format!("{}é", "a".repeat(most - 1));
    commit_main(
        &dir,
        &[
            write("memory/whole.md", whole.clone()),
            write("memory/long.md", long),
        ],
    );
    let answer = call_as(
        &host,
        &realm(),
        &read(None, Some("memory/whole.md")),
    )
    .await;
    assert_eq!(
        answer.unwrap(),
        ReadAnswer::Text {
            text: whole,
            cut: false
        }
    );
    let answer =
        call_as(&host, &realm(), &read(None, Some("memory/long.md")))
            .await;
    assert_eq!(
        answer.unwrap(),
        ReadAnswer::Text {
            text: "a".repeat(most - 1),
            cut: true
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_is_refused_a_revision_or_a_path_the_profile_lacks() {
    let dir = Scratch::new("profile-read-refused");
    let host = made(&dir).await;
    commit_main(&dir, &[write("memory/deep/a.md", "a")]);
    let v9 = Revision::Version {
        version: VersionName("v9".into()),
    };
    let missing =
        call_as(&host, &realm(), &read(Some(v9), None)).await;
    assert_eq!(
        missing.unwrap(),
        ReadAnswer::Refused {
            why: Missing::NoVersion {
                profile: ProfileId(1),
                version: VersionName("v9".into())
            }
            .into()
        }
    );
    let unknown = CommitId(Sha1([0xab; 20]));
    let commit = Revision::Commit { commit: unknown };
    let missing =
        call_as(&host, &realm(), &read(Some(commit), None)).await;
    assert_eq!(
        missing.unwrap(),
        ReadAnswer::Refused {
            why: BadCommit::NoCommit {
                profile: ProfileId(1),
                commit: unknown
            }
            .into()
        }
    );
    for path in [
        "nothing.md",
        "",
        "memory",
        "memory/",
        "../a.md",
        "/CHARACTER.md",
    ] {
        let answer =
            call_as(&host, &realm(), &read(None, Some(path))).await;
        assert_eq!(
            answer.unwrap(),
            ReadAnswer::Refused {
                why: BadPath::NotAPath { path: path.into() }.into()
            },
            "{path:?}"
        );
    }
    let read = call_as(&host, &agent(), &read(None, None)).await;
    assert_eq!(error_code(read), "message.wrong_caller");
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_s_repeat_is_answered_from_its_marker_and_its_id_is_its_own()
 {
    let dir = Scratch::new("profile-edit-repeat");
    let host = made(&dir).await;
    let id = RequestId::new();
    let first = edit(&id, "memory/a.md", "a");
    let done = call_as(&host, &realm(), &first).await.unwrap();
    assert_eq!(done, Done::Done);
    edited(&host, "memory/a.md", "b").await;
    let main = main_tip(&dir);
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir);
    assert_eq!(call_as(&host, &realm(), &first).await.unwrap(), done);
    let other = edit(&id, "memory/a.md", "c");
    assert_eq!(
        error_code(call_as(&host, &realm(), &other).await),
        "message.id_reused"
    );
    let erase = Erase { id };
    assert_eq!(
        error_code(call_as(&host, &realm(), &erase).await),
        "message.id_reused"
    );
    assert_eq!(main_tip(&dir), main);
    assert_eq!(
        text(&dir, commit_id(main), "memory/a.md").as_deref(),
        Some("b")
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erase_removes_the_repository_and_the_profile() {
    let dir = Scratch::new("profile-erase");
    let host = made(&dir).await;
    let erase = Erase {
        id: RequestId::new(),
    };
    let done = call_as(&host, &realm(), &erase).await.unwrap();
    assert_eq!(done, Done::Done);
    assert!(!dir.join("scholars/1.git").exists());
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.is_running(&profile_at(1)) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the profile stops within 5s");
    let after = call_as(&host, &realm(), &erase).await;
    assert_eq!(error_code(after), "actor.not_found");
    host.shutdown().await.unwrap();
}

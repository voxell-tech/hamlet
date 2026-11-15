//! A hire's files through a host: its agent's fills and saves, the
//! realm's pins, and `main` taken into the hires that follow it.

use std::collections::BTreeMap;

use hamlet_git::Change;
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    BadPath, Cursor, FileChange, Files, FilesAnswer, Gone, Missing,
    NotAHire, Piece, Pin, PinAnswer, PinRefusal, Protected, Save,
    SaveAnswer, SaveRefusal, TooLarge,
};
use hamlet_messages::workspace::CallId;
use hamlet_messages::{MOST_CALL, MOST_PART, PIECE_FRAME};

use super::*;

/// A kept unit of the realm's, a tag of `version`, whose settle
/// takes `main` into the hires that follow it.
async fn settle(host: &Host, version: &str) {
    let tagged =
        call_as(host, &realm(), &tag(&RequestId::new(), version, ""))
            .await
            .unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }), "{tagged:?}");
}

/// The result of one fill.
#[derive(Debug, Default)]
struct Filled {
    tip: Option<CommitId>,
    /// Each file, whole, and whether it runs.
    files: BTreeMap<String, (Vec<u8>, bool)>,
    removed: Vec<String>,
    character: Option<Character>,
    parts: usize,
}

/// Fills agent 7 from `since` as its agent does, every answer
/// measured as the host carried it.
async fn fill(host: &Host, since: Option<CommitId>) -> Filled {
    let mut filled = Filled::default();
    let mut after = None;
    loop {
        let asked = Files {
            since,
            after: after.take(),
        };
        let bytes = answer_of(host, &agent(), &asked).await.unwrap();
        assert!(
            bytes.len() <= MOST_CALL,
            "an answer of {}",
            bytes.len()
        );
        let FilesAnswer::Part {
            tip,
            files,
            removed,
            next,
            character,
        } = ciborium::from_reader(bytes.as_slice()).unwrap()
        else {
            panic!("a fill refused");
        };
        if filled.parts == 0 {
            filled.character = character;
        } else {
            assert_eq!(
                character, None,
                "a character after the first"
            );
        }
        assert!(filled.tip.is_none_or(|was| was == tip));
        filled.tip = Some(tip);
        filled.parts += 1;
        for piece in files {
            let (file, runs) =
                filled.files.entry(piece.path.clone()).or_default();
            assert_eq!(
                file.len(),
                piece.offset as usize,
                "{}",
                piece.path
            );
            file.extend(piece.bytes.0);
            *runs = piece.executable;
        }
        filled.removed.extend(removed.into_iter().map(|p| p.path));
        match next {
            Some(cursor) => after = Some(cursor),
            None => return filled,
        }
    }
}

/// Every file of `at` an agent is given, as a fill gives it.
fn given(
    dir: &Path,
    at: CommitId,
) -> BTreeMap<String, (Vec<u8>, bool)> {
    let repository = repository(dir, 1);
    let at = at.0.to_string().parse().unwrap();
    repository
        .entries(at)
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.path != "CHARACTER.md" && !e.path.starts_with("inbox/")
        })
        .map(|e| {
            let bytes = repository.blob(e.blob).unwrap();
            (e.path, (bytes, e.executable))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fill_of_many_files_comes_in_parts_none_over_a_call() {
    let dir = Scratch::new("profile-fill-many");
    let mut files = (0..2000)
        .map(|n| {
            write(
                &format!("memory/{n:04}.md"),
                format!("{n} ").repeat(250),
            )
        })
        .collect::<Vec<_>>();
    files.push(write("inbox/note.md", "for the agent alone"));
    let host = hired(&dir, &files, Arc::default()).await;

    let filled = fill(&host, None).await;
    assert!(filled.parts > 1, "{} parts", filled.parts);
    let tip = filled.tip.unwrap();
    assert_eq!(tip, commit_id(main_tip(&dir)));
    assert_eq!(filled.files.len(), 2000);
    assert_eq!(filled.files, given(&dir, tip));
    assert_eq!(
        filled.character,
        Some(Character {
            version: 1,
            text: CHARACTER.into()
        })
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_larger_than_a_part_comes_whole_in_pieces() {
    let dir = Scratch::new("profile-fill-large");
    let large = (0..3 << 20)
        .map(|n: u32| (n % 251) as u8)
        .collect::<Vec<_>>();
    let host = hired(
        &dir,
        &[write("large.bin", large.clone())],
        Arc::default(),
    )
    .await;

    let filled = fill(&host, None).await;
    assert!(filled.parts >= 5, "{} parts", filled.parts);
    assert_eq!(
        filled.files.keys().collect::<Vec<_>>(),
        ["large.bin"]
    );
    assert_eq!(filled.files["large.bin"].0, large);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn files_that_just_fill_a_part_or_just_miss_one_come_whole() {
    let dir = Scratch::new("profile-fill-edges");
    let mut files = (0..4)
        .map(|n| {
            let path = format!("exact{n}.bin");
            let size = MOST_PART - path.len() - PIECE_FRAME;
            write(&path, vec![0xff; size])
        })
        .collect::<Vec<_>>();
    let over = "over.bin";
    files.push(write(
        over,
        vec![0xfe; MOST_PART - over.len() - PIECE_FRAME + 1],
    ));
    files.push(write("zbig.bin", vec![0xfd; 3 * MOST_PART + 7]));
    let host = hired(&dir, &files, Arc::default()).await;

    let filled = fill(&host, None).await;
    assert_eq!(filled.files, given(&dir, filled.tip.unwrap()));
    assert_eq!(filled.files.len(), files.len());
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fill_since_a_commit_gives_only_what_changed() {
    let dir = Scratch::new("profile-fill-since");
    let files =
        [write("a.md", "a"), write("b.md", "b"), write("c.md", "c")];
    let host = hired(&dir, &files, Arc::default()).await;
    let first = fill(&host, None).await.tip.unwrap();

    let changes = [
        write("a.md", "A"),
        Change::Remove {
            path: "b.md".into(),
        },
        write("d.md", "d"),
        write("inbox/x.md", "x"),
    ];
    commit_main(&dir, &changes);
    settle(&host, "v1").await;

    let since = fill(&host, Some(first)).await;
    assert_eq!(
        since.files.keys().collect::<Vec<_>>(),
        ["a.md", "d.md"]
    );
    assert_eq!(since.files["a.md"].0, b"A");
    assert_eq!(since.removed, ["b.md"]);

    let unknown = CommitId(Sha1([1; 20]));
    let whole = fill(&host, Some(unknown)).await;
    assert_eq!(
        whole.files.keys().collect::<Vec<_>>(),
        ["a.md", "c.md", "d.md"]
    );
    assert!(whole.removed.is_empty());

    let gone = Files {
        since: None,
        after: Some(Cursor {
            commit: unknown,
            file: 1,
            offset: 0,
        }),
    };
    assert_eq!(
        call_as(&host, &agent(), &gone).await.unwrap(),
        FilesAnswer::Refused {
            why: Gone::CommitGone { commit: unknown }
        }
    );
    host.shutdown().await.unwrap();
}

fn save(
    call: &str,
    base: CommitId,
    part: u32,
    last: bool,
    changes: Vec<FileChange>,
) -> Save {
    Save {
        call: CallId(call.into()),
        base,
        part,
        last,
        changes,
    }
}

/// A piece of a file that runs.
fn runs(
    path: &str,
    offset: u32,
    bytes: &[u8],
    last: bool,
) -> FileChange {
    FileChange::Write(Piece {
        executable: true,
        offset,
        last,
        ..piece(path, bytes)
    })
}

fn piece(path: &str, bytes: &[u8]) -> Piece {
    Piece {
        path: path.into(),
        offset: 0,
        bytes: Base64(bytes.to_vec()),
        last: true,
        executable: false,
    }
}

fn whole(path: &str, bytes: &[u8]) -> FileChange {
    FileChange::Write(piece(path, bytes))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_in_parts_commits_once_and_its_repeat_answers_the_same()
 {
    let dir = Scratch::new("profile-save-parts");
    let host =
        hired(&dir, &[write("old.md", "old")], Arc::default()).await;
    let base = fill(&host, None).await.tip.unwrap();

    let parts = [
        save(
            "s1",
            base,
            0,
            false,
            vec![whole("notes.md", b"one"), whole("empty.md", b"")],
        ),
        save(
            "s1",
            base,
            1,
            false,
            vec![runs("run.sh", 0, b"#!/bin/sh\n", false)],
        ),
        save(
            "s1",
            base,
            2,
            true,
            vec![
                runs("run.sh", 10, b"echo hi\n", true),
                FileChange::Remove {
                    path: "old.md".into(),
                },
            ],
        ),
    ];
    let mut answers = Vec::new();
    for part in &parts {
        answers.push(call_as(&host, &agent(), part).await.unwrap());
    }
    let SaveAnswer::Saved { tip: saved } = answers[2] else {
        panic!("{answers:?}");
    };
    assert_eq!(answers[..2], [SaveAnswer::Kept, SaveAnswer::Kept]);
    assert_eq!(commit_id(tip(&dir, "hires/7").unwrap()), saved);

    let git_tip = tip(&dir, "hires/7").unwrap();
    let made = repository(&dir, 1)
        .first_parents(
            git_tip,
            Some(base.0.to_string().parse().unwrap()),
            10,
        )
        .unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].author, "agent:7");
    let files = given(&dir, saved);
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        ["empty.md", "notes.md", "run.sh"]
    );
    assert_eq!(
        files["run.sh"],
        (b"#!/bin/sh\necho hi\n".to_vec(), true)
    );
    assert_eq!(files["empty.md"], (Vec::new(), false));

    let again = call_as(&host, &agent(), &parts[2]).await.unwrap();
    assert_eq!(again, SaveAnswer::Saved { tip: saved });
    assert_eq!(tip(&dir, "hires/7"), Some(git_tip));
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_whose_parts_straddle_a_sleep_or_a_restart_commits_them_all()
 {
    let dir = Scratch::new("profile-save-restart");
    let host = hired(&dir, &[], Arc::default()).await;
    let base = fill(&host, None).await.tip.unwrap();
    let first = save("s3", base, 0, false, vec![whole("a.md", b"a")]);
    let kept = call_as(&host, &agent(), &first).await.unwrap();
    assert_eq!(kept, SaveAnswer::Kept);
    host.stop(&profile_at(1), ShutdownKind::Sleep)
        .await
        .unwrap();
    assert!(!host.is_running(&profile_at(1)));
    let second =
        save("s3", base, 1, false, vec![whole("b.md", b"b")]);
    let kept = call_as(&host, &agent(), &second).await.unwrap();
    assert_eq!(kept, SaveAnswer::Kept);
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir);
    let rest = save("s3", base, 2, true, vec![whole("c.md", b"c")]);
    let answer = call_as(&host, &agent(), &rest).await.unwrap();
    let SaveAnswer::Saved { tip: saved } = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(
        given(&dir, saved).keys().collect::<Vec<_>>(),
        ["a.md", "b.md", "c.md"]
    );
    assert_eq!(
        last_words(&dir, 1, "hires/7"),
        ("Kept what it learned".to_owned(), "agent:7".to_owned())
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_s_last_part_commits_only_when_every_part_before_it_came()
 {
    let dir = Scratch::new("profile-save-order");
    let host = hired(&dir, &[], Arc::default()).await;
    let base = fill(&host, None).await.tip.unwrap();
    let part = |call: &str, part: u32, last: bool, path: &str| {
        save(call, base, part, last, vec![whole(path, b"x")])
    };
    let kept = async |asked: Save| {
        let answer = call_as(&host, &agent(), &asked).await.unwrap();
        assert_eq!(answer, SaveAnswer::Kept);
    };

    // The last part before the middle one.
    kept(part("s5", 0, false, "a.md")).await;
    call_as(&host, &agent(), &part("s5", 2, true, "c.md"))
        .await
        .unwrap_err();
    // Another call's first part between two parts of one.
    kept(part("s6", 0, false, "a.md")).await;
    kept(part("s6", 1, false, "b.md")).await;
    kept(part("s7", 0, false, "x.md")).await;
    call_as(&host, &agent(), &part("s6", 2, true, "c.md"))
        .await
        .unwrap_err();
    // A refused middle part.
    kept(part("s8", 0, false, "a.md")).await;
    let refused = part("s8", 1, false, "inbox/b.md");
    let answer = call_as(&host, &agent(), &refused).await.unwrap();
    assert!(
        matches!(answer, SaveAnswer::Refused { .. }),
        "{answer:?}"
    );
    call_as(&host, &agent(), &part("s8", 2, true, "c.md"))
        .await
        .unwrap_err();
    assert_eq!(commit_id(tip(&dir, "hires/7").unwrap()), base);

    kept(part("s9", 0, false, "a.md")).await;
    kept(part("s9", 1, false, "b.md")).await;
    let answer =
        call_as(&host, &agent(), &part("s9", 2, true, "c.md"))
            .await
            .unwrap();
    let SaveAnswer::Saved { tip: saved } = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(
        given(&dir, saved).keys().collect::<Vec<_>>(),
        ["a.md", "b.md", "c.md"]
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_onto_a_moved_branch_keeps_both_and_its_side_wins() {
    let dir = Scratch::new("profile-save-moved");
    let notes = write("notes.md", "one\ntwo\nthree\n");
    let host = hired(&dir, &[notes], Arc::default()).await;
    let base = fill(&host, None).await.tip.unwrap();
    commit_main(
        &dir,
        &[
            write("notes.md", "uno\ntwo\nthree\n"),
            write("main.md", "from main"),
        ],
    );
    settle(&host, "v1").await;
    assert_ne!(commit_id(tip(&dir, "hires/7").unwrap()), base);

    let asked = save(
        "s2",
        base,
        0,
        true,
        vec![
            whole("notes.md", b"ONE\ntwo\nthree\n"),
            whole("saved.md", b"from the agent"),
        ],
    );
    let SaveAnswer::Saved { tip: saved } =
        call_as(&host, &agent(), &asked).await.unwrap()
    else {
        panic!("a save refused");
    };
    assert_eq!(
        text(&dir, saved, "notes.md").as_deref(),
        Some("ONE\ntwo\nthree\n")
    );
    assert_eq!(
        text(&dir, saved, "main.md").as_deref(),
        Some("from main")
    );
    assert_eq!(
        text(&dir, saved, "saved.md").as_deref(),
        Some("from the agent")
    );
    commit_main(&dir, &[write("later.md", "later")]);
    settle(&host, "v2").await;
    assert_eq!(
        last_words(&dir, 1, "hires/7"),
        (
            "Took in what its scholar learned".to_owned(),
            "realm".to_owned()
        )
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_is_refused_what_it_may_not_change() {
    let dir = Scratch::new("profile-save-refused");
    let host = hired(&dir, &[], Arc::default()).await;
    let base = fill(&host, None).await.tip.unwrap();
    let withheld = |path: &str| {
        SaveRefusal::from(Protected::Withheld { path: path.into() })
    };
    let not_a_path = |path: &str| {
        SaveRefusal::from(BadPath::NotAPath { path: path.into() })
    };
    let most = u32::try_from(MOST_PART).unwrap();
    let cases = [
        (vec![whole("inbox/a.md", b"a")], withheld("inbox/a.md")),
        (
            vec![FileChange::Remove {
                path: "inbox".into(),
            }],
            withheld("inbox"),
        ),
        (
            vec![whole("CHARACTER.md", b"Bolder.")],
            withheld("CHARACTER.md"),
        ),
        (vec![whole("../a.md", b"a")], not_a_path("../a.md")),
        (vec![whole("/a.md", b"a")], not_a_path("/a.md")),
        (vec![whole("a//b.md", b"a")], not_a_path("a//b.md")),
        (
            vec![whole("large.bin", &vec![0; MOST_PART])],
            SaveRefusal::from(TooLarge::PartTooLarge { most }),
        ),
    ];
    for (n, (changes, why)) in cases.into_iter().enumerate() {
        let asked = save(&format!("r{n}"), base, 0, true, changes);
        let answer = call_as(&host, &agent(), &asked).await.unwrap();
        assert_eq!(answer, SaveAnswer::Refused { why });
    }
    let gone = CommitId(Sha1([1; 20]));
    let asked =
        save("r-gone", gone, 0, true, vec![whole("a.md", b"a")]);
    assert_eq!(
        call_as(&host, &agent(), &asked).await.unwrap(),
        SaveAnswer::Refused {
            why: Gone::CommitGone { commit: gone }.into()
        }
    );
    assert_eq!(commit_id(tip(&dir, "hires/7").unwrap()), base);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_onto_another_hires_commit_is_refused() {
    let dir = Scratch::new("profile-save-other-base");
    let host = hired(&dir, &[], Arc::default()).await;
    hire(&host, 8).await;
    let base = commit_id(tip(&dir, "hires/8").unwrap());
    let theirs =
        save("theirs", base, 0, true, vec![whole("eight.md", b"8")]);
    let SaveAnswer::Saved { tip: eight } =
        call_as(&host, &agent_at(8), &theirs).await.unwrap()
    else {
        panic!("hire 8's save refused");
    };

    let mine =
        save("mine", eight, 0, true, vec![whole("seven.md", b"7")]);
    assert_eq!(
        call_as(&host, &agent(), &mine).await.unwrap(),
        SaveAnswer::Refused {
            why: Gone::CommitGone { commit: eight }.into()
        }
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_with_no_hire_here_is_a_wrong_caller() {
    let dir = Scratch::new("profile-files-caller");
    let host = hired(&dir, &[], Arc::default()).await;
    let base = commit_id(main_tip(&dir));
    let stranger = agent_at(8);
    let files = Files {
        since: None,
        after: None,
    };
    assert_eq!(
        error_code(call_as(&host, &stranger, &files).await),
        "message.wrong_caller"
    );
    let refused =
        save("w", base, 0, true, vec![whole("inbox/a.md", b"a")]);
    assert_eq!(
        error_code(call_as(&host, &stranger, &refused).await),
        "message.wrong_caller"
    );
    host.shutdown().await.unwrap();
}

fn pin(id: &RequestId, hire: u32, to: Option<&str>) -> Pin {
    Pin {
        id: id.clone(),
        hire: AgentNumber(hire),
        to: to.map(|to| VersionName(to.into())),
        by: By::Chief { chief: 3 },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pin_holds_a_hire_where_main_moves_the_others() {
    let dir = Scratch::new("profile-pin");
    let told = Arc::new(Told::default());
    let host = hired(&dir, &[], told.clone()).await;
    hire(&host, 8).await;
    for number in [7, 8] {
        host.get_or_start(&agent_at(number)).await.unwrap();
    }
    let first = main_tip(&dir);
    settle(&host, "v1").await;
    let pinned = call_as(
        &host,
        &realm(),
        &pin(&RequestId::new(), 7, Some("v1")),
    )
    .await
    .unwrap();
    assert_eq!(
        pinned,
        PinAnswer::Pinned {
            version: VersionName("v1".into()),
            commit: commit_id(first)
        }
    );

    let main =
        commit_main(&dir, &[write("memory/later.md", "later")]);
    settle(&host, "v2").await;
    assert_eq!(tip(&dir, "hires/8"), Some(main));
    assert_eq!(tip(&dir, "hires/7"), Some(first));
    let moved = moved_to(&told, 8).await;
    assert_eq!(
        moved,
        Moved {
            tip: commit_id(main),
            character: None
        }
    );
    assert!(
        told.0
            .lock()
            .unwrap()
            .iter()
            .all(|(to, _)| *to != agent().to_string())
    );

    let id = RequestId::new();
    let repinned = call_as(&host, &realm(), &pin(&id, 7, Some("v2")))
        .await
        .unwrap();
    assert_eq!(
        repinned,
        PinAnswer::Pinned {
            version: VersionName("v2".into()),
            commit: commit_id(main)
        }
    );
    assert_eq!(tip(&dir, "hires/7"), Some(main));
    assert_eq!(moved_to(&told, 7).await.tip, commit_id(main));
    let again = call_as(&host, &realm(), &pin(&id, 7, Some("v2")))
        .await
        .unwrap();
    assert_eq!(again, repinned);

    let missing = call_as(
        &host,
        &realm(),
        &pin(&RequestId::new(), 7, Some("v9")),
    )
    .await
    .unwrap();
    assert_eq!(
        missing,
        PinAnswer::Refused {
            why: PinRefusal::from(Missing::NoVersion {
                profile: ProfileId(1),
                version: VersionName("v9".into())
            })
        }
    );
    let stranger =
        call_as(&host, &realm(), &pin(&RequestId::new(), 12, None))
            .await
            .unwrap();
    assert_eq!(
        stranger,
        PinAnswer::Refused {
            why: PinRefusal::from(NotAHire::NoHire {
                hire: AgentNumber(12)
            })
        }
    );
    let follows =
        call_as(&host, &realm(), &pin(&RequestId::new(), 7, None))
            .await
            .unwrap();
    assert_eq!(follows, PinAnswer::FollowsMain);
    host.shutdown().await.unwrap();
}

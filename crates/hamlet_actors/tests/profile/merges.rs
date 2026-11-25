//! A merge of one hire's branch into another's and its split, a
//! branch read out in parts and a branch dropped.

use hamlet_git::Change;
use hamlet_messages::MOST_PART;
use hamlet_messages::profile::{
    Bundle, BundleAnswer, By, DropBranch, Gone, MergeAnswer,
    MergeBranch, Piece, Protected, Purge, PurgeAnswer, Removed,
    SaveRefusal, Source, Unmerge, UnmergeAnswer,
};

use super::*;

const TOOK_IN: &str =
    "Took in what hire#8 learned: merged by the chief";

/// Commits `changes` on `branch` of profile `id` as its agent's save
/// does.
fn save_on(
    dir: &Path,
    id: u32,
    branch: &str,
    changes: &[Change],
) -> hamlet_git::CommitId {
    let by = hamlet_git::Author {
        name: "agent:8",
        at: 6000,
    };
    repository(dir, id)
        .commit(branch, changes, "Kept what it learned", by)
        .unwrap()
}

fn read(
    dir: &Path,
    id: u32,
    at: hamlet_git::CommitId,
    path: &str,
) -> Option<String> {
    let bytes = repository(dir, id).read(at, path).unwrap()?;
    Some(String::from_utf8(bytes).unwrap())
}

fn hire_tip(dir: &Path, number: u32) -> hamlet_git::CommitId {
    tip(dir, &format!("hires/{number}")).unwrap()
}

fn git_commit(commit: CommitId) -> hamlet_git::CommitId {
    commit.0.to_string().parse().unwrap()
}

fn merge(id: &RequestId, from: Source) -> MergeBranch {
    MergeBranch {
        id: id.clone(),
        into: "hires/7".into(),
        absorbed: AgentNumber(8),
        message: TOOK_IN.into(),
        from,
    }
}

fn from_branch() -> Source {
    Source::Branch {
        name: "hires/8".into(),
    }
}

fn unmerge(id: &RequestId, merge: CommitId) -> Unmerge {
    Unmerge {
        id: id.clone(),
        branch: "hires/7".into(),
        merge,
    }
}

fn commit_count(dir: &Path, branch: &str) -> usize {
    repository(dir, 1)
        .history(tip(dir, branch).unwrap(), usize::MAX)
        .unwrap()
        .len()
}

/// Profile 1 whose `main` holds `files`, with hires 7 and 8, and
/// hire 7's agent there to be told its branch moved.
async fn two_hires(
    dir: &Path,
    files: &[Change],
    told: Arc<Told>,
) -> Host {
    let host = hired(dir, files, told).await;
    hire(&host, 8).await;
    host.get_or_start(&agent()).await.unwrap();
    host
}

/// Merges hires/8 into hires/7, each having changed `memory/notes.md`
/// and `skills/plan/SKILL.md`, and hires/8 also its character, its
/// inbox and a file of its own.
async fn merged(dir: &Path, told: Arc<Told>) -> (Host, MergeAnswer) {
    let main = [
        write("memory/notes.md", "one\n"),
        write("skills/plan/SKILL.md", "plan\n"),
    ];
    let host = two_hires(dir, &main, told).await;
    save_on(
        dir,
        1,
        "hires/7",
        &[
            write("memory/notes.md", "seven\n"),
            write("skills/plan/SKILL.md", "plan seven\n"),
        ],
    );
    save_on(
        dir,
        1,
        "hires/8",
        &[
            write("memory/notes.md", "eight\n"),
            write("skills/plan/SKILL.md", "plan eight\n"),
            write("memory/learned.md", "learned\n"),
            write("CHARACTER.md", "A reckless reader."),
            write("inbox/letter.md", "for hire 8 alone"),
        ],
    );
    let id = RequestId::new();
    let answer = call_as(&host, &realm(), &merge(&id, from_branch()))
        .await
        .unwrap();
    (host, answer)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_within_one_profile_keeps_the_kept_hires_lines_and_sets_nothing_aside()
 {
    let dir = Scratch::new("profile-merge-clash");
    let told = Arc::new(Told::default());
    let (host, answer) = merged(&dir, told.clone()).await;

    let tip = hire_tip(&dir, 7);
    assert_eq!(
        answer,
        MergeAnswer::Merged {
            tip: commit_id(tip),
            set_aside: Vec::new(),
        }
    );
    let file = |path| read(&dir, 1, tip, path);
    assert_eq!(file("memory/notes.md").as_deref(), Some("seven\n"));
    assert_eq!(file("memory/notes.md.from-hire-8"), None);
    assert_eq!(
        file("skills/plan/SKILL.md").as_deref(),
        Some("plan seven\n")
    );
    assert_eq!(file("skills/plan-from-hire-8/SKILL.md"), None);
    assert_eq!(
        file("memory/learned.md").as_deref(),
        Some("learned\n")
    );
    assert_eq!(file("CHARACTER.md").as_deref(), Some(CHARACTER));
    assert_eq!(file("inbox/letter.md"), None);
    assert_eq!(
        last_words(&dir, 1, "hires/7"),
        (TOOK_IN.to_owned(), "realm".to_owned())
    );
    told_at(&told, 7, tip).await;
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_merge_answers_its_first_answer_and_commits_once()
{
    let dir = Scratch::new("profile-merge-repeat");
    let main = [write("memory/notes.md", "one\n")];
    let host = two_hires(&dir, &main, Arc::default()).await;
    save_on(&dir, 1, "hires/8", &[write("memory/a.md", "a")]);
    let id = RequestId::new();
    let first = call_as(&host, &realm(), &merge(&id, from_branch()))
        .await
        .unwrap();
    let count = commit_count(&dir, "hires/7");
    let again = call_as(&host, &realm(), &merge(&id, from_branch()))
        .await
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(commit_count(&dir, "hires/7"), count);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_of_a_hire_that_learned_nothing_is_still_a_commit_of_its_own()
 {
    let dir = Scratch::new("profile-merge-nothing");
    let host = two_hires(&dir, &[], Arc::default()).await;
    let saved =
        save_on(&dir, 1, "hires/7", &[write("memory/a.md", "a")]);
    let id = RequestId::new();
    let MergeAnswer::Merged { tip, set_aside } =
        call_as(&host, &realm(), &merge(&id, from_branch()))
            .await
            .unwrap()
    else {
        panic!("not merged");
    };
    assert!(set_aside.is_empty());
    assert_ne!(git_commit(tip), saved);

    let split =
        call_as(&host, &realm(), &unmerge(&RequestId::new(), tip))
            .await
            .unwrap();
    assert!(matches!(split, UnmergeAnswer::Unmerged { .. }));
    let now = hire_tip(&dir, 7);
    assert_eq!(
        read(&dir, 1, now, "memory/a.md").as_deref(),
        Some("a")
    );
    host.shutdown().await.unwrap();
}

/// Every part of what `branch` of the host's profile 1 learned, as
/// the realm reads it.
async fn bundle(
    host: &Host,
    branch: &str,
) -> Vec<(Vec<Piece>, Vec<Removed>)> {
    let mut parts = Vec::new();
    let mut after = None;
    loop {
        let asked = Bundle {
            branch: branch.into(),
            after: after.take(),
        };
        let BundleAnswer::Part {
            files,
            removed,
            next,
        } = call_as(host, &realm(), &asked).await.unwrap()
        else {
            panic!("a bundle refused");
        };
        parts.push((files, removed));
        match next {
            Some(cursor) => after = Some(cursor),
            None => return parts,
        }
    }
}

fn part(
    number: usize,
    last: bool,
    (files, removed): (Vec<Piece>, Vec<Removed>),
) -> Source {
    Source::Part {
        part: u32::try_from(number).unwrap(),
        last,
        files,
        removed,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_from_another_profile_brings_its_files() {
    let other = Scratch::new("profile-merge-other-from");
    let common = [
        write("memory/notes.md", "one\n"),
        write("memory/old.md", "old\n"),
        write("memory/gone.md", "gone\n"),
    ];
    let from = hired(&other, &common, Arc::default()).await;
    hire(&from, 8).await;
    let large = "x".repeat(MOST_PART + MOST_PART / 2);
    save_on(
        &other,
        1,
        "hires/8",
        &[
            write("memory/new.md", "new\n"),
            write("memory/notes.md", "theirs\n"),
            write("memory/large.md", large.clone()),
            write("CHARACTER.md", "A reckless reader."),
            write("inbox/letter.md", "for hire 8 alone"),
            Change::Remove {
                path: "memory/old.md".into(),
            },
            Change::Remove {
                path: "memory/gone.md".into(),
            },
        ],
    );
    let parts = bundle(&from, "hires/8").await;
    assert!(parts.len() > 1, "{} parts", parts.len());
    let sent = parts
        .iter()
        .flat_map(|(files, _)| files.iter().map(|p| p.path.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        sent.into_iter().collect::<Vec<_>>(),
        ["memory/large.md", "memory/new.md", "memory/notes.md"]
    );
    from.shutdown().await.unwrap();

    let dir = Scratch::new("profile-merge-other-into");
    let told = Arc::new(Told::default());
    let host = hired(&dir, &common, told.clone()).await;
    host.get_or_start(&agent()).await.unwrap();
    save_on(&dir, 1, "hires/7", &[write("memory/gone.md", "kept\n")]);
    let id = RequestId::new();
    let last = parts.len() - 1;
    let mut answers = Vec::new();
    for (number, sent) in parts.clone().into_iter().enumerate() {
        let asked = merge(&id, part(number, number == last, sent));
        answers.push(call_as(&host, &realm(), &asked).await.unwrap());
    }
    let tip = hire_tip(&dir, 7);
    assert!(answers[..last].iter().all(|a| *a == MergeAnswer::Kept));
    assert_eq!(
        answers[last],
        MergeAnswer::Merged {
            tip: commit_id(tip),
            set_aside: vec!["memory/notes.md.from-hire-8".into()],
        }
    );
    let file = |path| read(&dir, 1, tip, path);
    assert_eq!(file("memory/new.md").as_deref(), Some("new\n"));
    assert_eq!(file("memory/notes.md").as_deref(), Some("one\n"));
    assert_eq!(
        file("memory/notes.md.from-hire-8").as_deref(),
        Some("theirs\n")
    );
    assert_eq!(file("memory/large.md"), Some(large));
    assert_eq!(file("memory/old.md"), None);
    assert_eq!(file("memory/gone.md").as_deref(), Some("kept\n"));
    assert_eq!(file("CHARACTER.md").as_deref(), Some(CHARACTER));
    assert_eq!(
        last_words(&dir, 1, "hires/7"),
        (TOOK_IN.to_owned(), "realm".to_owned())
    );
    told_at(&told, 7, tip).await;

    let count = commit_count(&dir, "hires/7");
    let again = merge(&id, part(last, true, parts[last].clone()));
    assert_eq!(
        call_as(&host, &realm(), &again).await.unwrap(),
        answers[last]
    );
    assert_eq!(commit_count(&dir, "hires/7"), count);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_part_that_names_the_character_or_the_inbox_is_refused()
 {
    let dir = Scratch::new("profile-merge-withheld");
    let host = hired(&dir, &[], Arc::default()).await;
    let was = hire_tip(&dir, 7);
    for path in ["CHARACTER.md", "inbox/letter.md"] {
        let piece = Piece {
            path: path.into(),
            offset: 0,
            bytes: hamlet_messages::bytes::Base64(b"x".to_vec()),
            last: true,
            executable: false,
        };
        let asked = merge(
            &RequestId::new(),
            part(0, true, (vec![piece], Vec::new())),
        );
        assert_eq!(
            call_as(&host, &realm(), &asked).await.unwrap(),
            MergeAnswer::Refused {
                why: SaveRefusal::Protected(Protected::Withheld {
                    path: path.into()
                })
            }
        );
    }
    let skipped = merge(
        &RequestId::new(),
        part(1, true, (Vec::new(), Vec::new())),
    );
    call_as(&host, &realm(), &skipped).await.unwrap_err();
    assert_eq!(hire_tip(&dir, 7), was);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_split_takes_back_the_merge_and_keeps_later_work() {
    let dir = Scratch::new("profile-split");
    let told = Arc::new(Told::default());
    let main = [write("memory/notes.md", "one\n")];
    let host = two_hires(&dir, &main, told.clone()).await;
    save_on(
        &dir,
        1,
        "hires/7",
        &[write("memory/notes.md", "seven\n")],
    );
    save_on(
        &dir,
        1,
        "hires/8",
        &[
            write("memory/notes.md", "eight\n"),
            write("memory/a.md", "a\n"),
            write("memory/b.md", "b\n"),
        ],
    );
    let MergeAnswer::Merged { tip: merged, .. } = call_as(
        &host,
        &realm(),
        &merge(&RequestId::new(), from_branch()),
    )
    .await
    .unwrap() else {
        panic!("not merged");
    };
    save_on(
        &dir,
        1,
        "hires/7",
        &[
            write("memory/b.md", "b, and more\n"),
            write("memory/later.md", "later\n"),
        ],
    );

    let id = RequestId::new();
    let split = call_as(&host, &realm(), &unmerge(&id, merged))
        .await
        .unwrap();
    let tip = hire_tip(&dir, 7);
    assert_eq!(
        split,
        UnmergeAnswer::Unmerged {
            tip: commit_id(tip)
        }
    );
    let file = |path| read(&dir, 1, tip, path);
    assert_eq!(file("memory/a.md"), None);
    assert_eq!(file("memory/notes.md.from-hire-8"), None);
    assert_eq!(file("memory/notes.md").as_deref(), Some("seven\n"));
    assert_eq!(file("memory/b.md").as_deref(), Some("b, and more\n"));
    assert_eq!(file("memory/later.md").as_deref(), Some("later\n"));
    assert_eq!(
        last_words(&dir, 1, "hires/7"),
        ("Split the merge".to_owned(), "realm".to_owned())
    );
    told_at(&told, 7, tip).await;

    let count = commit_count(&dir, "hires/7");
    assert_eq!(
        call_as(&host, &realm(), &unmerge(&id, merged))
            .await
            .unwrap(),
        split
    );
    assert_eq!(commit_count(&dir, "hires/7"), count);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_split_of_a_merge_the_branch_does_not_hold_is_refused() {
    let dir = Scratch::new("profile-split-gone");
    let host = two_hires(&dir, &[], Arc::default()).await;
    let elsewhere = commit_id(save_on(
        &dir,
        1,
        "hires/8",
        &[write("memory/a.md", "a")],
    ));
    let was = hire_tip(&dir, 7);
    let asked = unmerge(&RequestId::new(), elsewhere);
    assert_eq!(
        call_as(&host, &realm(), &asked).await.unwrap(),
        UnmergeAnswer::Refused {
            why: Gone::CommitGone { commit: elsewhere }
        }
    );
    assert_eq!(hire_tip(&dir, 7), was);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_branch_dropped_twice_is_gone_once() {
    let dir = Scratch::new("profile-drop");
    let host = two_hires(&dir, &[], Arc::default()).await;
    let seven = hire_tip(&dir, 7);
    let drop = DropBranch {
        name: "hires/8".into(),
    };
    call_as(&host, &realm(), &drop).await.unwrap();
    assert_eq!(tip(&dir, "hires/8"), None);
    call_as(&host, &realm(), &drop).await.unwrap();
    assert_eq!(tip(&dir, "hires/8"), None);

    commit_main(&dir, &[write("memory/a.md", "a")]);
    let tagged =
        call_as(&host, &realm(), &tag(&RequestId::new(), "v1", ""))
            .await
            .unwrap();
    assert!(matches!(tagged, TagAnswer::Tagged { .. }), "{tagged:?}");
    assert_ne!(hire_tip(&dir, 7), seven);
    assert_eq!(tip(&dir, "hires/8"), None);
    let branches = repository(&dir, 1).branches().unwrap();
    let names = branches.into_iter().map(|(name, _)| name);
    assert_eq!(names.collect::<Vec<_>>(), ["hires/7", "main"]);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_that_brings_nothing_splits_to_nothing() {
    let dir = Scratch::new("profile-merge-empty");
    let host = two_hires(&dir, &[], Arc::default()).await;
    let drop = DropBranch {
        name: "hires/8".into(),
    };
    call_as(&host, &realm(), &drop).await.unwrap();
    assert_eq!(
        bundle(&host, "hires/8").await,
        [(Vec::new(), Vec::new())]
    );
    let saved =
        save_on(&dir, 1, "hires/7", &[write("memory/a.md", "a")]);

    let empty = part(0, true, (Vec::new(), Vec::new()));
    for from in [from_branch(), empty] {
        let asked = merge(&RequestId::new(), from);
        let MergeAnswer::Merged { tip, set_aside } =
            call_as(&host, &realm(), &asked).await.unwrap()
        else {
            panic!("not merged");
        };
        assert!(set_aside.is_empty());
        assert_ne!(git_commit(tip), saved);
        let split = unmerge(&RequestId::new(), tip);
        call_as(&host, &realm(), &split).await.unwrap();
        let now = hire_tip(&dir, 7);
        assert_eq!(
            read(&dir, 1, now, "memory/a.md").as_deref(),
            Some("a")
        );
    }
    host.shutdown().await.unwrap();
}

/// Merges hires/8 into hires/7, splits it, lets hires/8 learn
/// `memory/b.md`, purges `secret.md` when `purge`, then merges again
/// with the same message, which must make a commit of its own.
async fn merged_again(name: &str, purge: bool) {
    let dir = Scratch::new(name);
    let main = [write("secret.md", "s\n")];
    let host = two_hires(&dir, &main, Arc::default()).await;
    save_on(&dir, 1, "hires/8", &[write("memory/a.md", "a\n")]);
    let MergeAnswer::Merged { tip: first, .. } = call_as(
        &host,
        &realm(),
        &merge(&RequestId::new(), from_branch()),
    )
    .await
    .unwrap() else {
        panic!("not merged");
    };
    call_as(&host, &realm(), &unmerge(&RequestId::new(), first))
        .await
        .unwrap();
    save_on(&dir, 1, "hires/8", &[write("memory/b.md", "b\n")]);
    if purge {
        let purge = Purge {
            id: RequestId::new(),
            path: "secret.md".into(),
            by: By::Chief { chief: 3 },
        };
        let purged = call_as(&host, &realm(), &purge).await.unwrap();
        assert!(
            matches!(purged, PurgeAnswer::Purged { .. }),
            "{purged:?}"
        );
    }
    let before = hire_tip(&dir, 7);

    let again = call_as(
        &host,
        &realm(),
        &merge(&RequestId::new(), from_branch()),
    )
    .await
    .unwrap();

    let after = hire_tip(&dir, 7);
    assert_ne!(after, before, "the second merge made no commit");
    assert_eq!(
        again,
        MergeAnswer::Merged {
            tip: commit_id(after),
            set_aside: Vec::new(),
        }
    );
    assert_eq!(
        read(&dir, 1, after, "memory/b.md").as_deref(),
        Some("b\n")
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_again_after_its_split_makes_a_commit_of_its_own() {
    merged_again("profile-merge-again", false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_again_after_a_purge_makes_a_commit_of_its_own() {
    merged_again("profile-merge-again-purged", true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_split_takes_out_the_merged_lines_and_keeps_later_ones() {
    let dir = Scratch::new("profile-split-lines");
    let main =
        [write("memory/notes.md", "one\ntwo\nthree\nfour\nfive\n")];
    let host = two_hires(&dir, &main, Arc::default()).await;
    save_on(&dir, 1, "hires/7", &[write("memory/own.md", "own\n")]);
    save_on(
        &dir,
        1,
        "hires/8",
        &[write(
            "memory/notes.md",
            "one\ntwo\nthree\nfour\nfive\nfrom eight\n",
        )],
    );
    let MergeAnswer::Merged { tip: merged, .. } = call_as(
        &host,
        &realm(),
        &merge(&RequestId::new(), from_branch()),
    )
    .await
    .unwrap() else {
        panic!("not merged");
    };
    save_on(
        &dir,
        1,
        "hires/7",
        &[write(
            "memory/notes.md",
            "ONE\ntwo\nthree\nfour\nfive\nfrom eight\n",
        )],
    );

    call_as(&host, &realm(), &unmerge(&RequestId::new(), merged))
        .await
        .unwrap();

    let tip = hire_tip(&dir, 7);
    assert_eq!(
        read(&dir, 1, tip, "memory/notes.md").as_deref(),
        Some("ONE\ntwo\nthree\nfour\nfive\n")
    );
    host.shutdown().await.unwrap();
}

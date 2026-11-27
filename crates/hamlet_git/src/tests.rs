use hamlet_testing::Scratch;

use super::*;

fn by(at: i64) -> Author<'static> {
    Author {
        name: "chief:1",
        at: at * 1000,
    }
}

fn write(path: &str, text: &str) -> Change {
    Change::Write {
        path: path.into(),
        bytes: text.into(),
    }
}

fn text(
    repo: &Repository,
    at: CommitId,
    path: &str,
) -> Option<String> {
    repo.read(at, path)
        .unwrap()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

fn repository(dir: &Scratch) -> Repository {
    Repository::create(&dir.join("profile.git")).unwrap()
}

#[test]
fn a_hire_branch_learns_apart_and_takes_in_main() {
    let dir = Scratch::new("git-learn");
    let repo = repository(&dir);
    let first = repo
        .commit(
            MAIN,
            &[
                write("CHARACTER.md", "Careful."),
                write("memory/notes.md", "one\ntwo\nthree\n"),
                write("skills/rust/SKILL.md", "Use clippy."),
            ],
            "made",
            by(1),
        )
        .unwrap();
    repo.branch("hires/4", first).unwrap();
    repo.commit(
        "hires/4",
        &[write("memory/notes.md", "ONE\ntwo\nthree\n")],
        "turn",
        by(2),
    )
    .unwrap();
    let main = repo
        .commit(
            MAIN,
            &[
                write("memory/notes.md", "one\ntwo\nTHREE\n"),
                write("skills/git/SKILL.md", "Small commits."),
            ],
            "edited",
            by(3),
        )
        .unwrap();

    let merged =
        repo.merge("hires/4", main, "took in main", by(4)).unwrap();
    assert_eq!(merged.joined, Joined::Committed);
    assert!(merged.clashes.is_empty());
    assert_eq!(
        text(&repo, merged.commit, "memory/notes.md").as_deref(),
        Some("ONE\ntwo\nTHREE\n")
    );
    assert!(
        text(&repo, merged.commit, "skills/git/SKILL.md").is_some()
    );
    assert_eq!(
        text(
            &repo,
            repo.tip(MAIN).unwrap().unwrap(),
            "memory/notes.md"
        )
        .as_deref(),
        Some("one\ntwo\nTHREE\n"),
        "main never sees what the hire learned"
    );
    let again = repo.merge("hires/4", main, "again", by(5)).unwrap();
    assert_eq!(again.joined, Joined::UpToDate);
    assert_eq!(again.commit, merged.commit);
}

#[test]
fn a_hire_that_learned_nothing_moves_forward_to_main() {
    let dir = Scratch::new("git-forward");
    let repo = repository(&dir);
    let first = repo
        .commit(MAIN, &[write("CHARACTER.md", "a")], "made", by(1))
        .unwrap();
    repo.branch("hires/1", first).unwrap();
    let main = repo
        .commit(MAIN, &[write("memory/a.md", "x")], "e", by(2))
        .unwrap();
    let merged = repo.merge("hires/1", main, "m", by(3)).unwrap();
    assert_eq!(merged.joined, Joined::FastForward);
    assert_eq!(repo.tip("hires/1").unwrap(), Some(main));
}

#[test]
fn a_clash_keeps_the_hires_lines() {
    let dir = Scratch::new("git-clash");
    let repo = repository(&dir);
    let first = repo
        .commit(
            MAIN,
            &[write("memory/notes.md", "a\n")],
            "made",
            by(1),
        )
        .unwrap();
    repo.branch("hires/4", first).unwrap();
    repo.commit(
        "hires/4",
        &[write("memory/notes.md", "hire\n")],
        "t",
        by(2),
    )
    .unwrap();
    let main = repo
        .commit(
            MAIN,
            &[write("memory/notes.md", "chief\n")],
            "e",
            by(3),
        )
        .unwrap();
    let merged = repo.merge("hires/4", main, "m", by(4)).unwrap();
    assert_eq!(merged.clashes, ["memory/notes.md"]);
    assert_eq!(
        text(&repo, merged.commit, "memory/notes.md").as_deref(),
        Some("hire\n")
    );
}

#[test]
fn a_revert_undoes_one_commit_and_keeps_later_ones() {
    let dir = Scratch::new("git-revert");
    let repo = repository(&dir);
    repo.commit(MAIN, &[write("memory/a.md", "a\n")], "made", by(1))
        .unwrap();
    let bad = repo
        .commit(
            MAIN,
            &[
                write("memory/b.md", "wrong\n"),
                write("memory/a.md", "a\nworse\n"),
            ],
            "dream",
            by(2),
        )
        .unwrap();
    repo.commit(MAIN, &[write("memory/c.md", "c\n")], "later", by(3))
        .unwrap();
    let reverted =
        repo.revert(MAIN, bad, "undo the dream", by(4)).unwrap();
    assert_eq!(reverted.joined, Joined::Committed);
    assert!(reverted.clashes.is_empty());
    let tip = reverted.commit;
    assert_eq!(
        text(&repo, tip, "memory/a.md").as_deref(),
        Some("a\n")
    );
    assert_eq!(text(&repo, tip, "memory/b.md"), None);
    assert_eq!(
        text(&repo, tip, "memory/c.md").as_deref(),
        Some("c\n")
    );
}

#[test]
fn a_name_no_ref_can_have_resolves_to_nothing() {
    let dir = Scratch::new("git-no-ref");
    let repo = repository(&dir);
    repo.commit(MAIN, &[write("CHARACTER.md", "a")], "made", by(1))
        .unwrap();

    for name in ["no such", "a..b", "", "end."] {
        assert_eq!(repo.resolve(name).unwrap(), None, "{name:?}");
    }
}

#[test]
fn versions_name_commits_and_resolve() {
    let dir = Scratch::new("git-versions");
    let repo = repository(&dir);
    let first = repo
        .commit(MAIN, &[write("CHARACTER.md", "a")], "made", by(1))
        .unwrap();
    repo.tag("v1", first, "first", by(2)).unwrap();
    assert!(repo.tag("v1", first, "again", by(3)).is_err());
    let second = repo
        .commit(MAIN, &[write("CHARACTER.md", "b")], "e", by(4))
        .unwrap();
    let versions = repo.versions().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].name, "v1");
    assert_eq!(versions[0].commit, first);
    assert_eq!(versions[0].note, "first");
    assert_eq!(versions[0].at, 2000);
    assert_eq!(repo.resolve("v1").unwrap(), Some(first));
    assert_eq!(repo.resolve(MAIN).unwrap(), Some(second));
    assert_eq!(
        repo.resolve(&first.to_string()).unwrap(),
        Some(first)
    );
    assert_eq!(repo.resolve("v9").unwrap(), None);
    let history = repo.history(second, 10).unwrap();
    assert_eq!(
        history.iter().map(|c| c.id).collect::<Vec<_>>(),
        [second, first]
    );
    assert_eq!(history[0].author, "chief:1");
    assert_eq!(history[0].at, 4000);
}

#[test]
fn a_kept_commit_is_told_from_a_missing_one_and_a_failed_read() {
    let dir = Scratch::new("git-has-commit");
    let repo = repository(&dir);
    let first = repo
        .commit(MAIN, &[write("CHARACTER.md", "a")], "made", by(1))
        .unwrap();
    assert!(repo.has_commit(first).unwrap());
    let blob = repo.write_blob(b"a file").unwrap();
    let blob = blob.to_string().parse().unwrap();
    assert!(!repo.has_commit(blob).unwrap());
    let missing = "ab".repeat(20).parse().unwrap();
    assert!(!repo.has_commit(missing).unwrap());

    let damaged: CommitId = "cd".repeat(20).parse().unwrap();
    let hex = damaged.to_string();
    let folder = dir.join("profile.git/objects").join(&hex[..2]);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(&hex[2..]), b"not a git object")
        .unwrap();
    assert!(repo.has_commit(damaged).is_err());
}

#[test]
fn a_folder_is_checked_out_and_committed_by_its_listing() {
    let dir = Scratch::new("git-lodge");
    let repo = repository(&dir);
    let first = repo
        .commit(
            MAIN,
            &[
                write("CHARACTER.md", "Careful."),
                write("memory/notes.md", "n"),
                write("skills/a/SKILL.md", "a"),
            ],
            "made",
            by(1),
        )
        .unwrap();
    repo.branch("hires/7", first).unwrap();
    let lodge = |path: &str| {
        path == "CHARACTER.md" || path.starts_with("inbox/")
    };

    // An empty folder gets every file but the character.
    let steps = repo.plan(first, &[], &lodge).unwrap();
    assert_eq!(
        steps
            .iter()
            .map(|step| match step {
                Step::Write { path, .. } | Step::Remove { path } => {
                    path.as_str()
                }
            })
            .collect::<Vec<_>>(),
        ["memory/notes.md", "skills/a/SKILL.md"]
    );

    // The hire changes its notes, adds a skill, drops one, and has a
    // file in its inbox and a stray character file.
    let notes = repo.write_blob(b"n2").unwrap();
    let skill = repo.write_blob(b"b").unwrap();
    let listing = vec![
        Entry {
            path: "CHARACTER.md".into(),
            blob: blob_id(b"I am free now."),
            executable: false,
        },
        Entry {
            path: "inbox/big.bin".into(),
            blob: blob_id(b"file"),
            executable: false,
        },
        Entry {
            path: "memory/notes.md".into(),
            blob: notes,
            executable: false,
        },
        Entry {
            path: "skills/b/run.sh".into(),
            blob: skill,
            executable: true,
        },
    ];
    assert_eq!(blob_id(b"n2"), notes);
    let commit = repo
        .commit_listing(
            "hires/7",
            &listing,
            &lodge,
            &[],
            "turn",
            by(2),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        text(&repo, commit, "CHARACTER.md").as_deref(),
        Some("Careful.")
    );
    assert_eq!(
        text(&repo, commit, "memory/notes.md").as_deref(),
        Some("n2")
    );
    assert_eq!(text(&repo, commit, "skills/a/SKILL.md"), None);
    assert_eq!(text(&repo, commit, "inbox/big.bin"), None);
    let entries = repo.entries(commit).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.path == "skills/b/run.sh" && e.executable)
    );
    assert_eq!(
        repo.commit_listing(
            "hires/7",
            &listing,
            &lodge,
            &[],
            "same",
            by(3)
        )
        .unwrap(),
        None,
        "an unchanged folder makes no commit"
    );
    let changes = repo.diff(Some(first), commit).unwrap();
    assert_eq!(
        changes.iter().map(FileChange::path).collect::<Vec<_>>(),
        ["memory/notes.md", "skills/a/SKILL.md", "skills/b/run.sh"]
    );
    assert!(repo.plan(commit, &listing, &lodge).unwrap().is_empty());
}

#[test]
fn a_fork_starts_from_a_version_of_another_profile() {
    let dir = Scratch::new("git-fork");
    let from = repository(&dir);
    let at = from
        .commit(
            MAIN,
            &[
                write("CHARACTER.md", "a"),
                write("skills/x/SKILL.md", "x"),
            ],
            "made",
            by(1),
        )
        .unwrap();
    let to = Repository::create(&dir.join("fork.git")).unwrap();
    let first =
        to.start_from(MAIN, &from, at, "forked", by(2)).unwrap();
    assert_eq!(to.files(first).unwrap(), from.files(at).unwrap());
    assert_eq!(to.history(first, 10).unwrap().len(), 1);
}

#[test]
fn a_purge_leaves_nothing_of_the_path() {
    let dir = Scratch::new("git-purge");
    let path = dir.join("profile.git");
    let repo = Repository::create(&path).unwrap();
    let first = repo
        .commit(
            MAIN,
            &[
                write("CHARACTER.md", "a"),
                write("memory/key.md", "secret-123"),
            ],
            "made",
            by(1),
        )
        .unwrap();
    repo.tag("v1", first, "first", by(2)).unwrap();
    repo.branch("hires/3", first).unwrap();
    repo.commit(
        "hires/3",
        &[write("memory/h.md", "h")],
        "turn",
        by(3),
    )
    .unwrap();
    repo.commit(MAIN, &[write("memory/n.md", "n")], "later", by(4))
        .unwrap();
    let secret = blob_id(b"secret-123");
    assert!(repo.has_blob(secret));
    drop(repo);

    let moved = purge(&path, "memory/key.md").unwrap();
    assert_eq!(moved.len(), 2);
    let repo = Repository::open(&path).unwrap();
    assert!(
        !repo.has_blob(secret),
        "the secret is gone from the objects"
    );
    for (branch, _, new) in &moved {
        assert_eq!(repo.tip(branch).unwrap(), Some(*new));
        for commit in repo.history(*new, 100).unwrap() {
            assert_eq!(text(&repo, commit.id, "memory/key.md"), None);
            assert_eq!(
                text(&repo, commit.id, "CHARACTER.md").as_deref(),
                Some("a")
            );
        }
    }
    let v1 = repo.resolve("v1").unwrap().unwrap();
    assert_eq!(text(&repo, v1, "memory/key.md"), None);
    assert_eq!(repo.versions().unwrap()[0].note, "first");
    assert_eq!(
        text(
            &repo,
            repo.tip("hires/3").unwrap().unwrap(),
            "memory/h.md"
        )
        .as_deref(),
        Some("h")
    );
}

#[test]
fn a_commit_on_a_base_moves_no_branch() {
    let dir = Scratch::new("git-commit-on");
    let repo = repository(&dir);
    let base = repo
        .commit(MAIN, &[write("memory/a.md", "a")], "made", by(1))
        .unwrap();
    let on = repo
        .commit_on(
            Some(base),
            &[write("memory/b.md", "b")],
            "on",
            by(2),
        )
        .unwrap();
    assert_eq!(repo.tip(MAIN).unwrap(), Some(base));
    assert_eq!(repo.history(on, 1).unwrap()[0].parents, [base]);
    assert_eq!(text(&repo, on, "memory/a.md").as_deref(), Some("a"));
    assert_eq!(text(&repo, on, "memory/b.md").as_deref(), Some("b"));
    assert_eq!(
        repo.commit_on(
            Some(base),
            &[write("memory/a.md", "a")],
            "same",
            by(3)
        )
        .unwrap(),
        base,
        "a change that leaves the files as they were makes no commit"
    );

    let root = repo
        .commit_on(None, &[write("SKILL.md", "s")], "offer", by(4))
        .unwrap();
    assert!(repo.history(root, 1).unwrap()[0].parents.is_empty());
    assert_eq!(repo.entries(root).unwrap().len(), 1);
    assert_eq!(repo.branches().unwrap().len(), 1);
}

#[test]
fn a_commit_if_the_tip_moved_moves_nothing() {
    let dir = Scratch::new("git-commit-if");
    let repo = repository(&dir);
    let read = repo
        .commit_if(
            MAIN,
            None,
            &[write("memory/a.md", "a")],
            "made",
            by(1),
        )
        .unwrap();
    let moved = repo
        .commit(MAIN, &[write("memory/a.md", "moved")], "e", by(2))
        .unwrap();

    assert!(
        repo.commit_if(
            MAIN,
            Some(read),
            &[write("memory/b.md", "b")],
            "late",
            by(3)
        )
        .is_err()
    );
    assert!(
        repo.commit_if(
            MAIN,
            Some(read),
            &[write("memory/a.md", "moved")],
            "same",
            by(3)
        )
        .is_err()
    );
    assert!(
        repo.commit_if(
            MAIN,
            None,
            &[write("memory/c.md", "c")],
            "anew",
            by(3)
        )
        .is_err()
    );
    assert_eq!(repo.tip(MAIN).unwrap(), Some(moved));

    let kept = repo
        .commit_if(
            MAIN,
            Some(moved),
            &[write("memory/b.md", "b")],
            "on time",
            by(4),
        )
        .unwrap();
    assert_eq!(repo.tip(MAIN).unwrap(), Some(kept));
    assert_eq!(repo.history(kept, 1).unwrap()[0].parents, [moved]);
}

#[test]
fn a_merge_of_two_commits_keeps_the_winning_side_of_a_clash() {
    let dir = Scratch::new("git-merge-commits");
    let repo = repository(&dir);
    let base = repo
        .commit(
            MAIN,
            &[write("memory/notes.md", "a\n")],
            "made",
            by(1),
        )
        .unwrap();
    let tip = repo
        .commit(
            MAIN,
            &[
                write("memory/notes.md", "chief\n"),
                write("memory/other.md", "o\n"),
            ],
            "e",
            by(2),
        )
        .unwrap();
    let saved = repo
        .commit_on(
            Some(base),
            &[write("memory/notes.md", "hire\n")],
            "save",
            by(3),
        )
        .unwrap();

    let merged = repo
        .merge_commits(tip, saved, Wins::From, "joined", by(4))
        .unwrap();
    assert_eq!(merged.joined, Joined::Committed);
    assert_eq!(merged.clashes, ["memory/notes.md"]);
    assert_eq!(
        text(&repo, merged.commit, "memory/notes.md").as_deref(),
        Some("hire\n")
    );
    assert_eq!(
        text(&repo, merged.commit, "memory/other.md").as_deref(),
        Some("o\n")
    );
    assert_eq!(
        repo.history(merged.commit, 1).unwrap()[0].parents,
        [tip, saved]
    );
    assert_eq!(repo.tip(MAIN).unwrap(), Some(tip));

    let kept = repo
        .merge_commits(tip, saved, Wins::Onto, "joined", by(4))
        .unwrap();
    assert_eq!(
        text(&repo, kept.commit, "memory/notes.md").as_deref(),
        Some("chief\n")
    );
}

#[test]
fn a_merge_where_one_side_holds_the_other_adds_no_commit() {
    let dir = Scratch::new("git-merge-held");
    let repo = repository(&dir);
    let old = repo
        .commit(MAIN, &[write("memory/a.md", "a")], "made", by(1))
        .unwrap();
    let new = repo
        .commit(MAIN, &[write("memory/b.md", "b")], "e", by(2))
        .unwrap();

    let held = repo
        .merge_commits(new, old, Wins::From, "m", by(3))
        .unwrap();
    assert_eq!((held.commit, held.joined), (new, Joined::UpToDate));
    let forward = repo
        .merge_commits(old, new, Wins::Onto, "m", by(3))
        .unwrap();
    assert_eq!(
        (forward.commit, forward.joined),
        (new, Joined::FastForward)
    );
    assert_eq!(repo.history(new, 10).unwrap().len(), 2);
}

#[test]
fn a_save_repeated_later_onto_a_tip_that_holds_it_adds_no_commit() {
    let dir = Scratch::new("git-merge-repeat");
    let repo = repository(&dir);
    let base = repo
        .commit(MAIN, &[write("memory/a.md", "a")], "made", by(1))
        .unwrap();
    let moved = repo
        .commit(MAIN, &[write("memory/m.md", "m")], "e", by(2))
        .unwrap();
    let changes = [write("memory/a.md", "saved")];
    let save = |at| {
        let saved = repo
            .commit_on(Some(base), &changes, "save", by(at))
            .unwrap();
        let tip = repo.tip(MAIN).unwrap().unwrap();
        let merged = repo
            .merge_commits(tip, saved, Wins::From, "save", by(at))
            .unwrap();
        if merged.commit != tip {
            repo.set_branch(MAIN, merged.commit, tip).unwrap();
        }
        merged
    };

    let first = save(3);
    assert_eq!(first.joined, Joined::Committed);
    let again = save(4);
    assert_eq!(
        (again.commit, again.joined),
        (first.commit, Joined::UpToDate)
    );
    assert_eq!(repo.tip(MAIN).unwrap(), Some(first.commit));
    assert_eq!(
        repo.first_parents(first.commit, Some(base), 10)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        [first.commit, moved],
        "the branch's own commits stay on its first parents"
    );
}

#[test]
fn a_first_parent_walk_skips_what_a_merge_brought_in() {
    let dir = Scratch::new("git-first-parents");
    let repo = repository(&dir);
    let first = repo
        .commit(MAIN, &[write("memory/a.md", "a")], "made", by(1))
        .unwrap();
    repo.branch("hires/2", first).unwrap();
    let own = repo
        .commit(
            "hires/2",
            &[write("memory/h.md", "h")],
            "turn",
            by(2),
        )
        .unwrap();
    let main = repo
        .commit(MAIN, &[write("memory/m.md", "m")], "e", by(3))
        .unwrap();
    let merged =
        repo.merge("hires/2", main, "took in main", by(4)).unwrap();
    assert_eq!(repo.history(merged.commit, 10).unwrap().len(), 4);

    let ids = |walk: Vec<Commit>| {
        walk.into_iter().map(|c| c.id).collect::<Vec<_>>()
    };
    assert_eq!(
        ids(repo.first_parents(merged.commit, None, 10).unwrap()),
        [merged.commit, own, first]
    );
    assert_eq!(
        ids(repo
            .first_parents(merged.commit, Some(first), 10)
            .unwrap()),
        [merged.commit, own]
    );
    assert_eq!(
        ids(repo.first_parents(merged.commit, None, 1).unwrap()),
        [merged.commit]
    );
}

#[test]
fn a_settle_cut_after_its_own_renames_keeps_the_purged_repository() {
    let dir = Scratch::new("git-settle-cut");
    let path = dir.join("profile.git");
    let old =
        Repository::create(&dir.join("profile.git.purged")).unwrap();
    old.commit(
        MAIN,
        &[write("memory/key.md", "secret")],
        "made",
        by(1),
    )
    .unwrap();
    let fresh = Repository::create(&path).unwrap();
    let kept = fresh
        .commit(MAIN, &[write("CHARACTER.md", "a")], "made", by(1))
        .unwrap();
    drop((old, fresh));
    std::fs::write(path.join("PURGED"), b"").unwrap();

    settle(&path).unwrap();
    assert_eq!(
        Repository::open(&path).unwrap().tip(MAIN).unwrap(),
        Some(kept)
    );
    assert!(!path.join("PURGED").exists());
    assert!(!dir.join("profile.git.purged").exists());
}

#[test]
fn a_purge_cut_between_its_renames_is_settled_whole() {
    let dir = Scratch::new("git-settle");
    let path = dir.join("profile.git");
    let old =
        Repository::create(&dir.join("profile.git.purged")).unwrap();
    old.commit(
        MAIN,
        &[write("memory/key.md", "secret")],
        "made",
        by(1),
    )
    .unwrap();
    let fresh =
        Repository::create(&dir.join("profile.git.purging")).unwrap();
    let kept = fresh
        .commit(MAIN, &[write("CHARACTER.md", "a")], "made", by(1))
        .unwrap();
    drop((old, fresh));
    std::fs::write(dir.join("profile.git.purging/PURGED"), b"")
        .unwrap();

    settle(&path).unwrap();
    let repo = Repository::open(&path).unwrap();
    assert_eq!(repo.tip(MAIN).unwrap(), Some(kept));
    assert!(!dir.join("profile.git.purged").exists());
    assert!(!dir.join("profile.git.purging").exists());
    settle(&path).unwrap();
    assert_eq!(
        Repository::open(&path).unwrap().tip(MAIN).unwrap(),
        Some(kept)
    );
}

#[test]
fn a_write_keeps_whether_its_file_runs() {
    let dir = Scratch::new("git-executable");
    let repo = repository(&dir);
    let tip = repo.tip(MAIN).unwrap();
    let runs = repo
        .commit_on(
            tip,
            &[Change::WriteExecutable {
                path: "skills/run.sh".into(),
                bytes: b"echo".to_vec(),
            }],
            "runs",
            by(2),
        )
        .unwrap();
    let plain = repo
        .commit_on(
            Some(runs),
            &[write("skills/run.sh", "echo")],
            "plain",
            by(3),
        )
        .unwrap();

    let runs_at = |at| {
        repo.entries(at)
            .unwrap()
            .into_iter()
            .find(|entry| entry.path == "skills/run.sh")
            .map(|entry| entry.executable)
    };
    assert_eq!(runs_at(runs), Some(true));
    assert_eq!(runs_at(plain), Some(false));
}

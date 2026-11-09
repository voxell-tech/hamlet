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
    Repository::create(&dir.join("scholar.git")).unwrap()
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
    // gift in its inbox and a stray character file.
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
            blob: blob_id(b"gift"),
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
fn a_fork_starts_from_a_version_of_another_scholar() {
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
    let path = dir.join("scholar.git");
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

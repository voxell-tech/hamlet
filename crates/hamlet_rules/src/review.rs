//! Read-only Git comparisons and the complete lines of one changed
//! file.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use similar::{ChangeTag, TextDiff};

/// The two sides of a review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    Unstaged,
    Staged,
    Revisions {
        base: String,
        target: String,
        merge_base: bool,
    },
}

/// A changed path, including the original name of a rename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
    pub before: Option<PathBuf>,
    pub status: char,
    old_id: String,
    new_id: String,
    old_mode: String,
    new_mode: String,
}

/// A repository's file list with revision names resolved to commits.
#[derive(Clone, Debug)]
pub struct Review {
    pub root: PathBuf,
    pub files: Vec<File>,
    comparison: Comparison,
}

/// A line's role in a patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Context,
    Delete,
    Add,
}

/// A source line and its one-based coordinates on either side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub mark: Mark,
    pub text: String,
    pub no_newline: bool,
}

/// A full-context line diff. Non-text changes have a description
/// instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub lines: Vec<Line>,
    pub description: String,
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_SHALLOW_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(name);
    }
    command
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
        ]);
    command
}

fn checked(output: Output) -> Result<Vec<u8>> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.trim().is_empty() {
            bail!("Git failed with {}", output.status);
        }
        bail!("{}", stderr.trim());
    }
    Ok(output.stdout)
}

fn commit(root: &Path, name: &str, field: &str) -> Result<String> {
    let output = git(root)
        .args(["rev-parse", "--verify", "--end-of-options"])
        .arg(format!("{name}^{{commit}}"))
        .output()?;
    let bytes = checked(output).with_context(|| {
        format!("Invalid {field} revision {name:?}")
    })?;
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}

fn check_folder(folder: &Path) -> Result<()> {
    if folder.as_os_str().is_empty() {
        bail!("Enter a repository folder.");
    }
    let metadata = std::fs::metadata(folder).with_context(|| {
        format!("Cannot open repository folder {}", folder.display())
    })?;
    if !metadata.is_dir() {
        bail!(
            "Repository path is not a folder: {}",
            folder.display()
        );
    }
    Ok(())
}

fn path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes.to_vec()).into()
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
            .into()
    }
}

impl Review {
    /// Whether a folder is inside a Git working tree.
    pub fn is_repository(folder: &Path) -> Result<bool> {
        check_folder(folder)?;
        let out = git(folder)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .context(
                "Unable to run Git. Check that Git is installed and \
                 on PATH.",
            )?;
        if !out.status.success()
            && String::from_utf8_lossy(&out.stderr)
                .starts_with("fatal: not a git repository")
        {
            return Ok(false);
        }
        Ok(checked(out)? == b"true\n")
    }

    /// Reads tracked changes and, for the working tree, untracked
    /// files.
    pub fn read(
        folder: &Path,
        mut comparison: Comparison,
    ) -> Result<Self> {
        check_folder(folder)?;
        let root = checked(
            git(folder)
                .args(["rev-parse", "--show-toplevel"])
                .output()
                .context(
                    "Unable to run Git. Check that Git is installed \
                     and on PATH.",
                )?,
        )?;
        let root = path(root.strip_suffix(b"\n").unwrap_or(&root));
        if let Comparison::Revisions {
            base,
            target,
            merge_base,
        } = &mut comparison
        {
            *base = commit(&root, base, "BASE")?;
            *target = commit(&root, target, "TARGET")?;
            if *merge_base {
                let output = git(&root)
                    .args(["merge-base", base, target])
                    .output()?;
                if output.status.code() == Some(1)
                    && output.stderr.is_empty()
                {
                    bail!(
                        "BASE and TARGET have no common ancestor. \
                         Use Direct to compare unrelated histories."
                    );
                }
                *base = String::from_utf8(checked(output)?)?
                    .trim()
                    .to_owned();
            }
        }
        let mut review = Self {
            root,
            files: Vec::new(),
            comparison,
        };
        let bytes = checked(
            review
                .command()
                .args(["--raw", "--no-abbrev", "-z", "--"])
                .output()?,
        )?;
        let mut fields =
            bytes.split(|b| *b == 0).filter(|s| !s.is_empty());
        while let Some(header) = fields.next() {
            let header = std::str::from_utf8(header)?;
            let parts = header
                .strip_prefix(':')
                .context("Missing Git raw header")?
                .split_whitespace()
                .collect::<Vec<_>>();
            let [old_mode, new_mode, old_id, new_id, status] =
                parts.as_slice()
            else {
                bail!("Invalid Git raw header");
            };
            let status = status.as_bytes()[0] as char;
            let first =
                path(fields.next().context("Missing Git path")?);
            let (before, path) = if matches!(status, 'R' | 'C') {
                (
                    Some(first),
                    path(
                        fields
                            .next()
                            .context("Missing renamed Git path")?,
                    ),
                )
            } else {
                (None, first)
            };
            review.files.push(File {
                path,
                before,
                status,
                old_id: (*old_id).into(),
                new_id: (*new_id).into(),
                old_mode: (*old_mode).into(),
                new_mode: (*new_mode).into(),
            });
        }
        let unmerged = review
            .files
            .iter()
            .filter(|file| file.status == 'U')
            .map(|file| file.path.clone())
            .collect::<HashSet<_>>();
        review.files.retain(|file| {
            file.status == 'U' || !unmerged.contains(&file.path)
        });
        if review.comparison == Comparison::Unstaged {
            let bytes = checked(
                git(&review.root)
                    .args([
                        "ls-files",
                        "--others",
                        "--exclude-standard",
                        "-z",
                    ])
                    .output()?,
            )?;
            review.files.extend(
                bytes
                    .split(|b| *b == 0)
                    .filter(|s| !s.is_empty())
                    .map(|p| File {
                        path: path(p),
                        before: None,
                        status: '?',
                        old_id: String::new(),
                        new_id: String::new(),
                        old_mode: "000000".into(),
                        new_mode: "100644".into(),
                    }),
            );
        }
        Ok(review)
    }

    fn command(&self) -> Command {
        let mut command = git(&self.root);
        command.args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--find-renames",
        ]);
        match &self.comparison {
            Comparison::Unstaged => {}
            Comparison::Staged => {
                command.arg("--cached");
            }
            Comparison::Revisions { base, target, .. } => {
                command.args([base, target]);
            }
        }
        command
    }

    /// Compares the file's blobs or working-tree contents with
    /// `similar`.
    pub fn diff(&self, file: &File) -> Result<Diff> {
        if file.status == 'U' {
            return Ok(Diff {
                description: "Unmerged file. Resolve conflicts \
                              before reviewing this file."
                    .into(),
                ..Diff::default()
            });
        }
        if file.old_mode == "160000" || file.new_mode == "160000" {
            return Ok(Diff {
                description: format!(
                    "Submodule changed: {} -> {}. Review its \
                     repository separately.",
                    file.old_id, file.new_id
                ),
                ..Diff::default()
            });
        }
        let old = self.blob(&file.old_id)?;
        let new = if file.status == 'D' {
            Vec::new()
        } else if self.comparison == Comparison::Unstaged {
            let path = self.root.join(&file.path);
            let kind = std::fs::symlink_metadata(&path)?.file_type();
            if kind.is_dir() {
                return Ok(Diff {
                    description: "Nested repository. Open this \
                                  folder separately to review its \
                                  changes."
                        .into(),
                    ..Diff::default()
                });
            }
            if kind.is_symlink() {
                std::fs::read_link(path)?
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec()
            } else if kind.is_file() {
                let attributes = checked(
                    git(&self.root)
                        .args([
                            "check-attr",
                            "-z",
                            "filter",
                            "ident",
                            "working-tree-encoding",
                            "--",
                        ])
                        .arg(&file.path)
                        .output()?,
                )?;
                let fields =
                    attributes.split(|b| *b == 0).collect::<Vec<_>>();
                let transformations = fields
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter(|field| {
                        !matches!(field[2], b"unspecified" | b"unset")
                    })
                    .map(|field| String::from_utf8_lossy(field[1]))
                    .collect::<Vec<_>>();
                if !transformations.is_empty() {
                    return Ok(Diff {
                        description: format!(
                            "Working-tree preview unavailable: Git \
                             attributes ({}) transform this file. \
                             Review staged or committed changes \
                             instead.",
                            transformations.join(", ")
                        ),
                        ..Diff::default()
                    });
                }
                let mut bytes = std::fs::read(path)?;
                self.normalize_eol(file, &mut bytes)?;
                bytes
            } else {
                bail!("Not a regular file");
            }
        } else {
            self.blob(&file.new_id)?
        };
        Ok(Diff::between(&old, &new))
    }

    fn normalize_eol(
        &self,
        file: &File,
        bytes: &mut Vec<u8>,
    ) -> Result<()> {
        if !bytes.windows(2).any(|pair| pair == b"\r\n") {
            return Ok(());
        }
        // Git supplies resolved attributes and its binary/index
        // classifications without running content filters.
        let info = checked(
            git(&self.root)
                .args([
                    "ls-files",
                    "--eol",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                    "--",
                ])
                .arg(&file.path)
                .output()?,
        )?;
        let header =
            info.split(|b| *b == b'\t').next().unwrap_or_default();
        let mut fields =
            std::str::from_utf8(header)?.split_whitespace();
        let index = fields.next().unwrap_or_default();
        let working = fields.next().unwrap_or_default();
        let attribute = fields.next().unwrap_or_default();
        let auto = if attribute == "attr/" {
            let config = checked(
                git(&self.root)
                    .args([
                        "config",
                        "--type=bool-or-str",
                        "--default=false",
                        "--get",
                        "core.autocrlf",
                    ])
                    .output()?,
            )?;
            matches!(config.as_slice(), b"true\n" | b"input\n")
        } else {
            attribute == "attr/text=auto"
        };
        // Auto conversion preserves binary content and CRLF already
        // stored in the index.
        if attribute == "attr/text"
            || (auto
                && working != "w/-text"
                && !matches!(index, "i/crlf" | "i/mixed"))
        {
            *bytes = bytes
                .iter()
                .enumerate()
                .filter_map(|(i, &byte)| {
                    (byte != b'\r'
                        || bytes.get(i + 1) != Some(&b'\n'))
                    .then_some(byte)
                })
                .collect();
        }
        Ok(())
    }

    fn blob(&self, id: &str) -> Result<Vec<u8>> {
        if id.bytes().all(|b| b == b'0') {
            return Ok(Vec::new());
        }
        checked(
            git(&self.root)
                .args(["cat-file", "blob", id])
                .output()?,
        )
    }
}

impl Diff {
    /// Compares complete file contents, preserving LF-based line
    /// numbers and final-newline changes.
    pub fn between(old: &[u8], new: &[u8]) -> Self {
        if old == new {
            return Self {
                description: "No text changes (rename, file mode, \
                              or empty file)."
                    .into(),
                ..Self::default()
            };
        }
        if old.contains(&0) || new.contains(&0) {
            return Self {
                description: "Binary file changed. Text preview is \
                              unavailable."
                    .into(),
                ..Self::default()
            };
        }
        let (Ok(old), Ok(new)) =
            (std::str::from_utf8(old), std::str::from_utf8(new))
        else {
            return Self {
                description: "Non-UTF-8 content. Text preview is \
                              unavailable."
                    .into(),
                ..Self::default()
            };
        };
        // Split only on LF: a lone CR is source content, not another
        // line.
        let old = old.split_inclusive('\n').collect::<Vec<_>>();
        let new = new.split_inclusive('\n').collect::<Vec<_>>();
        let diff = TextDiff::configure().diff_slices(&old, &new);
        let lines = diff
            .iter_all_changes()
            .map(|change| {
                let value = change.value();
                Line {
                    old: change.old_index().map(|i| i + 1),
                    new: change.new_index().map(|i| i + 1),
                    mark: match change.tag() {
                        ChangeTag::Equal => Mark::Context,
                        ChangeTag::Delete => Mark::Delete,
                        ChangeTag::Insert => Mark::Add,
                    },
                    text: value
                        .strip_suffix('\n')
                        .unwrap_or(value)
                        .to_owned(),
                    no_newline: !value.ends_with('\n'),
                }
            })
            .collect();
        Self {
            lines,
            description: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn run(root: &Path, args: &[&str]) {
        let output =
            hamlet_testing::git(root).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn unrelated_histories_report_an_error_but_allow_direct_comparison()
     {
        let root = Scratch::new("git-review-unrelated");
        run(&root, &["init", "-b", "main"]);
        run(&root, &["config", "user.name", "Test"]);
        run(&root, &["config", "user.email", "test@example.invalid"]);
        run(&root, &["commit", "--allow-empty", "-m", "main"]);
        run(&root, &["checkout", "--orphan", "other"]);
        std::fs::write(root.join("other.txt"), "other\n").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-m", "other"]);
        let compare = |base: &str, target: &str, merge_base| {
            Review::read(
                &root,
                Comparison::Revisions {
                    base: base.into(),
                    target: target.into(),
                    merge_base,
                },
            )
        };
        let error =
            compare("main", "other", true).unwrap_err().to_string();
        assert!(error.contains("no common ancestor"), "{error}");
        let direct = compare("main", "other", false).unwrap();
        assert_eq!(direct.files.len(), 1);
        assert_eq!(
            direct.diff(&direct.files[0]).unwrap().lines[0].text,
            "other"
        );
        for (base, target, field, name) in [
            ("missing", "other", "BASE", "missing"),
            ("main", "missing", "TARGET", "missing"),
            ("", "other", "BASE", ""),
        ] {
            let error =
                compare(base, target, false).unwrap_err().to_string();
            assert!(
                error.contains(field) && error.contains(name),
                "{error}"
            );
        }
    }

    #[test]
    fn conflicts_have_one_notice_per_path() {
        let root = Scratch::new("git-review-conflict");
        run(&root, &["init", "-b", "main"]);
        run(&root, &["config", "user.name", "Test"]);
        run(&root, &["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-m", "base"]);
        run(&root, &["checkout", "-b", "other"]);
        std::fs::write(root.join("file.txt"), "other\n").unwrap();
        run(&root, &["commit", "-am", "other"]);
        run(&root, &["checkout", "main"]);
        std::fs::write(root.join("file.txt"), "main\n").unwrap();
        run(&root, &["commit", "-am", "main"]);
        assert!(
            !hamlet_testing::git(&root)
                .args(["merge", "other"])
                .output()
                .unwrap()
                .status
                .success()
        );
        for comparison in [Comparison::Unstaged, Comparison::Staged] {
            let review = Review::read(&root, comparison).unwrap();
            assert_eq!(review.files.len(), 1);
            assert_eq!(review.files[0].status, 'U');
            assert!(
                review
                    .diff(&review.files[0])
                    .unwrap()
                    .description
                    .contains("Unmerged")
            );
        }
    }

    #[test]
    fn working_tree_transformations_show_notices_but_staged_blobs_remain_reviewable()
     {
        let root = Scratch::new("git-review-transformations");
        run(&root, &["init", "-b", "main"]);
        std::fs::write(root.join("file.txt"), "one\ntwo\nthree\n")
            .unwrap();
        run(&root, &["add", "."]);
        for attribute in
            ["filter=custom", "ident", "working-tree-encoding=UTF-16"]
        {
            let text = "one\nTWO\nthree\n";
            let bytes =
                if attribute.starts_with("working-tree-encoding") {
                    std::iter::once(0xfeff)
                        .chain(text.encode_utf16())
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    text.as_bytes().to_vec()
                };
            std::fs::write(root.join("file.txt"), bytes).unwrap();
            std::fs::write(
                root.join(".gitattributes"),
                format!("*.txt {attribute}\n"),
            )
            .unwrap();
            let review =
                Review::read(&root, Comparison::Staged).unwrap();
            assert_eq!(
                review.diff(&review.files[0]).unwrap().lines.len(),
                3
            );
            let working =
                Review::read(&root, Comparison::Unstaged).unwrap();
            let file = working
                .files
                .iter()
                .find(|f| f.path == Path::new("file.txt"))
                .unwrap();
            let diff = working.diff(file).unwrap();
            assert!(diff.lines.is_empty(), "{attribute}");
            assert!(
                diff.description
                    .contains(attribute.split('=').next().unwrap()),
                "{}",
                diff.description
            );
        }
        std::fs::write(
            root.join(".gitattributes"),
            "*.txt -filter -ident -working-tree-encoding\n",
        )
        .unwrap();
        std::fs::write(root.join("file.txt"), "one\nTWO\nthree\n")
            .unwrap();
        let review =
            Review::read(&root, Comparison::Unstaged).unwrap();
        let file = review
            .files
            .iter()
            .find(|f| f.path == Path::new("file.txt"))
            .unwrap();
        let diff = review.diff(file).unwrap();
        assert_eq!(
            diff.lines
                .iter()
                .filter(|l| l.mark == Mark::Context)
                .count(),
            2
        );
    }

    #[test]
    fn working_tree_eol_conversion_follows_git_policy() {
        for (attribute, autocrlf, initial, context) in [
            ("text eol=crlf", "false", "one\r\ntwo\r\nthree\r\n", 2),
            ("text=auto", "false", "one\r\ntwo\r\nthree\r\n", 2),
            ("!text", "true", "one\r\ntwo\r\nthree\r\n", 2),
            ("!text", "input", "one\r\ntwo\r\nthree\r\n", 2),
            ("-text", "true", "one\ntwo\nthree\n", 0),
            ("!text", "false", "one\ntwo\nthree\n", 0),
        ] {
            let root = Scratch::new("git-review-eol");
            run(&root, &["init", "-b", "main"]);
            run(&root, &["config", "core.autocrlf", autocrlf]);
            std::fs::write(
                root.join(".gitattributes"),
                format!("*.txt {attribute}\n"),
            )
            .unwrap();
            std::fs::write(root.join("file.txt"), initial).unwrap();
            run(&root, &["add", "."]);
            std::fs::write(
                root.join("file.txt"),
                "one\r\nTWO\r\nthree\r\n",
            )
            .unwrap();
            let review =
                Review::read(&root, Comparison::Unstaged).unwrap();
            let file = review
                .files
                .iter()
                .find(|f| f.path == Path::new("file.txt"))
                .unwrap();
            let diff = review.diff(file).unwrap();
            assert_eq!(
                diff.lines
                    .iter()
                    .filter(|l| l.mark == Mark::Context)
                    .count(),
                context,
                "{attribute}, autocrlf={autocrlf}"
            );
            assert_eq!(
                diff.lines
                    .iter()
                    .filter(|l| l.mark == Mark::Add)
                    .count(),
                3 - context
            );
            assert_eq!(
                diff.lines
                    .iter()
                    .filter(|l| l.mark == Mark::Delete)
                    .count(),
                3 - context
            );
        }
    }

    #[test]
    fn automatic_eol_conversion_preserves_index_crlf_and_lone_carriage_returns()
     {
        for (attribute, initial, working, context) in [
            (
                "text=auto",
                "one\r\ntwo\r\nthree\r\n",
                "one\r\nTWO\r\nthree\r\n",
                2,
            ),
            (
                "text=auto",
                "one\nkeep\rcarriage\nthree\n",
                "one\r\nkeep\rcarriage\nTHREE\r\n",
                1,
            ),
            (
                "text",
                "one\nkeep\rcarriage\nthree\n",
                "one\r\nkeep\rcarriage\nTHREE\r\n",
                2,
            ),
        ] {
            let root = Scratch::new("git-review-eol-safety");
            run(&root, &["init", "-b", "main"]);
            std::fs::write(
                root.join(".gitattributes"),
                "*.txt -text\n",
            )
            .unwrap();
            std::fs::write(root.join("file.txt"), initial).unwrap();
            run(&root, &["add", "."]);
            std::fs::write(
                root.join(".gitattributes"),
                format!("*.txt {attribute}\n"),
            )
            .unwrap();
            std::fs::write(root.join("file.txt"), working).unwrap();
            let review =
                Review::read(&root, Comparison::Unstaged).unwrap();
            let file = review
                .files
                .iter()
                .find(|f| f.path == Path::new("file.txt"))
                .unwrap();
            let diff = review.diff(file).unwrap();
            assert_eq!(
                diff.lines
                    .iter()
                    .filter(|l| l.mark == Mark::Context)
                    .count(),
                context
            );
            assert_eq!(
                diff.lines
                    .iter()
                    .filter(|l| l.mark == Mark::Add)
                    .count(),
                3 - context
            );
            assert_eq!(
                diff.lines
                    .iter()
                    .filter(|l| l.mark == Mark::Delete)
                    .count(),
                3 - context
            );
        }
    }

    #[test]
    fn nested_repositories_have_a_notice_and_other_files_remain_readable()
     {
        let root = Scratch::new("git-review-nested");
        run(&root, &["init", "-b", "main"]);
        run(&root, &["init", "-b", "main", "nested"]);
        run(
            &root.join("nested"),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "nested",
            ],
        );
        std::fs::write(root.join("z.txt"), "readable\n").unwrap();
        let review =
            Review::read(&root, Comparison::Unstaged).unwrap();
        assert_eq!(review.files.len(), 2);
        let nested = review.diff(&review.files[0]).unwrap();
        assert!(nested.lines.is_empty());
        assert!(nested.description.contains("Nested repository"));
        let other = review.diff(&review.files[1]).unwrap();
        assert_eq!(other.lines[0].text, "readable");
    }

    #[test]
    fn repository_detection_includes_nested_folders_and_linked_worktrees()
     {
        let root = Scratch::new("git-review-capability");
        assert!(!Review::is_repository(&root).unwrap());
        run(&root, &["init", "-b", "main"]);
        run(&root, &["config", "user.email", "test@example.invalid"]);
        run(&root, &["config", "user.name", "Test"]);
        run(&root, &["commit", "--allow-empty", "-m", "base"]);
        let nested = root.join("docs/nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(Review::is_repository(&nested).unwrap());
        run(
            &root,
            &["worktree", "add", "--detach", "linked", "HEAD"],
        );
        assert!(Review::is_repository(&root.join("linked")).unwrap());
        run(&root, &["init", "--bare", "bare.git"]);
        assert!(
            !Review::is_repository(&root.join("bare.git")).unwrap()
        );
    }

    #[test]
    fn line_coordinates_preserve_carriage_returns_and_final_newlines()
    {
        let diff = Diff::between(
            "same\r\nkeep\rcarriage\n旧\nlast".as_bytes(),
            "same\r\nkeep\rcarriage\n新\nextra\nlast\n".as_bytes(),
        );
        assert_eq!(
            diff.lines
                .iter()
                .map(|line| (
                    line.old,
                    line.new,
                    line.mark,
                    line.text.as_str(),
                    line.no_newline
                ))
                .collect::<Vec<_>>(),
            [
                (Some(1), Some(1), Mark::Context, "same\r", false),
                (
                    Some(2),
                    Some(2),
                    Mark::Context,
                    "keep\rcarriage",
                    false
                ),
                (Some(3), None, Mark::Delete, "旧", false),
                (Some(4), None, Mark::Delete, "last", true),
                (None, Some(3), Mark::Add, "新", false),
                (None, Some(4), Mark::Add, "extra", false),
                (None, Some(5), Mark::Add, "last", false),
            ]
        );
    }

    #[test]
    fn working_tree_index_and_revisions_keep_distinct_sides() {
        let root = Scratch::new("git-review");
        run(&root, &["init", "-b", "main"]);
        run(&root, &["config", "user.email", "test@example.invalid"]);
        run(&root, &["config", "user.name", "Test"]);
        let name = "odd [name].rs";
        std::fs::write(root.join(name), "one\ntwo\nthree\nfour\n")
            .unwrap();
        run(&root, &["add", "--", name]);
        let unborn = Review::read(&root, Comparison::Staged).unwrap();
        assert_eq!(
            unborn.diff(&unborn.files[0]).unwrap().lines[0].old,
            None
        );
        run(&root, &["commit", "-m", "base"]);
        std::fs::write(
            root.join(name),
            "one\nreplacement\nfour\nextra",
        )
        .unwrap();
        run(&root, &["add", "--", name]);
        std::fs::write(
            root.join(name),
            "one\nworking\nfour\nextra\n",
        )
        .unwrap();
        let long = "long line ".repeat(40);
        std::fs::write(
            root.join("untracked.txt"),
            format!("{}{long}", "new\n".repeat(501)),
        )
        .unwrap();
        let staged = Review::read(&root, Comparison::Staged).unwrap();
        assert_eq!(staged.files.len(), 1);
        let diff = staged.diff(&staged.files[0]).unwrap();
        assert_eq!(
            diff.lines
                .iter()
                .filter(|l| l.mark == Mark::Delete)
                .map(|l| (l.old, l.text.as_str()))
                .collect::<Vec<_>>(),
            [(Some(2), "two"), (Some(3), "three")]
        );
        assert!(diff.lines.last().unwrap().no_newline);
        let staged_diff = diff;
        let working =
            Review::read(&root, Comparison::Unstaged).unwrap();
        assert_eq!(working.files.len(), 2);
        let diff = working.diff(&working.files[0]).unwrap();
        assert!(diff.lines.iter().any(
            |l| l.mark == Mark::Delete && l.text == "replacement"
        ));
        assert!(diff.lines.iter().any(|l| l.mark == Mark::Add
            && l.new == Some(2)
            && l.text == "working"));
        let untracked = working.diff(&working.files[1]).unwrap();
        assert_eq!(untracked.lines.len(), 502);
        assert_eq!(untracked.lines[501].new, Some(502));
        assert_eq!(untracked.lines[501].text, long);
        assert!(untracked.lines[501].no_newline);
        run(&root, &["commit", "-m", "staged"]);
        let revisions = Review::read(
            &root,
            Comparison::Revisions {
                base: "HEAD~1".into(),
                target: "HEAD".into(),
                merge_base: false,
            },
        )
        .unwrap();
        assert_eq!(
            revisions.diff(&revisions.files[0]).unwrap(),
            staged_diff
        );
        run(&root, &["add", "--", name]);
        assert_eq!(
            staged.diff(&staged.files[0]).unwrap(),
            staged_diff
        );
        std::fs::remove_file(root.join(name)).unwrap();
        let deleted =
            Review::read(&root, Comparison::Unstaged).unwrap();
        let file =
            deleted.files.iter().find(|f| f.status == 'D').unwrap();
        let diff = deleted.diff(file).unwrap();
        assert_eq!(
            diff.lines
                .iter()
                .map(|l| (l.old, l.new, l.mark, l.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (Some(1), None, Mark::Delete, "one"),
                (Some(2), None, Mark::Delete, "working"),
                (Some(3), None, Mark::Delete, "four"),
                (Some(4), None, Mark::Delete, "extra"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_compare_targets_without_following_them() {
        use std::os::unix::fs::symlink;

        let root = Scratch::new("git-review-symlinks");
        run(&root, &["init", "-b", "main"]);
        let link = root.join("link");
        symlink("missing-old", &link).unwrap();
        run(&root, &["add", "link"]);
        std::fs::remove_file(&link).unwrap();
        symlink("missing-new", &link).unwrap();
        let review =
            Review::read(&root, Comparison::Unstaged).unwrap();
        let diff = review.diff(&review.files[0]).unwrap();
        assert_eq!(
            diff.lines,
            [
                Line {
                    old: Some(1),
                    new: None,
                    mark: Mark::Delete,
                    text: "missing-old".into(),
                    no_newline: true
                },
                Line {
                    old: None,
                    new: Some(1),
                    mark: Mark::Add,
                    text: "missing-new".into(),
                    no_newline: true
                },
            ]
        );
    }

    #[test]
    fn merge_base_renames_and_binary_files_are_reviewable() {
        let root = Scratch::new("git-review-branches");
        run(&root, &["init", "-b", "main"]);
        run(&root, &["config", "user.email", "test@example.invalid"]);
        run(&root, &["config", "user.name", "Test"]);
        std::fs::write(root.join("old.txt"), "unchanged\n").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-m", "base"]);
        run(&root, &["branch", "feature"]);
        std::fs::write(root.join("main.txt"), "main only\n").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-m", "main"]);
        run(&root, &["checkout", "feature"]);
        run(&root, &["mv", "old.txt", "new.txt"]);
        std::fs::write(root.join("binary"), b"\0\x01\x02").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-m", "feature"]);
        let compare = |merge_base| {
            Review::read(
                &root,
                Comparison::Revisions {
                    base: "main".into(),
                    target: "feature".into(),
                    merge_base,
                },
            )
            .unwrap()
        };
        let branch = compare(true);
        assert_eq!(branch.files.len(), 2);
        let renamed =
            branch.files.iter().find(|f| f.status == 'R').unwrap();
        assert_eq!(
            renamed.before.as_deref(),
            Some(Path::new("old.txt"))
        );
        assert!(branch.diff(renamed).unwrap().lines.is_empty());
        let binary = branch
            .files
            .iter()
            .find(|f| f.path == Path::new("binary"))
            .unwrap();
        assert!(
            branch
                .diff(binary)
                .unwrap()
                .description
                .contains("Binary")
        );
        assert!(
            compare(false)
                .files
                .iter()
                .any(|f| f.path == Path::new("main.txt")
                    && f.status == 'D')
        );
        assert!(
            Review::read(
                &root,
                Comparison::Revisions {
                    base: "--help".into(),
                    target: "HEAD".into(),
                    merge_base: false
                }
            )
            .is_err()
        );
    }
}

//! The `edit` tool: one exact replacement in a file of the
//! workspace or the project.

use std::path::{Component, Path};

use serde_json::Value;

use crate::tools::Outcome;

/// The most of a file shown when `old` is not in it.
const SHOWN: usize = 1200;

/// True when `rel`, a path under a project, is one of the places of a
/// git repository that hold what git later runs.
fn guarded(rel: &Path) -> bool {
    let parts = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    parts.windows(2).enumerate().any(|(i, pair)| {
        pair[0] == ".git"
            && (pair[1] == "hooks"
                || pair[1] == "config" && i + 2 == parts.len())
    })
}

/// The folder a path given to `edit` starts from, and the path under
/// it. A full path may name the project or the workspace. A relative
/// one is the project's, unless only the workspace has its first
/// part, as with `NOTES.md` and `skills`.
fn footing<'a>(
    workspace: &'a Path,
    project: Option<&'a Path>,
    given: &'a Path,
) -> (&'a Path, &'a Path) {
    let Some(project) = project else {
        // Models often pass the full path, or one that starts at
        // the workspace's own name; accept it when it is inside the
        // workspace.
        let rel = given.strip_prefix(workspace).unwrap_or(given);
        return match rel.strip_prefix("workspace") {
            Ok(inner) if !workspace.join("workspace").exists() => {
                (workspace, inner)
            }
            _ => (workspace, rel),
        };
    };
    if let Ok(rel) = given.strip_prefix(workspace) {
        return (workspace, rel);
    }
    if let Ok(rel) = given.strip_prefix(project) {
        return (project, rel);
    }
    let first = given.components().next();
    let own = first.is_some_and(|first| {
        !project.join(first).exists()
            && workspace.join(first).exists()
    });
    (if own { workspace } else { project }, given)
}

/// Runs in-process, so it confines itself to the workspace and the
/// project.
pub(super) fn edit(
    workspace: &Path,
    project: Option<&Path>,
    args: &Value,
) -> Outcome {
    // A missing `old` creates the file, as an empty one does.
    let old = args["old"].as_str().unwrap_or_default();
    let (Some(path), Some(new)) =
        (args["path"].as_str(), args["new"].as_str())
    else {
        return Outcome::plain(
            1,
            "Error: `path` and `new` are required",
        );
    };
    let (base, rel) = footing(workspace, project, Path::new(path));
    if rel.is_absolute()
        || rel.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Outcome::plain(
            1,
            match project {
                Some(_) => {
                    "Error: path must be inside the project or your \
                     workspace, without `..`"
                }
                None => {
                    "Error: path must be relative to the workspace, \
                     without `..`"
                }
            },
        );
    }
    let full = base.join(rel);
    let parent = full.parent().unwrap_or(base);
    // A symlink on the way could lead outside, so the deepest part
    // that exists is checked before any folder is created.
    let existing =
        parent.ancestors().find(|p| p.symlink_metadata().is_ok());
    let real = match (
        existing,
        existing.map(Path::canonicalize),
        base.canonicalize(),
    ) {
        (Some(known), Some(Ok(p)), Ok(b)) if p.starts_with(&b) => {
            // The file as it really lies under its folder.
            let rest = full.strip_prefix(known).unwrap_or(&full);
            p.join(rest).strip_prefix(&b).map(Path::to_path_buf)
        }
        _ => {
            return Outcome::plain(
                1,
                "Error: path resolves outside the workspace",
            );
        }
    };
    if project == Some(base) && real.is_ok_and(|real| guarded(&real))
    {
        return Outcome::plain(
            1,
            "Error: .git/hooks and .git/config cannot be edited",
        );
    }
    if let Err(e) = std::fs::create_dir_all(parent) {
        return Outcome::plain(1, format!("Error: {e}"));
    }
    if full.is_symlink() {
        return Outcome::plain(
            1,
            "Error: refusing to edit through a symlink",
        );
    }
    let result = match std::fs::read_to_string(&full) {
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && old.is_empty() =>
        {
            std::fs::write(&full, new)
                .map(|_| format!("created {path}"))
        }
        Err(e) => return Outcome::plain(1, format!("Error: {e}")),
        Ok(_) if old.is_empty() => {
            return Outcome::plain(
                1,
                format!(
                    "Error: {path} already exists. Put the text to \
                     replace in `old`"
                ),
            );
        }
        Ok(text) => match text.matches(old).count() {
            1 => std::fs::write(&full, text.replacen(old, new, 1))
                .map(|_| format!("edited {path}")),
            0 => {
                // Models often get the white space at the ends of
                // `old` wrong.
                let core = old.trim();
                if !core.is_empty() && text.matches(core).count() == 1
                {
                    std::fs::write(
                        &full,
                        text.replacen(core, new.trim(), 1),
                    )
                    .map(|_| format!("edited {path}"))
                } else {
                    let shown =
                        text.chars().take(SHOWN).collect::<String>();
                    return Outcome::plain(
                        1,
                        format!(
                            "Error: `old` is not in {path}. Copy \
                             the text to replace from the file \
                             exactly. It begins:\n{shown}"
                        ),
                    );
                }
            }
            n => {
                return Outcome::plain(
                    1,
                    format!(
                        "Error: `old` must match exactly once, \
                         found {n}"
                    ),
                );
            }
        },
    };
    match result {
        Ok(msg) => Outcome::plain(0, msg),
        Err(e) => Outcome::plain(1, format!("Error: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use hamlet_home::Home;
    use hamlet_testing::Scratch;
    use serde_json::json;

    use super::*;

    /// A home in a folder of its own, for its workspace.
    fn setup(tag: &str) -> (Scratch, Home) {
        let dir = Scratch::new(&format!("tools-{tag}"));
        let home = Home::open(dir.to_path_buf(), None).unwrap();
        (dir, home)
    }

    #[test]
    fn edit_keeps_to_the_workspace() {
        let (_dir, t) = setup("edit");
        let ws = &t.workspace;
        let ok = |a: Value| edit(ws, None, &a);
        let absolute = ws.join("a/c.txt");
        assert_eq!(ok(json!({"path": absolute.to_str().unwrap(), "old": "", "new": "x"})).exit_code, 0);
        assert!(absolute.exists());
        assert_eq!(
            ok(json!({"path": "../x", "old": "", "new": "x"}))
                .exit_code,
            1
        );
        assert_eq!(
            ok(json!({"path": "/etc/passwd", "old": "", "new": "x"}))
                .exit_code,
            1
        );
    }

    #[test]
    fn edit_never_replaces_a_file_it_could_not_read() {
        let (_dir, t) = setup("edit-keep");
        let ws = &t.workspace;
        let not_text = [0xff, 0xfe];
        std::fs::write(ws.join("data.bin"), not_text).unwrap();
        let o = edit(
            ws,
            None,
            &json!({"path": "data.bin", "old": "", "new": "x"}),
        );
        assert_eq!(o.exit_code, 1, "{}", o.output);
        assert_eq!(
            std::fs::read(ws.join("data.bin")).unwrap(),
            not_text
        );
        let o = edit(
            ws,
            None,
            &json!({"path": "NOTES.md", "old": "", "new": "x"}),
        );
        assert!(o.output.contains("already exists"), "{}", o.output);
        assert_eq!(
            std::fs::read_to_string(ws.join("NOTES.md")).unwrap(),
            "# Notes\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn edit_makes_nothing_beyond_a_symlink() {
        let (_dir, t) = setup("edit-link");
        let ws = &t.workspace;
        let outside = Scratch::new("tools-edit-outside");
        std::os::unix::fs::symlink(&*outside, ws.join("out"))
            .unwrap();
        for path in ["out/x", "out/sub/x"] {
            let o = edit(
                ws,
                None,
                &json!({"path": path, "old": "", "new": "x"}),
            );
            assert!(o.output.contains("outside"), "{}", o.output);
        }
        assert_eq!(std::fs::read_dir(&*outside).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn with_a_project_edit_works_there_and_guards_git() {
        let (_dir, t) = setup("edit-project");
        let project = Scratch::new("tools-edit-folder");
        let ws = &t.workspace;
        std::fs::create_dir_all(project.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(project.join("sub")).unwrap();
        std::os::unix::fs::symlink(
            project.join(".git/hooks"),
            project.join("sub/h"),
        )
        .unwrap();
        let ok = |path: &str, old: &str| {
            edit(
                ws,
                Some(&project),
                &json!({"path": path, "old": old, "new": "x"}),
            )
        };
        assert_eq!(ok("src/a.rs", "").exit_code, 0);
        assert!(project.join("src/a.rs").exists());
        // The agent's own files stay in its house.
        let o = ok("NOTES.md", "# Notes");
        assert_eq!(o.exit_code, 0, "{}", o.output);
        assert_eq!(ok("skills/new/SKILL.md", "").exit_code, 0);
        assert!(!project.join("NOTES.md").exists());
        assert!(!project.join("skills").exists());
        assert!(ws.join("skills/new/SKILL.md").exists());
        let full = ws.join("own.txt");
        assert_eq!(ok(full.to_str().unwrap(), "").exit_code, 0);
        for path in [
            ".git/hooks/pre-commit",
            ".git/config",
            ".GIT/Hooks/pre-commit",
            "vendor/lib/.git/config",
            "sub/h/pre-commit",
        ] {
            let o = ok(path, "");
            assert!(o.output.contains("cannot be edited"), "{path}");
        }
        assert_eq!(
            std::fs::read_dir(project.join(".git/hooks"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(ok(".git/info/exclude", "").exit_code, 0);
        assert_eq!(ok("/etc/hamlet-never", "").exit_code, 1);
        assert_eq!(ok("../out.txt", "").exit_code, 1);
    }
}

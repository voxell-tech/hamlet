//! Hamlet's `find` (C16), which only reads and so never waits.

use serde_json::{Map, Value};

use super::walk::{
    FOUND, Failure, reached, report, shown, under, walk, wanted,
};
use super::{Arguments, Outcome};
use crate::sandbox::{Env, FileKind};

pub(super) const DESCRIPTION: &str =
    "List a folder, or find files by name under it. Without `name` \
     it lists what is directly in the folder. With `name` it gives \
     every file under the folder whose name matches, up to 100. \
     Never waits for approval: use it rather than ls or find.";

pub(super) fn run(
    env: &Env,
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new("find", arguments);
    let (path, name) = match (
        args.optional_string("path"),
        args.optional_string("name"),
    ) {
        (Ok(path), Ok(name)) => (path, name),
        (Err(invalid), _) | (_, Err(invalid)) => return invalid,
    };
    match find(env, path, name) {
        Ok(found) => Outcome::text(found),
        Err(failure) => Outcome::failed(failure.words()),
    }
}

/// A folder's entries, sorted with `/` after each folder, or the
/// files under it whose name `name` matches.
fn find(
    env: &Env,
    path: Option<&str>,
    name: Option<&str>,
) -> Result<String, Failure> {
    let (root, info) = reached(env, path)?;
    if info.kind != FileKind::Directory {
        return Err(Failure::NotAFolder(shown(env, &root)));
    }
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        let listed = env.list_dir(&root).map_err(|error| {
            Failure::Unreadable {
                path: shown(env, &root),
                reason: error.code,
            }
        })?;
        let mut entries = listed
            .into_iter()
            .map(|entry| match entry.kind {
                FileKind::Directory => format!("{}/", entry.name),
                _ => entry.name,
            })
            .collect::<Vec<_>>();
        entries.sort();
        return Ok(if entries.is_empty() {
            "(empty)".into()
        } else {
            entries.join("\n")
        });
    };
    let mut found = Vec::new();
    let capped = walk(env, &root, |at, _| {
        if wanted(name, &under(&root, at)) {
            found.push(shown(env, at));
        }
        found.len() <= FOUND
    });
    Ok(report(&found, capped, "no file by that name"))
}

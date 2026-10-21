//! Hamlet's `find` (C16), which only reads and so never waits.

use serde_json::{Map, Value};

use super::environment::Reach;
use super::walk::{
    FOUND, Failure, reached, report, shown, under, wanted,
};
use super::{Arguments, Outcome};
use crate::sandbox::FileKind;

pub(super) const DESCRIPTION: &str =
    "List a folder, or find files by name under it. Without `name` \
     it lists what is directly in the folder. With `name` it gives \
     every file under the folder whose name matches, up to 100. \
     Never waits for approval: use it rather than ls or find.";

pub(super) async fn run(
    env: &mut impl Reach,
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
    match find(env, path, name).await {
        Ok(found) => Outcome::text(found),
        Err(failure) => Outcome::failed(failure.words()),
    }
}

/// A folder's entries, sorted with `/` after each folder, or the
/// files under it whose name `name` matches.
async fn find(
    env: &mut impl Reach,
    path: Option<&str>,
    name: Option<&str>,
) -> Result<String, Failure> {
    let (root, info) = reached(env, path).await?;
    if info.kind != FileKind::Directory {
        return Err(Failure::NotAFolder(shown(env, &root)));
    }
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        let listed = env.list_dir(&root).await.map_err(|error| {
            Failure::Unreadable {
                path: shown(env, &root),
                reason: error.code.as_str(),
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
    let walked = env.walk(&root).await;
    let mut capped = walked.capped;
    for file in walked.files {
        if wanted(name, &under(&root, &file.path)) {
            found.push(shown(env, &file.path));
        }
        if found.len() > FOUND {
            capped = false;
            break;
        }
    }
    Ok(report(&found, capped, "no file by that name"))
}

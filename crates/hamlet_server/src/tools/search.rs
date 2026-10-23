//! Hamlet's `search` (C16), which only reads and so never waits.

use hamlet_workspace_wire::{Found, Walked};
use regex::Regex;
use serde_json::{Map, Value};

use super::environment::Reach;
use super::walk::{
    FOUND, Failure, is_wanted, reached, report, shown, under,
};
use super::{Arguments, Outcome, is_pi_space};
use crate::change::{Cut, clip, lines_of};
use crate::sandbox::FileKind;

pub(super) const DESCRIPTION: &str =
    "Search the contents of files under a folder for a regular \
     expression. Gives each match as path:line: text, up to 100. \
     Skips .git and build folders such as target and node_modules. \
     Never waits for approval: use it rather than grep or rg.";

/// The most characters of a line a search shows.
const WIDTH: usize = 300;

/// The largest file a search reads, in bytes.
const LARGEST: u64 = 1024 * 1024;

pub(super) async fn run(
    env: &mut impl Reach,
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new("search", arguments);
    let given = (
        args.string("pattern"),
        args.optional_string("path"),
        args.optional_string("name"),
        args.boolean("ignore_case"),
    );
    let (pattern, path, name, ignore_case) = match given {
        (Ok(pattern), Ok(path), Ok(name), Ok(ignore_case)) => {
            (pattern, path, name, ignore_case.unwrap_or(false))
        }
        (Err(invalid), ..)
        | (_, Err(invalid), ..)
        | (_, _, Err(invalid), _)
        | (.., Err(invalid)) => return invalid,
    };
    match search(env, pattern, path, name, ignore_case).await {
        Ok(found) => Outcome::text(found),
        Err(failure) => Outcome::failed(failure.words()),
    }
}

/// The lines matching a pattern in the text files under a folder. A
/// pattern runs in linear time, so none can stall the server; a file
/// over 1 MiB, one that is not UTF-8 and one holding NUL are passed
/// over.
async fn search(
    env: &mut impl Reach,
    pattern: &str,
    path: Option<&str>,
    name: Option<&str>,
    ignore_case: bool,
) -> Result<String, Failure> {
    if pattern.is_empty() {
        return Err(Failure::PatternEmpty);
    }
    let matches = compile(pattern, ignore_case)?;
    let (root, info) = reached(env, path).await?;
    let walked = if info.kind == FileKind::Directory {
        env.walk(&root).await
    } else {
        Walked {
            files: vec![Found {
                path: root.clone(),
                size: info.size,
            }],
            capped: false,
        }
    };
    let mut found = Vec::new();
    let mut look = async |at: &str, size: u64| {
        if let Some(name) = name.filter(|name| !name.is_empty())
            && !is_wanted(name, &under(&root, at))
        {
            return true;
        }
        if size > LARGEST {
            return true;
        }
        let Ok(text) = env.read_text(at).await else {
            return true;
        };
        if text.contains('\0') {
            return true;
        }
        for (index, line) in lines_of(&text).into_iter().enumerate() {
            if !matches(line) {
                continue;
            }
            found.push(format!(
                "{}:{}: {}",
                shown(env, at),
                index + 1,
                clip(
                    line.trim_end_matches(is_pi_space),
                    WIDTH,
                    Cut::Dotted
                )
            ));
            if found.len() > FOUND {
                return false;
            }
        }
        true
    };
    let mut capped = walked.capped;
    for file in walked.files {
        if !look(&file.path, file.size).await {
            capped = false;
            break;
        }
    }
    Ok(report(&found, capped, "no match"))
}

/// A test of a line against `pattern`. For `ignore_case` the pattern
/// and the line are put in lower case, but for the letter that
/// follows a backslash.
fn compile(
    pattern: &str,
    ignore_case: bool,
) -> Result<impl Fn(&str) -> bool, Failure> {
    let mut source = pattern.to_owned();
    if ignore_case {
        source.clear();
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match chars.peek() {
                Some(&next) if c == '\\' => {
                    source.push(c);
                    source.push(next);
                    chars.next();
                }
                _ => source.extend(c.to_lowercase()),
            }
        }
    }
    let regex = Regex::new(&source).map_err(|error| {
        let words = error.to_string();
        let reason = words
            .rsplit_once("error: ")
            .map_or(words.as_str(), |(_, reason)| reason)
            .trim()
            .to_owned();
        if reason.contains("look-around")
            || reason.contains("backreferences")
        {
            Failure::PatternNotLinear
        } else {
            Failure::PatternInvalid(reason)
        }
    })?;
    Ok(move |line: &str| {
        if ignore_case {
            regex.is_match(&line.to_lowercase())
        } else {
            regex.is_match(line)
        }
    })
}

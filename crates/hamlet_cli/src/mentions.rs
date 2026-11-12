//! The `@path` tokens of a typed line, which attach what they name
//! where they stand (C33).

use std::path::{Path, PathBuf};

/// A line with its `@path` tokens taken out.
#[derive(Debug, PartialEq)]
pub(crate) struct Taken {
    pub text: String,
    /// Each path taken, with the byte offset into `text` where its
    /// token stood, in the order they were written.
    pub paths: Vec<(u64, PathBuf)>,
}

/// The characters a sentence may put after a path.
const TRAILING: [char; 7] = ['.', ',', ';', ':', '!', '?', ')'];

/// Takes from `line` each token `@path` or `@"path with spaces"`
/// that starts the line or follows whitespace and whose path
/// `exists`. Any other token stays in the text. A token that ends a
/// word with a mark of punctuation names the path before the mark.
pub(crate) fn take(
    line: &str,
    exists: impl Fn(&Path) -> bool,
) -> Taken {
    let mut found = Vec::new();
    let mut after = true;
    let mut skip_to = 0;
    for (start, ch) in line.char_indices() {
        let follows_space = after;
        after = ch.is_whitespace();
        if ch != '@' || !follows_space || start < skip_to {
            continue;
        }
        let rest = &line[start + 1..];
        // The names the token may mean, each with where it ends.
        let names = match rest.strip_prefix('"') {
            Some(quoted) => {
                let Some(close) = quoted.find('"') else {
                    continue;
                };
                vec![(&quoted[..close], start + 3 + close)]
            }
            None => {
                let word = rest
                    .split(char::is_whitespace)
                    .next()
                    .unwrap_or_default();
                let bare = word.trim_end_matches(TRAILING);
                [word, bare]
                    .map(|name| (name, start + 1 + name.len()))
                    .to_vec()
            }
        };
        let Some((name, end)) =
            names.into_iter().find(|(name, _)| {
                !name.is_empty() && exists(Path::new(name))
            })
        else {
            continue;
        };
        found.push((start, end, PathBuf::from(name)));
        skip_to = end;
    }

    let mut text = String::new();
    let mut paths = Vec::new();
    let mut from = 0;
    for (start, end, path) in found {
        text.push_str(&line[from..start]);
        paths.push((text.len() as u64, path));
        from = end;
        // The space after a token that stood alone goes with it.
        if (text.is_empty() || text.ends_with(char::is_whitespace))
            && let Some(next) = line[from..]
                .chars()
                .next()
                .filter(|next| next.is_whitespace())
        {
            from += next.len_utf8();
        }
    }
    text.push_str(&line[from..]);
    Taken { text, paths }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn there(path: &Path) -> bool {
        ["a.png", "notes", "my dir/b.txt"]
            .contains(&path.to_str().unwrap())
    }

    /// The text left, and the paths taken as `offset path` lines.
    fn taken(line: &str) -> (String, String) {
        let taken = take(line, there);
        let paths = taken
            .paths
            .iter()
            .map(|(at, path)| format!("{at} {}", path.display()))
            .collect::<Vec<_>>();
        (taken.text, paths.join(", "))
    }

    #[test]
    fn a_token_is_taken_out_where_it_stood() {
        for (line, text, paths) in [
            (
                "look at @a.png and compare",
                "look at and compare",
                "8 a.png",
            ),
            ("@notes what is in it", "what is in it", "0 notes"),
            ("compare @a.png", "compare ", "8 a.png"),
        ] {
            assert_eq!(taken(line), (text.into(), paths.into()));
        }
    }

    #[test]
    fn two_tokens_keep_their_order() {
        for (line, text, paths) in [
            ("@a.png @notes", "", "0 a.png, 0 notes"),
            (
                "see @a.png, then @notes.",
                "see , then .",
                "4 a.png, 11 notes",
            ),
        ] {
            assert_eq!(taken(line), (text.into(), paths.into()));
        }
    }

    #[test]
    fn a_quoted_token_holds_spaces() {
        assert_eq!(
            taken("read @\"my dir/b.txt\" now"),
            ("read now".into(), "5 my dir/b.txt".into())
        );
    }

    #[test]
    fn what_names_no_path_stays_in_the_text() {
        for line in [
            "ask @someone about it",
            "mail a@b.com",
            "a lone @ here",
            "@\"unclosed a.png",
            "@\"nothing here\" at all",
        ] {
            assert_eq!(taken(line), (line.into(), String::new()));
        }
    }
}

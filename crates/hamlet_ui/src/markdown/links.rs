//! The links in a run of words: web addresses and what reads as a
//! path.

use std::ops::Range;

use super::{Span, Target};

/// The longest address or path taken for a link, in bytes.
const LONGEST: usize = 2048;

/// The marks a word may open with that are no part of its link.
const OPENS: [char; 6] = ['(', '[', '{', '<', '"', '\''];

/// The marks a sentence puts after a link.
const ENDS: [char; 9] =
    ['.', ',', ';', ':', '!', '?', '"', '\'', '>'];

/// `address` if a browser or a mail program may be given it.
pub(super) fn web(address: &str) -> Option<Target> {
    let lower = address.get(..8)?.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &address[8..]
    } else if lower.starts_with("http://") {
        &address[7..]
    } else if lower.starts_with("mailto:") {
        address[7..].split_once('@')?.1
    } else {
        return None;
    };
    let host = rest.chars().next()?;
    let clean =
        !address.chars().any(|c| c.is_whitespace() || c.is_control());
    (host.is_alphanumeric() && clean && address.len() <= LONGEST)
        .then(|| Target::Web(address.to_string()))
}

/// `word` with no `:line` or `:line:column` after it.
fn sans_place(word: &str) -> &str {
    let mut path = word;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, digits))
                if !head.is_empty()
                    && !digits.is_empty()
                    && digits.bytes().all(|b| b.is_ascii_digit()) =>
            {
                path = head;
            }
            _ => break,
        }
    }
    path
}

/// True when `path` reads as a path: folders with a name in them, or
/// one name with an ending.
fn is_path(path: &str) -> bool {
    if path.len() > LONGEST || !path.chars().any(char::is_alphabetic)
    {
        return false;
    }
    // The colon of a drive is the one colon a path has.
    let drive = path.as_bytes().get(1) == Some(&b':')
        && path.starts_with(|c: char| c.is_ascii_alphabetic())
        && matches!(path.as_bytes().get(2), Some(b'/' | b'\\'));
    let body = if drive { &path[2..] } else { path };
    let plain = |c: char| {
        c.is_alphanumeric()
            || matches!(
                c,
                '/' | '\\' | '.' | '_' | '-' | '+' | '@' | '~'
            )
    };
    if !body.chars().all(plain) || body.contains("//") {
        return false;
    }
    if body.contains(['/', '\\']) {
        return true;
    }
    match body.rsplit_once('.') {
        Some((stem, ending)) => {
            (stem.is_empty() || stem.len() > 1)
                && !stem.ends_with('.')
                && (1..=10).contains(&ending.len())
                && ending.chars().any(char::is_alphabetic)
        }
        None => false,
    }
}

/// The place `address` leads, as the address of a Markdown link. An
/// address of any scheme but the web's and mail's leads nowhere.
pub(super) fn address(address: &str) -> Option<Target> {
    if let Some(target) = web(address) {
        return Some(target);
    }
    let path = sans_place(address);
    let scheme = path.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '+' | '-' | '.')
            })
    });
    let clean = !path.chars().any(char::is_control);
    (!scheme && clean && !path.is_empty() && path.len() <= LONGEST)
        .then(|| Target::Path(path.to_string()))
}

/// The link in a word of running text, and the part of the word it
/// is.
fn word(word: &str) -> Option<(Range<usize>, Target)> {
    let start = word.len() - word.trim_start_matches(OPENS).len();
    let mut link = &word[start..];
    loop {
        let unopened = |close: char, open: char| {
            link.ends_with(close)
                && link.matches(close).count()
                    > link.matches(open).count()
        };
        if link.ends_with(ENDS)
            || unopened(')', '(')
            || unopened(']', '[')
            || unopened('}', '{')
        {
            link = &link[..link.len() - 1];
        } else {
            break;
        }
    }
    let target = web(link).or_else(|| {
        let path = sans_place(link);
        is_path(path).then(|| Target::Path(path.to_string()))
    })?;
    Some((start..start + link.len(), target))
}

/// The links in `text`, each with the part of the text it is.
fn find(text: &str) -> Vec<(Range<usize>, Target)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = text[at..]
        .find(|c: char| !c.is_whitespace())
        .map(|start| at + start)
    {
        let end = text[start..]
            .find(char::is_whitespace)
            .map_or(text.len(), |end| start + end);
        let one = &text[start..end];
        if one.contains(['/', '\\', '.'])
            && let Some((part, target)) = word(one)
        {
            found
                .push((start + part.start..start + part.end, target));
        }
        at = end;
    }
    found
}

/// `spans` with each web address and each path in their words set
/// apart as a span that links to it.
pub(super) fn linked(spans: Vec<Span>) -> Vec<Span> {
    let mut all = Vec::with_capacity(spans.len());
    for span in spans {
        let found = match span.link {
            Some(_) => Vec::new(),
            None => find(&span.text),
        };
        if found.is_empty() {
            all.push(span);
            continue;
        }
        let part = |range: Range<usize>, link| Span {
            text: span.text[range].to_string(),
            style: span.style,
            link,
        };
        let mut at = 0;
        for (range, target) in found {
            if range.start > at {
                all.push(part(at..range.start, None));
            }
            at = range.end;
            all.push(part(range, Some(target)));
        }
        if at < span.text.len() {
            all.push(part(at..span.text.len(), None));
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The links of `text`, each as its words and where it leads.
    fn links(text: &str) -> Vec<(&str, Target)> {
        find(text)
            .into_iter()
            .map(|(range, target)| (&text[range], target))
            .collect()
    }

    fn path(path: &str) -> Target {
        Target::Path(path.into())
    }

    #[test]
    fn a_link_ends_before_the_marks_of_its_sentence() {
        let url = "https://bevy.org/news_(0.19)";
        assert_eq!(
            links(&format!("See ({url}), or <http://a.b/c?d=1&e>.")),
            [
                (url, Target::Web(url.into())),
                (
                    "http://a.b/c?d=1&e",
                    Target::Web("http://a.b/c?d=1&e".into())
                ),
            ]
        );
        assert_eq!(
            links(
                "In src/main.rs:42:7, ./notes/plan.md and \
                 Cargo.toml."
            ),
            [
                ("src/main.rs:42:7", path("src/main.rs")),
                ("./notes/plan.md", path("./notes/plan.md")),
                ("Cargo.toml", path("Cargo.toml")),
            ]
        );
    }

    #[test]
    fn only_the_web_and_a_path_are_links() {
        assert_eq!(
            links(
                "file:///etc/passwd javascript:alert(1) x://y \
                 https:// 3.14 1/2 v0.19.1 i.e. and so on... a::b"
            ),
            []
        );
        assert_eq!(address("file:///etc/passwd"), None);
        assert_eq!(address("javascript:alert(1)"), None);
        assert_eq!(address("vscode://file/x"), None);
        assert_eq!(
            address("my notes/plan.md:3"),
            Some(path("my notes/plan.md"))
        );
        assert_eq!(
            address("mailto:chief@hamlet.gg"),
            Some(Target::Web("mailto:chief@hamlet.gg".into()))
        );
    }
}

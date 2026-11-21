//! The links in a run of words: web addresses, agents' addresses and
//! what reads as a path.

use std::ops::Range;

use super::{IsAddress, Span, Target};

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

/// The longest name taken after an address, in bytes.
const LONGEST_NAME: usize = 64;

/// The marks that make a word worth reading as a link.
const MARKS: [char; 5] = ['/', '\\', '.', '#', '@'];

fn agent(text: &str, is_address: IsAddress) -> Option<Target> {
    (text.len() <= LONGEST && is_address(text))
        .then(|| Target::Agent(text.to_string()))
}

/// Whether `target` is an address with no mark, as `hand`, which in
/// running words is most often a plain word.
fn is_plain_address(target: &Target) -> bool {
    matches!(target, Target::Agent(at) if !at.contains(MARKS))
}

/// The length of the ` (<name>)` a copied address carries, if `rest`
/// opens with one.
fn named(rest: &str) -> Option<usize> {
    let name = rest.strip_prefix(" (")?;
    let end = name.find(')')?;
    let after = name[end + 1..].chars().next();
    let name = &name[..end];
    let plain = !name.trim().is_empty()
        && name.len() <= LONGEST_NAME
        && !name.contains(['(', '\n'])
        && after
            .is_none_or(|c| c.is_whitespace() || ENDS.contains(&c));
    plain.then_some(end + 3)
}

/// `text` as an address alone, its copied name allowed.
fn whole_agent(text: &str, is_address: IsAddress) -> Option<Target> {
    let text = text.trim();
    agent(text, is_address).or_else(|| {
        let (address, _) = text.split_once(' ')?;
        let target = agent(address, is_address)?;
        (named(&text[address.len()..])
            == Some(text.len() - address.len()))
        .then_some(target)
    })
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
pub(super) fn address(
    address: &str,
    is_address: IsAddress,
) -> Option<Target> {
    if let Some(target) =
        web(address).or_else(|| agent(address, is_address))
    {
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
fn word(
    word: &str,
    is_address: IsAddress,
) -> Option<(Range<usize>, Target)> {
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
    let named_agent = agent(link, is_address)
        .filter(|target| !is_plain_address(target));
    let target = web(link).or(named_agent).or_else(|| {
        let path = sans_place(link);
        is_path(path).then(|| Target::Path(path.to_string()))
    })?;
    Some((start..start + link.len(), target))
}

/// When an address with no mark in running words is one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Plain {
    Never,
    /// With its copied name after it.
    Copied,
}

/// The links in `text`, each with the part of the text it is.
pub(super) fn find(
    text: &str,
    is_address: IsAddress,
) -> Vec<(Range<usize>, Target)> {
    scan(text, Plain::Never, is_address)
}

/// The addresses written in `text`, each with its part of the text,
/// copied name included.
pub fn agent_addresses(
    text: &str,
    is_address: IsAddress,
) -> Vec<(Range<usize>, String)> {
    scan(text, Plain::Copied, is_address)
        .into_iter()
        .filter_map(|(part, target)| match target {
            Target::Agent(address) => Some((part, address)),
            Target::Web(_)
            | Target::Path(_)
            | Target::AgentWords(_) => None,
        })
        .collect()
}

fn scan(
    text: &str,
    plain: Plain,
    is_address: IsAddress,
) -> Vec<(Range<usize>, Target)> {
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
        let mut next = end;
        let opens = one.len() - one.trim_start_matches(OPENS).len();
        let link = match one.contains(MARKS) {
            true => word(one, is_address),
            false if plain == Plain::Copied => {
                agent(&one[opens..], is_address)
                    .map(|target| (opens..one.len(), target))
            }
            false => None,
        };
        if let Some((part, target)) = link {
            let mut part = start + part.start..start + part.end;
            // A copied address carries its name: `agent#3 (Mira)`.
            let name = match matches!(target, Target::Agent(_))
                && part.end == end
            {
                true => named(&text[end..]),
                false => None,
            };
            if let Some(name) = name {
                part.end += name;
                next = part.end;
            }
            if name.is_some() || !is_plain_address(&target) {
                found.push((part, target));
            }
        }
        at = next;
    }
    found
}

/// `spans` with each web address and each path in their words set
/// apart as a span that links to it.
pub(super) fn linked(
    spans: Vec<Span>,
    is_address: IsAddress,
) -> Vec<Span> {
    let mut all = Vec::with_capacity(spans.len());
    for span in spans {
        let found = match (&span.link, span.style.code) {
            (Some(_), _) => Vec::new(),
            (None, true) => match whole_agent(&span.text, is_address)
            {
                Some(target) => vec![(0..span.text.len(), target)],
                None => find(&span.text, is_address),
            },
            (None, false) => find(&span.text, is_address),
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
    use crate::markdown::Style;

    fn is_address(word: &str) -> bool {
        matches!(
            word,
            "hand"
                | "agent#3"
                | "worker#12"
                | "lead@0b6f-9a"
                | "profile#4"
        )
    }

    /// The links of `text`, each as its words and where it leads.
    fn links(text: &str) -> Vec<(&str, Target)> {
        find(text, is_address)
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
    fn an_address_takes_the_name_copied_after_it_and_hand_is_one_only_in_code()
     {
        let agent = |address: &str| Target::Agent(address.into());
        assert_eq!(
            links(
                "Ask agent#3 (Mira Vale), then worker#12. On the \
                 other hand (left), see lead@0b6f-9a and profile#4 \
                 (x)y, not agent#x or agent#3, (Mira)."
            ),
            [
                ("agent#3 (Mira Vale)", agent("agent#3")),
                ("worker#12", agent("worker#12")),
                ("lead@0b6f-9a", agent("lead@0b6f-9a")),
                ("profile#4", agent("profile#4")),
                ("agent#3", agent("agent#3")),
            ]
        );
        let code = |text: &str| Span {
            text: text.into(),
            style: Style {
                code: true,
                ..Style::default()
            },
            link: None,
        };
        assert_eq!(
            linked(
                vec![code("hand"), code("agent#3 (Mira)")],
                is_address
            )
            .into_iter()
            .map(|span| span.link)
            .collect::<Vec<_>>(),
            [Some(agent("hand")), Some(agent("agent#3"))]
        );
        assert_eq!(address("hand", is_address), Some(agent("hand")));
        // In a field, `hand` is one as it is copied, with its name.
        let text = "On the other hand, ask hand (The Hand).";
        assert_eq!(
            agent_addresses(text, is_address)
                .into_iter()
                .map(|(range, address)| (&text[range], address))
                .collect::<Vec<_>>(),
            [("hand (The Hand)", "hand".to_string())]
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
        assert_eq!(address("file:///etc/passwd", is_address), None);
        assert_eq!(address("javascript:alert(1)", is_address), None);
        assert_eq!(address("vscode://file/x", is_address), None);
        assert_eq!(
            address("my notes/plan.md:3", is_address),
            Some(path("my notes/plan.md"))
        );
        assert_eq!(
            address("mailto:chief@hamlet.gg", is_address),
            Some(Target::Web("mailto:chief@hamlet.gg".into()))
        );
    }
}

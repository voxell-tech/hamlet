//! Credentials taken out of text and values before they are kept or
//! shown (C32).

use serde_json::Value;

/// The words a credential is replaced with.
pub const REDACTED: &str = "[REDACTED]";

/// Replaces every known secret in a stream of text with [`REDACTED`],
/// also one split across chunks. It holds back the end of what it was
/// given while that end could begin a secret.
pub struct Scrubber {
    /// The longest first, where one secret holds another.
    secrets: Vec<String>,
    held: String,
}

impl Scrubber {
    /// Ignores empty secrets.
    pub fn new(secrets: &[String]) -> Self {
        let mut unique = Vec::<String>::new();
        for secret in secrets {
            if !secret.is_empty() && !unique.contains(secret) {
                unique.push(secret.clone());
            }
        }
        unique.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        Self {
            secrets: unique,
            held: String::new(),
        }
    }

    /// The text that can go out now of what came so far and `chunk`.
    pub fn push(&mut self, chunk: &str) -> String {
        if self.secrets.is_empty() {
            return chunk.to_owned();
        }
        let text = self.replace(&format!("{}{chunk}", self.held));
        let cut = text.len() - self.beginning(&text);
        self.held = text[cut..].to_owned();
        text[..cut].to_owned()
    }

    /// The text it held back, let out once nothing more comes.
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.held)
    }

    /// `text` with each secret replaced, leftmost first and, at one
    /// place, the longest.
    fn replace(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        'scan: while let Some(c) = rest.chars().next() {
            for secret in &self.secrets {
                if let Some(after) =
                    rest.strip_prefix(secret.as_str())
                {
                    out.push_str(REDACTED);
                    rest = after;
                    continue 'scan;
                }
            }
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        out
    }

    /// The length in bytes of the longest end of `text` that begins a
    /// secret.
    fn beginning(&self, text: &str) -> usize {
        let mut longest = 0;
        for secret in &self.secrets {
            let starts = secret
                .char_indices()
                .map(|(at, _)| at)
                .skip(1)
                .filter(|&at| at > longest && at <= text.len());
            for length in starts.collect::<Vec<_>>().into_iter().rev()
            {
                if text.ends_with(&secret[..length]) {
                    longest = length;
                    break;
                }
            }
        }
        longest
    }
}

/// `value` with each secret replaced in its strings and its keys.
pub fn scrub_value(secrets: &[String], value: &mut Value) {
    let mut scrubber = Scrubber::new(secrets);
    scrub(&mut scrubber, value);
}

fn scrub(scrubber: &mut Scrubber, value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = scrubber.push(text) + &scrubber.flush()
        }
        Value::Array(items) => {
            for item in items {
                scrub(scrubber, item);
            }
        }
        Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut item) in entries {
                let key = scrubber.push(&key) + &scrubber.flush();
                scrub(scrubber, &mut item);
                map.insert(key, item);
            }
        }
        _ => {}
    }
}

/// `text` with each of `secrets` replaced.
pub(crate) fn scrub_text(secrets: &[String], text: &str) -> String {
    let mut scrubber = Scrubber::new(secrets);
    scrubber.push(text) + &scrubber.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn through(secrets: &[&str], chunks: &[&str]) -> Vec<String> {
        let secrets =
            secrets.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut scrubber = Scrubber::new(&secrets);
        let mut out = chunks
            .iter()
            .map(|chunk| scrubber.push(chunk))
            .collect::<Vec<_>>();
        out.push(scrubber.flush());
        out
    }

    #[test]
    fn replaces_every_secret_also_across_chunks() {
        assert_eq!(
            through(
                &["sk-abc", "tok-123"],
                &["key sk-abc and tok-123"]
            ),
            ["key [REDACTED] and [REDACTED]", ""]
        );
        assert_eq!(
            through(&["sk-abc"], &["key sk-", "abc end"]),
            ["key ", "[REDACTED] end", ""]
        );
        assert_eq!(
            through(&["sk-abc"], &["s", "k", "-", "a", "b", "c!"])
                .concat(),
            "[REDACTED]!"
        );
        assert_eq!(
            through(&["abc", "abcdef"], &["xabcdefx"]).concat(),
            "x[REDACTED]x"
        );
    }

    #[test]
    fn holds_back_only_what_could_begin_a_secret() {
        let secrets = ["abcdef".to_owned(), "xyz".to_owned()];
        let mut scrubber = Scrubber::new(&secrets);
        assert_eq!(scrubber.push("0123abcde"), "0123");
        assert_eq!(scrubber.flush(), "abcde");
        assert_eq!(
            through(&["abcdef", "xyz"], &["text xy"]),
            ["text ", "xy"]
        );
    }
}

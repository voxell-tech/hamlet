//! What a chief is shown of a change: a text cut to its start, also
//! one read in parts, as a large file a write replaces is.

use hamlet_sandbox::scrub::Scrubber;

/// The characters of a text a chief is shown.
const SHOWN: usize = 2000;

// Copied from the server's `oversight/gate.rs` `cut`.
/// `text` cut to its first 2000 characters, saying how many more
/// there were.
pub(super) fn cut(text: &str) -> String {
    let mut head = Head::default();
    head.push(text);
    head.shown()
}

/// The first [`SHOWN`] characters of a text that arrives in parts,
/// and a count of the rest.
#[derive(Default)]
struct Head {
    text: String,
    kept: usize,
    more: usize,
}

impl Head {
    fn push(&mut self, part: &str) {
        let mut chars = part.chars();
        for c in chars.by_ref().take(SHOWN - self.kept) {
            self.text.push(c);
            self.kept += 1;
        }
        self.more += chars.count();
    }

    fn shown(self) -> String {
        if self.more == 0 {
            self.text
        } else {
            format!("{}\n[{} more characters]", self.text, self.more)
        }
    }
}

/// The text of a file read in parts, as [`cut`] shows it after
/// [`String::from_utf8_lossy`] and the secrets taken out, without
/// holding more of it than its start.
pub(super) struct Replaced {
    /// The start of a character the last part cut in two.
    pending: Vec<u8>,
    scrubber: Scrubber,
    head: Head,
}

impl Replaced {
    pub(super) fn new(secrets: &[String]) -> Self {
        Self {
            pending: Vec::new(),
            scrubber: Scrubber::new(secrets),
            head: Head::default(),
        }
    }

    pub(super) fn push(&mut self, part: &[u8]) {
        self.pending.extend_from_slice(part);
        let bytes = std::mem::take(&mut self.pending);
        let mut text = String::new();
        let mut at = 0;
        loop {
            match std::str::from_utf8(&bytes[at..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let end = at + error.valid_up_to();
                    text.push_str(&String::from_utf8_lossy(
                        &bytes[at..end],
                    ));
                    let Some(invalid) = error.error_len() else {
                        self.pending = bytes[end..].to_vec();
                        break;
                    };
                    text.push(char::REPLACEMENT_CHARACTER);
                    at = end + invalid;
                }
            }
        }
        self.head.push(&self.scrubber.push(&text));
    }

    pub(super) fn shown(mut self) -> String {
        if !self.pending.is_empty() {
            self.head.push(&self.scrubber.push("\u{fffd}"));
        }
        self.head.push(&self.scrubber.flush());
        self.head.shown()
    }
}

#[cfg(test)]
mod tests {
    use hamlet_sandbox::scrub::scrub_text;

    use super::*;

    /// A text read in parts, cut anywhere, a character or a secret
    /// among them, shows as the whole text does.
    #[test]
    fn a_text_in_parts_shows_as_the_whole_does() {
        let secrets = vec!["s3cr3t".to_owned()];
        let mut bytes = "é".repeat(1990).into_bytes();
        bytes.extend_from_slice(b"ab s3cr3t \xff\xe2\x82 tail ");
        bytes.extend_from_slice("€".repeat(30).as_bytes());
        bytes.extend_from_slice(b"\xe2\x82");
        let whole = cut(&scrub_text(
            &secrets,
            &String::from_utf8_lossy(&bytes),
        ));
        for size in [1, 2, 3, 5, 7, 4096] {
            let mut replaced = Replaced::new(&secrets);
            for part in bytes.chunks(size) {
                replaced.push(part);
            }
            assert_eq!(replaced.shown(), whole, "parts of {size}");
        }
        assert!(whole.ends_with("[43 more characters]"), "{whole}");
    }

    #[test]
    fn a_short_text_is_shown_whole() {
        assert_eq!(cut("short"), "short");
        assert_eq!(cut(&"x".repeat(SHOWN)), "x".repeat(SHOWN));
    }
}

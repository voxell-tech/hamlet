//! The images and files a chief's line carries besides its words,
//! each with the caption that says where it is saved, set in the
//! words where the attachment sits (C33).

use hamlet_protocol::client::{AttachedFile, AttachedImage};

/// The words that tell a model where one image, file or folder is
/// saved.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Caption {
    /// The byte offset into the line's words where the attachment
    /// sits; without one the caption goes after them.
    pub at: Option<u64>,
    pub words: String,
}

/// The images and files of a line a chief sent, in the order the
/// model is given them: by position, those without one last.
#[derive(Clone, Debug, Default)]
pub(crate) struct Attached {
    pub images: Vec<AttachedImage>,
    pub files: Vec<AttachedFile>,
    pub captions: Vec<Caption>,
}

/// A chief's line with its attachments in place.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Line {
    /// The words as a transcript shows them.
    pub text: String,
    /// The words with each caption where its attachment sits, which
    /// the model reads.
    pub content: String,
    /// The images, at their places in `text`.
    pub images: Vec<AttachedImage>,
    /// The files, at their places in `text`.
    pub files: Vec<AttachedFile>,
}

impl Line {
    /// A line of words alone.
    pub fn words(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            content: text.to_owned(),
            ..Self::default()
        }
    }
}

/// Whether `at` is a place in `text` an attachment may sit at.
pub(crate) fn is_place(text: &str, at: u64) -> bool {
    usize::try_from(at)
        .is_ok_and(|at| at <= text.len() && text.is_char_boundary(at))
}

impl Attached {
    /// The line of `text` with the attachments in place. `scrub` is
    /// applied to the words, a piece at a time so that every place
    /// moves with the words before it.
    pub fn place(
        self,
        text: &str,
        scrub: impl Fn(&str) -> String,
    ) -> Line {
        let mut cuts = self
            .captions
            .iter()
            .filter_map(|caption| caption.at)
            .chain(self.images.iter().filter_map(|image| image.at))
            .chain(self.files.iter().filter_map(|file| file.at))
            .map(|at| {
                usize::try_from(at).expect("a place was checked")
            })
            .collect::<Vec<_>>();
        cuts.sort_unstable();
        cuts.dedup();
        let mut pieces = Vec::with_capacity(cuts.len() + 1);
        let mut from = 0;
        for cut in &cuts {
            pieces.push(scrub(&text[from..*cut]));
            from = *cut;
        }
        pieces.push(scrub(&text[from..]));
        let mut moved = Vec::with_capacity(cuts.len());
        let mut length = 0;
        for piece in &pieces[..cuts.len()] {
            length += piece.len() as u64;
            moved.push(length);
        }
        let moved_to = |at: Option<u64>| {
            at.and_then(|at| {
                let cut = usize::try_from(at).ok()?;
                Some(moved[cuts.binary_search(&cut).ok()?])
            })
        };

        let mut content = String::new();
        for (index, piece) in pieces.iter().enumerate() {
            content.push_str(piece);
            let Some(cut) = cuts.get(index) else { break };
            let words = self
                .captions
                .iter()
                .filter(|caption| {
                    caption.at.and_then(|at| usize::try_from(at).ok())
                        == Some(*cut)
                })
                .map(|caption| caption.words.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if words.is_empty() {
                continue;
            }
            if !content.is_empty()
                && !content.ends_with(char::is_whitespace)
            {
                content.push(' ');
            }
            content.push_str(&words);
            if pieces[index + 1]
                .chars()
                .next()
                .is_some_and(|next| !next.is_whitespace())
            {
                content.push(' ');
            }
        }
        let last = self
            .captions
            .iter()
            .filter(|caption| caption.at.is_none())
            .map(|caption| caption.words.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let content = match (content.is_empty(), last.is_empty()) {
            (_, true) => content,
            (true, false) => last,
            (false, false) => format!("{content}\n\n{last}"),
        };

        Line {
            text: pieces.concat(),
            content,
            images: self
                .images
                .into_iter()
                .map(|image| AttachedImage {
                    at: moved_to(image.at),
                    ..image
                })
                .collect(),
            files: self
                .files
                .into_iter()
                .map(|file| AttachedFile {
                    at: moved_to(file.at),
                    ..file
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caption(at: Option<u64>, words: &str) -> Caption {
        Caption {
            at,
            words: words.into(),
        }
    }

    #[test]
    fn captions_sit_in_the_words_where_their_attachments_do() {
        let attached = Attached {
            captions: vec![
                caption(Some(5), "[Image 1]"),
                caption(Some(5), "[File 1]"),
                caption(Some(0), "[Image 2]"),
                caption(None, "[File 2]"),
            ],
            ..Attached::default()
        };
        let line = attached.place("look at it", str::to_owned);
        assert_eq!(
            line.content,
            "[Image 2] look [Image 1] [File 1] at it\n\n[File 2]"
        );
        assert_eq!(line.text, "look at it");
    }

    #[test]
    fn a_place_moves_with_the_words_scrubbed_before_it() {
        let attached = Attached {
            captions: vec![caption(Some(7), "[Image 1]")],
            images: vec![AttachedImage {
                at: Some(7),
                id: "0".repeat(64).parse().unwrap(),
            }],
            ..Attached::default()
        };
        let line = attached.place("secret is here", |words| {
            words.replace("secret", "x")
        });
        assert_eq!(line.text, "x is here");
        assert_eq!(line.images[0].at, Some(2));
    }
}

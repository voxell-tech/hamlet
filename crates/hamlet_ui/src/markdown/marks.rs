//! The marks that draw Markdown as it is typed in a field: each part
//! in the look a sent line gives it, with the signs that only say
//! how it looks hidden until the caret comes to them. An inline
//! part's signs show while the caret is anywhere in it, and a line's
//! `#` or `>` signs while the caret is at them.

use std::ops::Range;

use bevy::asset::Handle;
use bevy::color::Color;
use bevy::text::{Font, FontSource, FontStyle, FontWeight};
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{LineColor, Look, Mark, MarkKind};
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag,
};

use super::links::find;
use super::no_address;
use crate::theme::HamletTheme;

/// What typed Markdown is drawn in, from the theme.
#[derive(Clone)]
pub struct Inks {
    body: f32,
    small: f32,
    reference: Color,
    dim: Color,
    bar: Color,
    code_fill: Color,
    block_fill: Color,
    mono: Option<Handle<Font>>,
}

impl Inks {
    /// The theme's inks, with code in `mono` when there is one.
    pub fn of(
        theme: &HamletTheme,
        mono: Option<Handle<Font>>,
    ) -> Self {
        Self {
            body: theme.text.body,
            small: theme.text.small,
            reference: theme.tone(Tone::Reference),
            dim: theme.tone(Tone::Dim),
            bar: theme.palette.mark,
            code_fill: theme.palette.status.info.fill,
            block_fill: theme.palette.code,
            mono,
        }
    }

    fn code(&self, size: f32) -> Look {
        Look {
            font: self.mono.clone().map(FontSource::from),
            size: Some(size),
            ..Look::default()
        }
    }
}

/// How far left of the text a quote's bar is, and how wide.
const BAR_X: f32 = -7.0;
const BAR_WIDE: f32 = 3.0;

/// An open part of the text, with where its words start and end.
struct Open {
    tag: Tag<'static>,
    range: Range<usize>,
    inner: Option<Range<usize>>,
}

/// The marks that draw `text` as a sent line is drawn.
pub fn marks(text: &str, inks: &Inks) -> Vec<Mark> {
    let options = Options::ENABLE_STRIKETHROUGH;
    let mut marks = Vec::new();
    let mut open = Vec::<Open>::new();
    let look = |range: Range<usize>, look: Look| Mark {
        range,
        kind: MarkKind::Look(look),
    };
    for (event, range) in
        Parser::new_ext(text, options).into_offset_iter()
    {
        // Each open part's words run from the first thing in it to
        // the last; a part in it counts once it closes.
        let child = !matches!(event, Event::Start(_) | Event::End(_));
        if child && let Some(parent) = open.last_mut() {
            let inner = parent.inner.get_or_insert(range.clone());
            inner.end = inner.end.max(range.end);
        }
        match event {
            Event::Start(tag) => {
                open.push(Open {
                    tag: tag.into_static(),
                    range,
                    inner: None,
                });
            }
            Event::End(_) => {
                let Some(Open { tag, range, inner }) = open.pop()
                else {
                    continue;
                };
                if let Some(parent) = open.last_mut() {
                    let words =
                        parent.inner.get_or_insert(range.clone());
                    words.start = words.start.min(range.start);
                    words.end = words.end.max(range.end);
                }
                closed(text, &tag, range, inner, inks, &mut marks);
            }
            Event::Code(_) => {
                let ticks = text[range.clone()]
                    .bytes()
                    .take_while(|b| *b == b'`')
                    .count();
                if range.len() > 2 * ticks {
                    let words =
                        range.start + ticks..range.end - ticks;
                    marks.push(hidden(
                        range.start..words.start,
                        &range,
                    ));
                    marks.push(look(
                        words.clone(),
                        Look {
                            color: Some(inks.reference),
                            background: Some(inks.code_fill),
                            ..inks.code(inks.body - 1.0)
                        },
                    ));
                    marks.push(hidden(words.end..range.end, &range));
                }
            }
            Event::Text(_) => {
                let in_link = open.iter().any(|part| {
                    matches!(
                        part.tag,
                        Tag::Link { .. } | Tag::CodeBlock(_)
                    )
                });
                if !in_link {
                    for (found, _) in
                        find(&text[range.clone()], no_address)
                    {
                        let at = range.start;
                        marks.push(look(
                            at + found.start..at + found.end,
                            linked(inks),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    marks
}

/// `range` hidden while the caret is not in or at `shown_in`.
fn hidden(range: Range<usize>, shown_in: &Range<usize>) -> Mark {
    Mark {
        range,
        kind: MarkKind::Hidden {
            shown_in: shown_in.clone(),
        },
    }
}

/// A line's `signs`, hidden while the caret is not at them.
fn line_signs(signs: Range<usize>) -> Mark {
    hidden(signs.clone(), &signs)
}

fn linked(inks: &Inks) -> Look {
    Look {
        color: Some(inks.reference),
        underline: Some(LineColor::Ink),
        ..Look::default()
    }
}

/// The marks of the part `tag` over `range`, whose words are
/// `inner`, once it closes.
fn closed(
    text: &str,
    tag: &Tag,
    range: Range<usize>,
    inner: Option<Range<usize>>,
    inks: &Inks,
    marks: &mut Vec<Mark>,
) {
    let look = |range: Range<usize>, look: Look| Mark {
        range,
        kind: MarkKind::Look(look),
    };
    // The signs around the words, hidden, when there are words.
    let mut around = |words: &Range<usize>| {
        marks.push(hidden(range.start..words.start, &range));
        marks.push(hidden(words.end..range.end, &range));
    };
    match tag {
        Tag::Strong | Tag::Emphasis | Tag::Strikethrough => {
            let Some(words) = inner else { return };
            around(&words);
            marks.push(look(
                words,
                match tag {
                    Tag::Strong => Look {
                        weight: Some(FontWeight::BOLD),
                        ..Look::default()
                    },
                    Tag::Emphasis => Look {
                        style: Some(FontStyle::Italic),
                        ..Look::default()
                    },
                    _ => Look {
                        strikethrough: Some(LineColor::Ink),
                        ..Look::default()
                    },
                },
            ));
        }
        Tag::Link { .. } => {
            let Some(words) = inner else { return };
            around(&words);
            marks.push(look(words, linked(inks)));
        }
        Tag::Heading { level, .. } => {
            let Some(words) = inner else { return };
            // Only a heading opened with `#`, whose signs are before
            // its words on its line.
            if !text[range.clone()].starts_with('#') {
                return;
            }
            let size = match level {
                HeadingLevel::H1 => 20.0,
                HeadingLevel::H2 => 17.0,
                _ => 15.0,
            };
            marks.push(line_signs(range.start..words.start));
            let closing =
                words.end..range.end.min(line_end(text, words.end));
            marks.push(line_signs(closing));
            marks.push(look(
                words,
                Look {
                    size: Some(size),
                    weight: Some(FontWeight::BOLD),
                    ..Look::default()
                },
            ));
        }
        Tag::BlockQuote(_) => {
            marks.push(Mark {
                range: range.clone(),
                kind: MarkKind::Bar {
                    color: inks.bar,
                    x: BAR_X,
                    width: BAR_WIDE,
                },
            });
            marks.push(look(
                range.clone(),
                Look {
                    color: Some(inks.dim),
                    ..Look::default()
                },
            ));
            for line in lines(text, range) {
                let signs = quote_signs(&text[line.clone()]);
                // A line of signs alone keeps them, or it would have
                // no height.
                if signs > 0
                    && signs < text[line.clone()].trim_end().len()
                {
                    marks.push(line_signs(
                        line.start..line.start + signs,
                    ));
                }
            }
        }
        Tag::CodeBlock(kind) => {
            marks.push(Mark {
                range: range.clone(),
                kind: MarkKind::Fill(inks.block_fill),
            });
            marks.push(look(range.clone(), inks.code(inks.small)));
            if matches!(kind, CodeBlockKind::Fenced(_)) {
                let first = range.start..line_end(text, range.start);
                marks.push(look(
                    first,
                    Look {
                        color: Some(inks.dim),
                        ..Look::default()
                    },
                ));
                let last = lines(text, range).pop();
                if let Some(last) = last
                    && text[last.clone()]
                        .trim_start()
                        .starts_with(['`', '~'])
                {
                    marks.push(look(
                        last,
                        Look {
                            color: Some(inks.dim),
                            ..Look::default()
                        },
                    ));
                }
            }
        }
        _ => {}
    }
}

/// The byte where the line holding `at` ends, before its break.
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |end| at + end)
}

/// The lines of `range`, each without its break.
fn lines(text: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut at = range.start;
    while at < range.end {
        let end = line_end(text, at).min(range.end);
        lines.push(at..end);
        at = end + 1;
    }
    lines
}

/// The bytes of the `>` signs, with the space after each, that open
/// a quoted `line`.
fn quote_signs(line: &str) -> usize {
    let mut at = 0;
    let bytes = line.as_bytes();
    loop {
        let start = at;
        while bytes.get(at) == Some(&b' ') && at - start < 3 {
            at += 1;
        }
        if bytes.get(at) != Some(&b'>') {
            return start;
        }
        at += 1;
        if bytes.get(at) == Some(&b' ') {
            at += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inks() -> Inks {
        Inks {
            body: 13.0,
            small: 12.0,
            reference: Color::WHITE,
            dim: Color::WHITE,
            bar: Color::WHITE,
            code_fill: Color::WHITE,
            block_fill: Color::WHITE,
            mono: None,
        }
    }

    /// The text `text` shows away from the caret, with what `marks`
    /// hides taken out.
    fn shown(text: &str) -> String {
        let hidden = marks(text, &inks())
            .into_iter()
            .filter(|mark| {
                matches!(mark.kind, MarkKind::Hidden { .. })
            })
            .map(|mark| mark.range)
            .collect::<Vec<_>>();
        text.char_indices()
            .filter(|(at, _)| {
                !hidden.iter().any(|range| range.contains(at))
            })
            .map(|(_, c)| c)
            .collect()
    }

    #[test]
    fn the_signs_of_closed_parts_are_hidden_and_open_ones_shown() {
        assert_eq!(shown("a **b** *c* ~~d~~ `e`"), "a b c d e");
        assert_eq!(shown("a **b"), "a **b");
        assert_eq!(shown("[site](https://example.com) x"), "site x");
        assert_eq!(shown("# Title\nwords"), "Title\nwords");
        assert_eq!(shown("#"), "#");
    }

    #[test]
    fn a_quote_hides_its_signs_but_on_a_line_of_signs_alone() {
        assert_eq!(shown("> one\n>\n> two"), "one\n>\ntwo");
        assert_eq!(shown("> > deep"), "deep");
    }

    #[test]
    fn a_code_block_keeps_its_fences() {
        assert_eq!(shown("```\n**a**\n```"), "```\n**a**\n```");
    }
}

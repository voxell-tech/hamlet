//! Markdown read into a flat list of blocks.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag,
    TagEnd,
};

use super::links::{address, linked, web};
use super::{Align, Block, Kind, Marker, Row, Span, Style, Target};

#[derive(Default)]
struct Reader {
    blocks: Vec<Block>,
    quote: usize,
    /// The next number of each open list, `None` for bullets.
    lists: Vec<Option<u64>>,
    /// The mark of the open list item, until a block takes it.
    marker: Option<Marker>,
    spans: Vec<Span>,
    bold: u32,
    italic: u32,
    strike: u32,
    /// The place each open link leads, the innermost last.
    links: Vec<Option<Target>>,
    /// Each open image, the innermost last: true for the one that
    /// is a block of its own.
    images: Vec<bool>,
    /// The words and the place of the open picture.
    picture: Option<(String, Target)>,
    /// Set when the last block is a picture cut out of the line the
    /// next words are on.
    cut: bool,
    heading: Option<u8>,
    code: Option<(String, String)>,
    /// The open table: its columns and its rows so far.
    table: Option<(Vec<Align>, Vec<Row>)>,
}

impl Reader {
    fn emit(&mut self, kind: Kind) {
        let cap = |n: usize| n.min(u8::MAX as usize) as u8;
        self.blocks.push(Block {
            quote: cap(self.quote),
            indent: cap(self.lists.len()),
            marker: self.marker.take(),
            kind,
        });
    }

    /// Ends the run of inline text, as a heading or a paragraph.
    fn flush(&mut self) {
        if self.table.is_some() {
            return;
        }
        let mut spans = std::mem::take(&mut self.spans);
        if std::mem::take(&mut self.cut)
            && let Some(first) = spans.first_mut()
        {
            first.text = first.text.trim_start().to_string();
        }
        spans.retain(|span| !span.text.is_empty());
        let spans = linked(spans);
        let heading = self.heading.take();
        if spans.is_empty() {
            return;
        }
        self.emit(match heading {
            Some(level) => Kind::Heading(level, spans),
            None => Kind::Paragraph(spans),
        });
    }

    /// Ends the open picture as a block of its own, between the
    /// words before it and those after.
    fn depict(&mut self) {
        let Some((caption, target)) = self.picture.take() else {
            return;
        };
        let heading = self.heading;
        if self.spans.iter().all(|span| span.text.trim().is_empty()) {
            self.spans.clear();
        }
        self.flush();
        self.heading = heading;
        let caption =
            caption.split_whitespace().collect::<Vec<_>>().join(" ");
        self.emit(Kind::Picture { caption, target });
        self.cut = true;
    }

    fn text(&mut self, text: &str, code: bool) {
        if let Some((_, body)) = &mut self.code {
            body.push_str(text);
            return;
        }
        if let Some((caption, _)) = &mut self.picture {
            caption.push_str(text);
            return;
        }
        let style = Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
        };
        let link = self.links.last().cloned().flatten();
        match self.spans.last_mut() {
            Some(last)
                if last.style == style && last.link == link =>
            {
                last.text.push_str(text);
            }
            _ => self.spans.push(Span {
                text: text.to_string(),
                style,
                link,
            }),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link {
                link_type: LinkType::Email,
                dest_url,
                ..
            } => self.links.push(web(&format!("mailto:{dest_url}"))),
            Tag::Link { dest_url, .. } => {
                self.links.push(address(&dest_url));
            }
            Tag::Image { dest_url, .. } => {
                let target = address(&dest_url);
                // A cell of a table holds words alone.
                let alone =
                    self.table.is_none() && self.picture.is_none();
                self.images.push(alone && target.is_some());
                match target {
                    Some(target) if alone => {
                        self.picture = Some((String::new(), target));
                    }
                    target => self.links.push(target),
                }
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some((_, rows)) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.spans.clear(),
            block => {
                self.flush();
                match block {
                    Tag::Heading { level, .. } => {
                        self.heading = Some(level as u8);
                    }
                    Tag::BlockQuote(_) => self.quote += 1,
                    Tag::CodeBlock(kind) => {
                        let lang = match kind {
                            CodeBlockKind::Fenced(info) => info
                                .split_whitespace()
                                .next()
                                .unwrap_or_default()
                                .to_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        self.code = Some((lang, String::new()));
                    }
                    Tag::List(first) => self.lists.push(first),
                    Tag::Item => {
                        self.marker =
                            Some(match self.lists.last_mut() {
                                Some(Some(next)) => {
                                    *next += 1;
                                    Marker::Number(*next - 1)
                                }
                                _ => Marker::Bullet,
                            });
                    }
                    Tag::Table(align) => {
                        let align = align
                            .into_iter()
                            .map(|a| match a {
                                Alignment::Center => Align::Center,
                                Alignment::Right => Align::End,
                                _ => Align::Start,
                            })
                            .collect::<Vec<_>>();
                        self.table = Some((align, Vec::new()));
                    }
                    _ => {}
                }
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        let less = |n: &mut u32| *n = n.saturating_sub(1);
        match tag {
            TagEnd::Emphasis => less(&mut self.italic),
            TagEnd::Strong => less(&mut self.bold),
            TagEnd::Strikethrough => less(&mut self.strike),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => match self.images.pop() {
                Some(true) => self.depict(),
                _ => {
                    self.links.pop();
                }
            },
            TagEnd::TableHead | TagEnd::TableRow => {}
            TagEnd::TableCell => {
                let cell = linked(std::mem::take(&mut self.spans));
                if let Some((_, rows)) = &mut self.table
                    && let Some(row) = rows.last_mut()
                {
                    row.push(cell);
                }
            }
            TagEnd::Table => {
                if let Some((align, rows)) = self.table.take() {
                    self.emit(Kind::Table { align, rows });
                }
            }
            TagEnd::CodeBlock => {
                if let Some((lang, mut text)) = self.code.take() {
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    self.emit(Kind::Code { lang, text });
                }
            }
            block => {
                self.flush();
                match block {
                    TagEnd::BlockQuote(_) => {
                        self.quote = self.quote.saturating_sub(1);
                    }
                    TagEnd::List(_) => {
                        self.lists.pop();
                    }
                    // An item with nothing in it still shows its
                    // mark.
                    TagEnd::Item if self.marker.is_some() => {
                        self.emit(Kind::Paragraph(Vec::new()));
                    }
                    _ => {}
                }
            }
        }
    }

    fn read(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text)
            | Event::Html(text)
            | Event::InlineHtml(text) => self.text(&text, false),
            Event::Code(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => self.text(&text, true),
            Event::SoftBreak => self.text(" ", false),
            Event::HardBreak => self.text("\n", false),
            Event::Rule => {
                self.flush();
                self.emit(Kind::Rule);
            }
            Event::TaskListMarker(done) => {
                if self.marker.is_some() {
                    self.marker = Some(Marker::Task(done));
                }
            }
            Event::FootnoteReference(_) => {}
        }
    }
}

/// The spans of words set as they were typed, with the links in
/// them.
pub(super) fn plain(text: &str) -> Vec<Span> {
    linked(vec![Span {
        text: text.to_string(),
        style: Style::default(),
        link: None,
    }])
}

/// The blocks of a Markdown document, which may stop anywhere: it
/// is read as far as it goes. Text that makes no block is one
/// paragraph as written.
pub fn parse(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut reader = Reader::default();
    for event in Parser::new_ext(text, options) {
        reader.read(event);
    }
    reader.flush();
    if reader.blocks.is_empty() && !text.trim().is_empty() {
        reader.text(text.trim(), false);
        reader.flush();
    }
    reader.blocks
}

/// The marker and the length of the fence `line` opens or closes,
/// and what follows it.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim_start();
    let mark =
        line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let run = line.chars().take_while(|c| *c == mark).count();
    (run >= 3).then(|| (mark, run, &line[run..]))
}

/// True if `line` starts a list item, or could become one after
/// another character is typed.
fn item(line: &str) -> bool {
    let line = line.trim_start();
    let digits =
        line.chars().take_while(char::is_ascii_digit).count();
    let rest = if digits > 0 {
        match line[digits..].trim_end().strip_prefix(['.', ')']) {
            Some(rest) => rest,
            None => return line[digits..].trim().is_empty(),
        }
    } else {
        match line.strip_prefix(['-', '*', '+']) {
            Some(rest) => rest,
            None => return false,
        }
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

/// Where the last block that later words cannot change ends in
/// `text`, or 0 if no block is known to be done. `None` if a
/// definition in `text` can change the blocks above it.
fn settled(text: &str) -> Option<usize> {
    // The marker and the length of the fence that is open.
    let mut open = None::<(char, usize)>;
    let mut blank = false;
    let mut end = 0;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        // A definition further down can turn words above it into a
        // link.
        if open.is_none()
            && trimmed.starts_with('[')
            && trimmed.contains("]:")
        {
            return None;
        }
        if let Some((mark, run, rest)) = fence(line) {
            match open {
                None if mark == '~' || !rest.contains('`') => {
                    open = Some((mark, run));
                }
                Some((was, len))
                    if was == mark
                        && run >= len
                        && rest.trim().is_empty() =>
                {
                    open = None;
                }
                _ => {}
            }
        }
        if blank
            && open.is_none()
            && !trimmed.is_empty()
            && !line.starts_with(char::is_whitespace)
            // An item goes on the list above it.
            && !item(line)
        {
            end = at;
        }
        blank = trimmed.is_empty() && open.is_none();
        at += line.len();
    }
    Some(end)
}

pub(super) fn hashed(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The blocks of a document that grows at its end. The blocks
/// before the last are read once and shared from then on.
#[derive(Default)]
pub(crate) struct Reading {
    end: usize,
    hash: u64,
    done: Vec<Arc<Block>>,
}

impl Reading {
    pub fn read(&mut self, text: &str) -> Vec<Arc<Block>> {
        let kept = text
            .get(..self.end)
            .is_some_and(|head| hashed(head) == self.hash);
        if !kept {
            *self = Self::default();
        }
        let Some(more) = settled(&text[self.end..]) else {
            *self = Self::default();
            return parse(text).into_iter().map(Arc::new).collect();
        };
        if more > 0 {
            let next = self.end + more;
            self.done.extend(
                parse(&text[self.end..next])
                    .into_iter()
                    .map(Arc::new),
            );
            self.end = next;
            self.hash = hashed(&text[..next]);
        }
        let mut all = self.done.clone();
        all.extend(
            parse(&text[self.end..]).into_iter().map(Arc::new),
        );
        all
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::testing::{plain, top};

    fn link(text: &str, target: Target) -> Span {
        Span {
            link: Some(target),
            ..plain(text)
        }
    }

    fn styled(text: &str, set: fn(&mut Style)) -> Span {
        let mut span = plain(text);
        set(&mut span.style);
        span
    }

    fn said(block: &Block) -> String {
        match &block.kind {
            Kind::Paragraph(spans) | Kind::Heading(_, spans) => {
                spans.iter().map(|s| s.text.as_str()).collect()
            }
            Kind::Code { text, .. } => text.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn a_link_keeps_the_place_it_leads() {
        let blocks = parse(
            "[a](docs/a.md:3) <https://b.c> [d](file:///e) \
             `src/f.rs:9` g\n\n```\nsrc/h.rs\n```",
        );
        let path = |path: &str| Target::Path(path.into());
        assert_eq!(
            blocks,
            [
                top(Kind::Paragraph(vec![
                    link("a", path("docs/a.md")),
                    plain(" "),
                    link(
                        "https://b.c",
                        Target::Web("https://b.c".into())
                    ),
                    plain(" d "),
                    Span {
                        link: Some(path("src/f.rs")),
                        ..styled("src/f.rs:9", |s| s.code = true)
                    },
                    plain(" g"),
                ])),
                top(Kind::Code {
                    lang: String::new(),
                    text: "src/h.rs".into(),
                }),
            ]
        );
    }

    #[test]
    fn a_picture_is_a_block_between_the_words_around_it() {
        let picture = |caption: &str, target| {
            top(Kind::Picture {
                caption: caption.into(),
                target,
            })
        };
        let path = |path: &str| Target::Path(path.into());
        let blocks = parse(
            "See ![the *old*\npress](shots/press.png) and\n\
             ![](https://a.b/c.png)\nafter.\n\n| ![x](y.png) \
             |\n|---|\n\n![no](file:///etc/passwd) ![half](shots/pr",
        );
        assert_eq!(
            blocks,
            [
                top(Kind::Paragraph(vec![plain("See ")])),
                picture("the old press", path("shots/press.png")),
                top(Kind::Paragraph(vec![plain("and ")])),
                picture("", Target::Web("https://a.b/c.png".into())),
                top(Kind::Paragraph(vec![plain("after.")])),
                top(Kind::Table {
                    align: vec![Align::Start],
                    rows: vec![vec![vec![link("x", path("y.png"))]]],
                }),
                top(Kind::Paragraph(vec![plain(
                    "no ![half](shots/pr"
                )])),
            ]
        );
    }

    #[test]
    fn lists_are_flat_blocks_that_say_how_deep_they_sit() {
        let blocks = parse(
            "- a\n- b\n  1. c\n  2. d\n     - e\n\n       more\n- \
             [x] f\n- [ ] g\n\n3. h\n4. i\n",
        );
        let shape = blocks
            .iter()
            .map(|b| (b.indent, b.marker, said(b)))
            .collect::<Vec<_>>();
        let s = str::to_string;
        assert_eq!(
            shape,
            [
                (1, Some(Marker::Bullet), s("a")),
                (1, Some(Marker::Bullet), s("b")),
                (2, Some(Marker::Number(1)), s("c")),
                (2, Some(Marker::Number(2)), s("d")),
                (3, Some(Marker::Bullet), s("e")),
                (3, None, s("more")),
                (1, Some(Marker::Task(true)), s("f")),
                (1, Some(Marker::Task(false)), s("g")),
                (1, Some(Marker::Number(3)), s("h")),
                (1, Some(Marker::Number(4)), s("i")),
            ]
        );
    }

    #[test]
    fn a_document_cut_off_anywhere_still_reads() {
        assert_eq!(
            parse("Some wor"),
            [top(Kind::Paragraph(vec![plain("Some wor")]))]
        );
        assert_eq!(
            parse("it is **bol"),
            [top(Kind::Paragraph(vec![plain("it is **bol")]))]
        );
        assert_eq!(
            parse("see [the doc](http://exa"),
            [top(Kind::Paragraph(vec![plain(
                "see [the doc](http://exa"
            )]))]
        );
        // A header row alone is a line of text until its rule comes.
        assert_eq!(parse("| a | b |\n|--").len(), 1);
        assert!(said(&parse("| a | b |\n|--")[0]).starts_with("| a"));
        let cut = parse("| a | b |\n|---|---|\n| 1 | 2 |\n| 3");
        let Kind::Table { rows, .. } = &cut[0].kind else {
            panic!("a table");
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2][0], [plain("3")]);

        let half = parse("- a\n- ");
        assert_eq!(half.len(), 2);
        assert_eq!(half[1].marker, Some(Marker::Bullet));
        assert_eq!(
            parse("Run:\n\n```sh\ncargo te"),
            [
                top(Kind::Paragraph(vec![plain("Run:")])),
                top(Kind::Code {
                    lang: "sh".into(),
                    text: "cargo te".into(),
                }),
            ]
        );
        assert_eq!(
            parse("```"),
            [top(Kind::Code {
                lang: String::new(),
                text: String::new(),
            })]
        );
    }

    #[test]
    fn every_prefix_of_a_document_parses() {
        let whole = "# T\n\nA *b* **c** `d` [e](f).\n\n- x\n  1. \
                     y\n\n> q\n\n```rs\ncode\n```\n\n| a | b \
                     |\n|---|--:|\n| 1 | 2 \
                     |\n\n---\n<div>\u{e9}</div>\n";
        for (end, _) in whole.char_indices() {
            let _ = parse(&whole[..end]);
        }
        assert_eq!(parse(whole).len(), 9);
    }

    #[test]
    fn a_document_read_as_it_grows_is_read_whole() {
        let texts = [
            "# Plan\n\nFirst *one*.\n\n```\na\n\nb\n```\n\n- x\n- \
             y\n\n  more\n\n1. one\n\n2. two\n\nLast words",
            "See [a].\n\nMore\n\n[a]: http://x",
            "````\n```\n\nx\n````\n\nAfter",
            "1. one\n\n1. two\n\n- a\n\n- b\n\nLast",
        ];
        for text in texts {
            let mut reading = Reading::default();
            for end in
                (0..=text.len()).filter(|n| text.is_char_boundary(*n))
            {
                let grown = reading.read(&text[..end]);
                let whole = parse(&text[..end]);
                assert_eq!(
                    grown.len(),
                    whole.len(),
                    "{text:?} {end}"
                );
                for (a, b) in grown.iter().zip(&whole) {
                    assert_eq!(**a, *b, "{text:?} {end}");
                }
            }
        }
    }

    #[test]
    fn hostile_nesting_ends_quickly() {
        let started = Instant::now();
        let n = 5000;
        let hostile = [
            "[".repeat(n),
            format!("{}a{}", "[".repeat(n), "]".repeat(n)),
            format!("{} a", ">".repeat(n)),
            format!("{} a", "> ".repeat(n)),
            "*".repeat(n),
            format!("{}a{}", "*a ".repeat(n), "*".repeat(n)),
            "`".repeat(n),
            "- ".repeat(n),
            (0..200)
                .map(|i| format!("{}- a\n", " ".repeat(i * 2)))
                .collect::<String>(),
            format!("{}\n|-|", "|".repeat(n)),
            "<".repeat(n),
            "![".repeat(n),
            "\\".repeat(n),
        ];
        for text in &hostile {
            assert!(!parse(text).is_empty(), "{text:.20}");
        }
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

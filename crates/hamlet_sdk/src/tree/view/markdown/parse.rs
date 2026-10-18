//! Markdown read into a flat list of blocks.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// The style of a run of text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
}

/// A run of text in one style.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Span {
    pub text: String,
    pub style: Style,
}

/// What a block is.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Kind {
    Text(Vec<Span>),
    /// A heading and its level, from 1.
    Heading(u8, Vec<Span>),
    Code(String),
    Rule,
}

/// One block, with the lists it sits in.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Block {
    /// The lists around it.
    pub indent: usize,
    /// The mark of the list item it opens.
    pub mark: Option<String>,
    pub kind: Kind,
}

#[derive(Default)]
struct Reader {
    blocks: Vec<Block>,
    /// The next number of each open list, `None` for bullets.
    lists: Vec<Option<u64>>,
    mark: Option<String>,
    spans: Vec<Span>,
    bold: u32,
    italic: u32,
    strike: u32,
    heading: Option<u8>,
    code: Option<String>,
}

impl Reader {
    fn emit(&mut self, kind: Kind) {
        self.blocks.push(Block {
            indent: self.lists.len(),
            mark: self.mark.take(),
            kind,
        });
    }

    /// Ends the run of inline text, as a heading or a paragraph.
    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.spans);
        let heading = self.heading.take();
        if spans.is_empty() {
            return;
        }
        self.emit(match heading {
            Some(level) => Kind::Heading(level, spans),
            None => Kind::Text(spans),
        });
    }

    fn text(&mut self, text: &str, code: bool) {
        if let Some(body) = &mut self.code {
            body.push_str(text);
            return;
        }
        let style = Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
        };
        match self.spans.last_mut() {
            Some(last) if last.style == style => {
                last.text.push_str(text)
            }
            _ => self.spans.push(Span {
                text: text.to_string(),
                style,
            }),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as u8);
            }
            Tag::List(first) => {
                self.flush();
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                self.mark = Some(match self.lists.last_mut() {
                    Some(Some(next)) => {
                        let mark = format!("{next}.");
                        *next += 1;
                        mark
                    }
                    _ => "\u{2022}".to_string(),
                });
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.code = Some(String::new());
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::Item => {
                self.flush()
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::CodeBlock => {
                if let Some(body) = self.code.take() {
                    self.emit(Kind::Code(
                        body.trim_end().to_string(),
                    ));
                }
            }
            TagEnd::Emphasis => {
                self.italic = self.italic.saturating_sub(1)
            }
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.strike = self.strike.saturating_sub(1)
            }
            _ => {}
        }
    }
}

/// The blocks of `text`. A link shows as its words, and what the
/// subset has no block for shows as plain text.
pub(super) fn parse(text: &str) -> Vec<Block> {
    let mut reader = Reader::default();
    for event in Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH)
    {
        match event {
            Event::Start(tag) => reader.start(tag),
            Event::End(tag) => reader.end(tag),
            Event::Text(text) | Event::Html(text) => {
                reader.text(&text, false)
            }
            Event::InlineHtml(text) => reader.text(&text, false),
            Event::Code(text) => reader.text(&text, true),
            Event::SoftBreak => reader.text(" ", false),
            Event::HardBreak => reader.text("\n", false),
            Event::Rule => {
                reader.flush();
                reader.emit(Kind::Rule);
            }
            _ => {}
        }
    }
    reader.flush();
    reader.blocks
}

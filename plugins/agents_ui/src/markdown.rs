//! Markdown as the engine's labels can set it: headings, paragraphs,
//! items, quotes, code and rules, one label each. What a label cannot
//! show inside a line (emphasis, code, a link's address) is left out
//! and its words kept.

use hamlet_engine_sdk::ui::{
    AnyView, FrameProps, Size, Space, Tone, ViewExt, column, divider,
    label, row,
};

/// One block of a text.
#[derive(Clone, PartialEq, Debug)]
enum Block {
    Heading(usize, String),
    Paragraph(String),
    Item(String),
    Quote(String),
    Code(String),
    Rule,
}

/// `text` as Markdown, its words in `tone`.
pub fn markdown(text: &str, tone: Tone) -> AnyView {
    let views = blocks(text)
        .into_iter()
        .map(|block| match block {
            Block::Heading(level, words) => label(words)
                .bold(true)
                .size(if level <= 2 {
                    Size::Heading
                } else {
                    Size::Body
                })
                .tone(tone)
                .boxed(),
            Block::Paragraph(words) => {
                label(words).tone(tone).boxed()
            }
            Block::Item(words) => row((
                label("-").tone(Tone::Faint),
                label(words).tone(tone),
            ))
            .space(Space::Half)
            .boxed(),
            Block::Quote(words) => row((
                label("|").tone(Tone::Faint),
                label(words).tone(Tone::Dim),
            ))
            .space(Space::Half)
            .boxed(),
            Block::Code(lines) => {
                label(lines).size(Size::Small).tone(Tone::Dim).boxed()
            }
            Block::Rule => divider().boxed(),
        })
        .collect::<Vec<_>>();
    column(views).space(Space::Half).boxed()
}

/// The blocks of `text`.
fn blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph = Vec::<&str>::new();
    let mut code = None::<Vec<&str>>;
    let end = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks
                .push(Block::Paragraph(inline(&paragraph.join(" "))));
            paragraph.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(lines) = &mut code {
            if trimmed.starts_with("```") {
                blocks.push(Block::Code(lines.join("\n")));
                code = None;
            } else {
                lines.push(line);
            }
            continue;
        }
        if trimmed.starts_with("```") {
            end(&mut paragraph, &mut blocks);
            code = Some(Vec::new());
            continue;
        }
        if trimmed.is_empty() {
            end(&mut paragraph, &mut blocks);
            continue;
        }
        let block = if let Some(level) = heading(trimmed) {
            Some(Block::Heading(
                level,
                inline(trimmed[level..].trim()),
            ))
        } else if matches!(trimmed, "---" | "***" | "___") {
            Some(Block::Rule)
        } else if let Some(quoted) = trimmed.strip_prefix('>') {
            Some(Block::Quote(inline(quoted.trim())))
        } else {
            item(trimmed).map(|words| Block::Item(inline(words)))
        };
        match block {
            Some(block) => {
                end(&mut paragraph, &mut blocks);
                blocks.push(block);
            }
            None => paragraph.push(trimmed),
        }
    }
    end(&mut paragraph, &mut blocks);
    if let Some(lines) = code {
        blocks.push(Block::Code(lines.join("\n")));
    }
    blocks
}

/// The level of a heading line: its count of `#`.
fn heading(line: &str) -> Option<usize> {
    let level = line.chars().take_while(|c| *c == '#').count();
    ((1..=6).contains(&level) && line[level..].starts_with(' '))
        .then_some(level)
}

/// The words of a list item, after its mark.
fn item(line: &str) -> Option<&str> {
    for mark in ["- ", "* ", "+ "] {
        if let Some(words) = line.strip_prefix(mark) {
            return Some(words);
        }
    }
    let digits =
        line.chars().take_while(char::is_ascii_digit).count();
    (digits > 0)
        .then(|| line[digits..].strip_prefix(". "))
        .flatten()
        .map(|_| line)
}

/// The words of a line with its emphasis and code marks taken out,
/// and each link or picture as its words.
fn inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let (before, after) = rest.split_at(open);
        let link =
            after[1..].split_once("](").and_then(|(words, more)| {
                more.split_once(')').map(|(_, rest)| (words, rest))
            });
        match link {
            Some((words, after)) => {
                // A picture is its words too.
                out.push_str(
                    before.strip_suffix('!').unwrap_or(before),
                );
                out.push_str(words);
                rest = after;
            }
            None => {
                out.push_str(before);
                out.push('[');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "").replace('`', "")
}

//! Markdown in a message: its blocks (`parse`), the links in its
//! words (`links`) and the views that draw them ([`document`],
//! [`fixed`], [`typed`]), a `picture` among them.

mod draw;
mod links;
mod parse;
mod picture;

pub use self::draw::{document, entry, fixed, typed};

/// The style of a run of text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
}

/// The place a link leads, as it was written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    /// A web or mail address.
    Web(String),
    /// A path with no line number, which may name no file.
    Path(String),
}

/// A run of text in one style.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<Target>,
}

/// The mark a list item opens with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker {
    Bullet,
    Number(u64),
    Task(bool),
}

/// The side a table's column sets its cells to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Start,
    Center,
    End,
}

/// The cells of a table's row.
pub type Row = Vec<Vec<Span>>;

/// The kind of a block, with its text.
#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    Paragraph(Vec<Span>),
    /// A heading and its level, from 1.
    Heading(u8, Vec<Span>),
    Code {
        lang: String,
        text: String,
    },
    Rule,
    /// A picture, with the words that stand for it.
    Picture {
        caption: String,
        target: Target,
    },
    /// The rows of a table, cell by cell, the header first.
    Table {
        align: Vec<Align>,
        rows: Vec<Row>,
    },
}

/// One block of a document. Lists and quotes are not blocks of their
/// own: each block says how deep it sits in them.
#[derive(Clone, PartialEq, Debug)]
pub struct Block {
    /// The block quotes around it.
    pub quote: u8,
    /// The lists around it.
    pub indent: u8,
    /// Set on the first block of a list item.
    pub marker: Option<Marker>,
    pub kind: Kind,
}

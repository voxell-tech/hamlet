//! Markdown: its blocks ([`parse()`]), the links in its words and the
//! views that draw them ([`document`], [`fixed`], [`typed`]). Where a
//! written path leads and how a picture is drawn are the app's, set
//! with [`PathLinks`] and [`PictureView`]; without them a path is
//! plain text and a picture is its caption.

mod draw;
mod links;
mod parse;

use std::path::PathBuf;

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;

pub use self::draw::{
    Face, Ink, document, fixed, picture_words, rich, typed, unsettle,
};
pub use self::parse::{Reading, hashed, parse};
use crate::AnyView;

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

/// The place a span of text leads when it is clicked.
#[derive(Component, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Anchor {
    Web(String),
    /// A file or a folder, with every link on the way to it followed.
    File(PathBuf),
}

/// Text whose words are selected as one, across every text node in
/// it.
#[derive(Component)]
pub struct Area;

/// The file a path written in a document names, if any. The app
/// moves `generation` on when the answers may have changed, and the
/// links are found again.
#[derive(Resource, Clone, Copy)]
pub struct PathLinks {
    pub resolve: fn(&mut World, &str) -> Option<PathBuf>,
    pub generation: u64,
}

/// How a picture block is drawn, from its caption and target. Without
/// it, a picture is [`picture_words`].
#[derive(Resource, Clone, Copy)]
pub struct PictureView(pub fn(&str, &Target, Ink) -> AnyView);

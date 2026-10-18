//! A panel's tree as the host holds it.

pub mod wire;

use hamlet_sdk::icons::Glyph;

/// One node of a panel: what it is, the id it is matched by among its
/// siblings, and its children.
#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    pub id: Option<String>,
    pub kind: Kind,
    pub children: Vec<Node>,
}

/// What a node is.
#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    Column,
    Row,
    Scroll,
    Label(String),
    Heading(String),
    Note(String),
    Icon(Glyph),
    Button(String),
    Checkbox { label: String, checked: bool },
    TextField(String),
    Divider,
}

/// What a node is, without what it holds. A node that changes its
/// tag is built again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    Column,
    Row,
    Scroll,
    Label,
    Heading,
    Note,
    Icon(Glyph),
    Button,
    Checkbox,
    TextField,
    Divider,
}

/// How a node is found among its siblings: by its id, or by its
/// place when it has none.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Key {
    pub name: String,
    pub tag: Tag,
}

impl Node {
    pub fn new(kind: Kind) -> Self {
        Self {
            id: None,
            kind,
            children: Vec::new(),
        }
    }

    /// This, with the id `id`.
    pub fn id(mut self, id: &str) -> Self {
        self.id = Some(id.into());
        self
    }

    /// This, with `children`.
    pub fn of(mut self, children: Vec<Node>) -> Self {
        self.children = children;
        self
    }

    pub fn tag(&self) -> Tag {
        match &self.kind {
            Kind::Column => Tag::Column,
            Kind::Row => Tag::Row,
            Kind::Scroll => Tag::Scroll,
            Kind::Label(_) => Tag::Label,
            Kind::Heading(_) => Tag::Heading,
            Kind::Note(_) => Tag::Note,
            Kind::Icon(glyph) => Tag::Icon(*glyph),
            Kind::Button(_) => Tag::Button,
            Kind::Checkbox { .. } => Tag::Checkbox,
            Kind::TextField(_) => Tag::TextField,
            Kind::Divider => Tag::Divider,
        }
    }

    /// The key of this node, the `at`th of its siblings.
    pub fn key(&self, at: usize) -> Key {
        Key {
            name: self.id.clone().unwrap_or_else(|| format!("#{at}")),
            tag: self.tag(),
        }
    }

    /// The words a node shows.
    pub fn text(&self) -> &str {
        match &self.kind {
            Kind::Label(text)
            | Kind::Heading(text)
            | Kind::Note(text)
            | Kind::Button(text)
            | Kind::TextField(text)
            | Kind::Checkbox { label: text, .. } => text,
            _ => "",
        }
    }

    pub fn checked(&self) -> bool {
        matches!(self.kind, Kind::Checkbox { checked: true, .. })
    }

    /// The keys of its children.
    pub fn keys(&self) -> Vec<Key> {
        self.children
            .iter()
            .enumerate()
            .map(|(at, child)| child.key(at))
            .collect()
    }

    /// The child of this key.
    pub fn child(&self, key: &Key) -> Option<&Node> {
        self.children
            .iter()
            .enumerate()
            .find(|(at, child)| child.key(*at) == *key)
            .map(|(_, child)| child)
    }
}

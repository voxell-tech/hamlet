//! The widget tree a plugin describes a panel with, and the engine
//! draws with its own styles.
//!
//! The tree is flat, in pre-order: each node is followed by its
//! children, each with theirs, and says how many children it has.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A node's id, given by the plugin, kept from one tree to the next
/// so the engine can match a node to the one it drew before.
#[derive(
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
)]
pub struct WidgetId(pub String);

/// One kind of widget, with what it shows.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Widget {
    Text(String),
    Heading(String),
    /// An icon of the engine's set, by name.
    Icon(String),
    /// A button with its label.
    Button(String),
    Toggle {
        label: String,
        on: bool,
    },
    /// A single line of editable text.
    Field {
        value: String,
        placeholder: String,
    },
    /// Its children as rows of a list.
    List,
    Row,
    Column,
    /// Its children in a column that scrolls.
    Scroll,
    Divider,
}

impl Widget {
    /// Whether it may have children.
    pub fn holds(&self) -> bool {
        matches!(
            self,
            Self::List | Self::Row | Self::Column | Self::Scroll
        )
    }

    /// Every text it shows.
    fn texts(&self) -> impl Iterator<Item = &str> {
        let (one, two) = match self {
            Self::Text(text)
            | Self::Heading(text)
            | Self::Icon(text)
            | Self::Button(text)
            | Self::Toggle { label: text, .. } => (Some(text), None),
            Self::Field { value, placeholder } => {
                (Some(value), Some(placeholder))
            }
            _ => (None, None),
        };
        one.into_iter().chain(two).map(String::as_str)
    }
}

/// A widget, its id and how many children follow it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct WidgetNode {
    pub id: WidgetId,
    pub children: u32,
    pub widget: Widget,
}

/// A panel's content: one root and everything under it, in
/// pre-order. Empty for a panel with nothing in it.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct WidgetTree {
    pub nodes: Vec<WidgetNode>,
}

/// What the person did to a widget of one of a plugin's panels.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct WidgetEvent {
    /// The panel's or settings page's name within the plugin.
    pub panel: String,
    pub widget: WidgetId,
    pub action: Action,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// A button pressed.
    Click,
    /// A toggle switched, to this.
    Toggle(bool),
    /// A field's text, as the person left it.
    Text(String),
}

/// The most a tree may hold.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TreeLimits {
    pub nodes: usize,
    /// The deepest a node may sit, the root being 1.
    pub depth: usize,
    /// The longest text of one widget, in bytes.
    pub text: usize,
}

/// Why a tree is refused.
#[derive(Clone, PartialEq, Debug)]
pub enum TreeError {
    TooManyNodes,
    TooDeep,
    TooLong(WidgetId),
    /// A widget that holds nothing, given children.
    Leaf(WidgetId),
    SameId(WidgetId),
    /// More nodes than the root and its children account for.
    Trailing,
    /// Fewer nodes than the child counts call for.
    Missing,
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyNodes => write!(f, "too many widgets"),
            Self::TooDeep => write!(f, "widgets nested too deep"),
            Self::TooLong(id) => {
                write!(f, "{:?} holds too much text", id.0)
            }
            Self::Leaf(id) => {
                write!(f, "{:?} cannot hold other widgets", id.0)
            }
            Self::SameId(id) => {
                write!(f, "two widgets are {:?}", id.0)
            }
            Self::Trailing => {
                write!(f, "widgets after the root's last")
            }
            Self::Missing => write!(f, "fewer widgets than counted"),
        }
    }
}

impl std::error::Error for TreeError {}

impl WidgetTree {
    /// Whether the counts make one tree within `limits`.
    pub fn check(
        &self,
        limits: &TreeLimits,
    ) -> Result<(), TreeError> {
        if self.nodes.len() > limits.nodes {
            return Err(TreeError::TooManyNodes);
        }
        // The children still to come of each node open above this
        // one.
        let mut open = Vec::<u32>::new();
        let mut seen = std::collections::HashSet::new();
        for (at, node) in self.nodes.iter().enumerate() {
            if at > 0 && open.is_empty() {
                return Err(TreeError::Trailing);
            }
            if !seen.insert(&node.id) {
                return Err(TreeError::SameId(node.id.clone()));
            }
            if node
                .widget
                .texts()
                .any(|text| text.len() > limits.text)
            {
                return Err(TreeError::TooLong(node.id.clone()));
            }
            if let Some(left) = open.last_mut() {
                *left -= 1;
            }
            if node.children > 0 {
                if !node.widget.holds() {
                    return Err(TreeError::Leaf(node.id.clone()));
                }
                if open.len() + 2 > limits.depth {
                    return Err(TreeError::TooDeep);
                }
                open.push(node.children);
            }
            while open.last() == Some(&0) {
                open.pop();
            }
        }
        if open.is_empty() {
            Ok(())
        } else {
            Err(TreeError::Missing)
        }
    }
}

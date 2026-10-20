//! A widget tree as text: a line for each widget, indented under the
//! one that holds it, with its kind, its id after `#` and what it
//! shows.
//!
//! ```text
//! column
//!   heading #title "Board"
//!   button #save "Save"
//! ```

use hamlet_engine_api::{Widget, WidgetTree};

/// `tree` as an outline.
pub fn outline(tree: &WidgetTree) -> String {
    let mut lines = Vec::new();
    // The children still to come of each widget open above this one.
    let mut open = Vec::<u32>::new();
    for node in &tree.nodes {
        let mut line = "  ".repeat(open.len());
        let (kind, shows) = match &node.widget {
            Widget::Text(text) => ("text", quoted(text)),
            Widget::Heading(text) => ("heading", quoted(text)),
            Widget::Icon(name) => ("icon", quoted(name)),
            Widget::Button(label) => ("button", quoted(label)),
            Widget::Toggle { label, on } => (
                "toggle",
                format!(
                    "{} {}",
                    quoted(label),
                    if *on { "on" } else { "off" }
                ),
            ),
            Widget::Field { value, placeholder } => (
                "field",
                format!("{} ({})", quoted(value), placeholder),
            ),
            Widget::List => ("list", String::new()),
            Widget::Row => ("row", String::new()),
            Widget::Column => ("column", String::new()),
            Widget::Scroll => ("scroll", String::new()),
            Widget::Divider => ("divider", String::new()),
        };
        line.push_str(kind);
        if !node.id.0.is_empty() {
            line.push_str(&format!(" #{}", node.id.0));
        }
        if !shows.is_empty() {
            line.push(' ');
            line.push_str(&shows);
        }
        lines.push(line);
        if let Some(left) = open.last_mut() {
            *left -= 1;
        }
        if node.children > 0 {
            open.push(node.children);
        }
        while open.last() == Some(&0) {
            open.pop();
        }
    }
    lines.join("\n")
}

fn quoted(text: &str) -> String {
    format!("{text:?}")
}

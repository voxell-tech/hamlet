//! The widget tree a panel is drawn with, built in order.

use hamlet_engine_api::{Widget, WidgetId, WidgetNode, WidgetTree};

/// A panel's widgets, under a column the engine is given with the
/// empty id. Each widget has an id of the plugin's, kept from one
/// drawing to the next, and unique in the panel.
pub struct Ui {
    nodes: Vec<WidgetNode>,
    /// The widgets being filled, innermost last.
    open: Vec<usize>,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            nodes: vec![WidgetNode {
                id: WidgetId(String::new()),
                children: 0,
                widget: Widget::Column,
            }],
            open: vec![0],
        }
    }
}

impl Ui {
    pub fn text(
        &mut self,
        id: &str,
        text: impl Into<String>,
    ) -> &mut Self {
        self.leaf(id, Widget::Text(text.into()))
    }

    pub fn heading(
        &mut self,
        id: &str,
        text: impl Into<String>,
    ) -> &mut Self {
        self.leaf(id, Widget::Heading(text.into()))
    }

    /// The icon `name` of the engine's set.
    pub fn icon(&mut self, id: &str, name: &str) -> &mut Self {
        self.leaf(id, Widget::Icon(name.into()))
    }

    /// A button; a press is an [`Action::Click`](crate::Action).
    pub fn button(
        &mut self,
        id: &str,
        label: impl Into<String>,
    ) -> &mut Self {
        self.leaf(id, Widget::Button(label.into()))
    }

    /// A toggle showing `on`; a press is an
    /// [`Action::Toggle`](crate::Action) with the state asked for.
    pub fn toggle(
        &mut self,
        id: &str,
        label: impl Into<String>,
        on: bool,
    ) -> &mut Self {
        self.leaf(
            id,
            Widget::Toggle {
                label: label.into(),
                on,
            },
        )
    }

    /// A line of text to edit; Enter, or leaving it changed, is an
    /// [`Action::Text`](crate::Action) with its text.
    pub fn field(
        &mut self,
        id: &str,
        value: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> &mut Self {
        self.leaf(
            id,
            Widget::Field {
                value: value.into(),
                placeholder: placeholder.into(),
            },
        )
    }

    pub fn divider(&mut self, id: &str) -> &mut Self {
        self.leaf(id, Widget::Divider)
    }

    /// The widgets `fill` adds, as rows of a list.
    pub fn list(
        &mut self,
        id: &str,
        fill: impl FnOnce(&mut Ui),
    ) -> &mut Self {
        self.holder(id, Widget::List, fill)
    }

    pub fn row(
        &mut self,
        id: &str,
        fill: impl FnOnce(&mut Ui),
    ) -> &mut Self {
        self.holder(id, Widget::Row, fill)
    }

    pub fn column(
        &mut self,
        id: &str,
        fill: impl FnOnce(&mut Ui),
    ) -> &mut Self {
        self.holder(id, Widget::Column, fill)
    }

    /// The widgets `fill` adds, in a column that scrolls.
    pub fn scroll(
        &mut self,
        id: &str,
        fill: impl FnOnce(&mut Ui),
    ) -> &mut Self {
        self.holder(id, Widget::Scroll, fill)
    }

    pub(crate) fn finish(self) -> WidgetTree {
        WidgetTree { nodes: self.nodes }
    }

    fn leaf(&mut self, id: &str, widget: Widget) -> &mut Self {
        self.push(id, widget);
        self
    }

    fn holder(
        &mut self,
        id: &str,
        widget: Widget,
        fill: impl FnOnce(&mut Ui),
    ) -> &mut Self {
        let at = self.push(id, widget);
        self.open.push(at);
        fill(self);
        self.open.pop();
        self
    }

    fn push(&mut self, id: &str, widget: Widget) -> usize {
        if let Some(&parent) = self.open.last() {
            self.nodes[parent].children += 1;
        }
        self.nodes.push(WidgetNode {
            id: WidgetId(id.into()),
            children: 0,
            widget,
        });
        self.nodes.len() - 1
    }
}

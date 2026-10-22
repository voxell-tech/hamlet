//! Rows of a list as the shell's Plugins list shows them: each lit
//! inside under the pointer, a thin divider between each and the
//! next.

use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, Tone,
    ViewExt, column, divider, icon, pressable,
};

/// A row of a list that leads somewhere: `content`, and a chevron at
/// its end. Its root is the button.
pub fn link_row(content: AnyView) -> AnyView {
    pressable((content, icon("chevron_right").tone(Tone::Dim)))
        .style(ButtonStyle::Row)
        .boxed()
}

/// `rows` as one list, a divider between each and the next.
pub fn link_rows(rows: Vec<AnyView>) -> AnyView {
    let mut parts = Vec::new();
    for row in rows {
        if !parts.is_empty() {
            parts.push(divider().boxed());
        }
        parts.push(row);
    }
    column(parts).space(Space::None).boxed()
}

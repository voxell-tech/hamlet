//! Hamlet's widget kit: the theme, the styles and the widgets the
//! panels are built from, on fynix. It holds no panels and reads no
//! client state; the window in `hamlet_app` does.

pub mod fields;
pub mod styles;
pub mod theme;
pub mod widgets;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy_fynix::Bevy;

pub use self::styles::{
    ICON, back_link, button, danger_button, marked, primary_button,
    text_input,
};
use self::theme::HamletTheme;
pub use self::widgets::{
    dial, disclosure, effects, fade, icons, platform, switch, tabs,
};

/// The theme, the widgets' systems and the fields.
pub fn plugin(app: &mut App) {
    app.add_plugins((theme::plugin, widgets::plugin, fields::plugin));
}

/// The context a view is built in.
pub type Cx<'a> = bevy_fynix::Cx<'a, Bevy, HamletTheme>;

/// A view, its type erased.
pub type AnyView = bevy_fynix::AnyView<Bevy, HamletTheme>;

/// A view. A trait of its own until Rust has trait aliases.
pub trait View:
    bevy_fynix::View<Bevy, HamletTheme> + 'static
{
}

impl<V: bevy_fynix::View<Bevy, HamletTheme> + 'static> View for V {}

/// A view built by a closure, with the context in hand.
pub struct Built<F>(F);

impl<F> bevy_fynix::View<Bevy, HamletTheme> for Built<F>
where
    F: FnOnce(&mut Cx) -> Entity,
{
    fn build(self, cx: &mut Cx<'_>) -> Entity {
        (self.0)(cx)
    }
}

/// The view `build` makes with the context in hand, written in
/// place.
pub fn built(
    build: impl FnOnce(&mut Cx) -> Entity + 'static,
) -> impl View {
    Built(build)
}

/// The view `make` gives with the context in hand, for one made
/// where there is none, as in a `keyed` view.
pub fn with_cx<V: View>(
    make: impl FnOnce(&Cx) -> V + 'static,
) -> impl View {
    built(move |cx| {
        let view = make(cx);
        cx.build(view)
    })
}

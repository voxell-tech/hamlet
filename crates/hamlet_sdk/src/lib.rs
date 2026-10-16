//! What a feature of the command center is built with. A feature
//! crate depends on this and on the crates below it, never on
//! `hamlet_ui`.

pub mod cue;
pub mod feel;
pub mod icons;
pub mod looks;
pub mod me;
pub mod platform;
pub mod tabs;
pub mod theme;

use bevy::app::App;
use bevy_fynix::Bevy;

use self::theme::HamletTheme;

/// What every panel is built on: the typefaces, the tabs, the icons
/// and the feel. A theme is the app's to give.
pub fn plugin(app: &mut App) {
    app.add_plugins((
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        feel::plugin,
    ));
}

/// The context a view of the window is built in.
pub type Cx<'a> = bevy_fynix::Cx<'a, Bevy, HamletTheme>;

/// A view of the window, its type erased.
pub type AnyView = bevy_fynix::AnyView<Bevy, HamletTheme>;

/// A view of the window. A trait of its own until Rust has trait
/// aliases.
pub trait View:
    bevy_fynix::View<Bevy, HamletTheme> + 'static
{
}

impl<V: bevy_fynix::View<Bevy, HamletTheme> + 'static> View for V {}

/// The view `make` gives with the context in hand, for one made
/// where there is none, as in a `keyed` view.
pub fn with_cx<V: View>(
    make: impl FnOnce(&Cx) -> V + 'static,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let view = make(cx);
        cx.build(view)
    })
}

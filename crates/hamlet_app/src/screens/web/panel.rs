//! A Web panel's view: the bar of its address over the page.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Overflow, UiRect, percent, px};
use bevy_cef::prelude::{
    CefStatus, RequestGoBack, RequestGoForward, RequestReload,
    RequestShowDevTool,
};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::shortcut::{
    Layer, Scope, ScopeId, ScopeSpec, ShortcutAppExt,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_protocol::client::web_address;
use hamlet_ui::styles::{
    Fills, ICON, action, description, link_row, note, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::fetch::{self, Fetch, State};
use super::{Addresses, PANEL, page};
use crate::dock::{PanelAppExt, flush_panel};

/// The name of a Web panel's tab, before its page has a title.
const TITLE: &str = "WEB";

/// The letters of a page's title a tab shows.
const TITLE_LETTERS: usize = 24;

/// The scope of a Web panel's commands.
pub(super) const SCOPE: ScopeId = ScopeId("web");

pub(super) fn plugin(app: &mut App) {
    app.add_numbered_panel(PANEL, window).add_scope(ScopeSpec {
        id: SCOPE,
        label: "Web panel",
        layer: Layer::Panel,
        active: None,
    });
}

/// The dock window of the Web panel `id`.
fn window(id: &str) -> DockWindowKind<HamletTheme> {
    let panel = PanelId(id.to_owned());
    DockWindowKind::new(TITLE, move || flush_panel(contents(&panel)))
        .icon(Glyph::Globe.image(ICON))
}

/// The id of a Web panel in the dock.
#[derive(Component, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) struct PanelId(pub String);

/// The node a Web panel's page is drawn in.
#[derive(Component)]
pub(super) struct Surface(pub PanelId);

/// A Web panel's field of its address.
#[derive(Component)]
pub(super) struct AddressField;

/// The name of the tab of a panel whose page has `title`, cut to
/// [`TITLE_LETTERS`], and that is `agent`'s when it is an agent's.
pub(super) fn tab_name(title: &str, agent: Option<&str>) -> String {
    let named = match agent {
        Some(agent) => format!("{TITLE} ({agent})"),
        None => TITLE.to_owned(),
    };
    let title = title.trim();
    if title.is_empty() {
        return named;
    }
    let mut cut =
        title.chars().take(TITLE_LETTERS).collect::<String>();
    if title.chars().count() > TITLE_LETTERS {
        cut.push_str("...");
    }
    format!("{named}: {cut}")
}

/// Whether the dock window `id` is a Web panel.
pub(super) fn is_panel(id: &str) -> bool {
    id == PANEL || crate::dock::number(PANEL, id).is_some()
}

fn contents(panel: &PanelId) -> impl View + use<> {
    column((bar(panel), body(panel)))
        .fills_width()
        .fills_height()
        .gap(0.0)
        .tagged(Scope(SCOPE))
        .tagged(panel.clone())
}

/// Back, forward and reload, the address, and the developer tools.
fn bar(panel: &PanelId) -> impl View + use<> {
    let panel = panel.clone();
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        row((
            press(Glyph::Back, "Back", &panel, |world, page| {
                world.trigger(RequestGoBack { webview: page });
            }),
            press(
                Glyph::Forward,
                "Forward",
                &panel,
                |world, page| {
                    world.trigger(RequestGoForward { webview: page });
                },
            ),
            press(Glyph::Restore, "Reload", &panel, |world, page| {
                world.trigger(RequestReload { webview: page });
            }),
            address(&panel),
            press(
                Glyph::Lens,
                "Developer tools",
                &panel,
                |world, page| {
                    world.trigger(RequestShowDevTool {
                        webview: page,
                    });
                },
            ),
        ))
        .gap(gap / 2.0)
        .padding(UiRect::all(px(gap / 2.0)))
        .align(AlignItems::Center)
        .width(percent(100.0))
        .shrink(0.0)
    })
}

/// The field of `panel`'s address, which loads what is typed in it.
fn address(panel: &PanelId) -> impl View + use<> {
    let shown = panel.clone();
    let typed = panel.clone();
    text_field(
        derived(move |world: &World| {
            world
                .resource::<Addresses>()
                .0
                .get(&shown.0)
                .cloned()
                .unwrap_or_default()
        })
        .when(every_frame()),
        move |world, words| go(world, &typed, &words),
    )
    .grow(1.0)
    .min_width(px(0.0))
    .rules(text_input)
    .tagged(AddressField)
}

/// A small press of `which`, named by a tooltip, that acts on the
/// panel's page once there is one.
fn press(
    which: Glyph,
    words: &'static str,
    panel: &PanelId,
    act: fn(&mut World, Entity),
) -> AnyView {
    let panel = panel.clone();
    button(glyph(which, ICON).tone(Tone::Dim))
        .padding(UiRect::all(px(4.0)))
        .rules(link_row)
        .on_activate_with(move |world, _| {
            if let Some(page) = page::of(world, &panel) {
                act(world, page);
            }
        })
        .tooltip(move || note(words))
        .boxed()
}

/// Loads the address typed into `panel`'s bar.
pub(super) fn go(world: &mut World, panel: &PanelId, typed: &str) {
    let Some(address) = web_address(typed) else {
        return;
    };
    world
        .resource_mut::<Addresses>()
        .0
        .insert(panel.0.clone(), address.clone());
    page::navigate(world, panel, address);
}

/// What a panel's body holds.
#[derive(Clone, PartialEq)]
enum Body {
    /// The engine is not on this machine, and how fetching it goes.
    Missing(State),
    /// The engine did not start, for the reason given.
    Failed(String),
    /// No address yet.
    Blank,
    Page,
}

/// The page, or the words of a panel that has none.
fn body(panel: &PanelId) -> impl View + use<> {
    let shown = panel.clone();
    let panel = panel.clone();
    keyed(
        derived(move |world: &World| {
            if world.get_resource::<CefStatus>().is_none() {
                Body::Missing(world.resource::<Fetch>().state())
            } else if let Some(CefStatus::Failed(error)) =
                world.get_resource::<CefStatus>()
            {
                Body::Failed(error.to_string())
            } else if world
                .resource::<Addresses>()
                .0
                .contains_key(&shown.0)
            {
                Body::Page
            } else {
                Body::Blank
            }
        })
        .when(every_frame()),
        move |body| match body {
            Body::Missing(state) => missing(state),
            Body::Failed(error) => words(format!(
                "The web engine did not start: {error}. A Web panel \
                 needs CEF where CEF_PATH points."
            )),
            Body::Blank => words(
                "Type an address above: a site, or localhost and a \
                 port for one an agent serves on this machine."
                    .to_owned(),
            ),
            Body::Page => frame()
                .grow(1.0)
                .fills_width()
                .min_height(px(0.0))
                .overflow(Overflow::clip())
                .tagged(Surface(panel.clone()))
                .boxed(),
        },
    )
    .within(
        column(())
            .gap(0.0)
            .grow(1.0)
            .fills_width()
            .min_height(px(0.0)),
    )
}

/// What a panel shows while the engine is not on this machine: the
/// press that fetches it, and then how that goes.
fn missing(state: &State) -> AnyView {
    let (said, press) = match state {
        State::Idle => (
            "Web panels need the web engine, CEF, which is not on \
             this machine yet. Fetching it puts about 340 MB from \
             CEF's own builds in your caches."
                .to_owned(),
            Some("FETCH THE WEB ENGINE"),
        ),
        State::Fetching(bytes) => (
            format!(
                "Fetching the web engine: {} MB so far.",
                bytes / 1_000_000
            ),
            None,
        ),
        State::Fetched => (
            "The web engine is fetched. Close the window and open \
             it again to use it."
                .to_owned(),
            None,
        ),
        State::Failed(why) => (
            format!("Fetching the web engine failed: {why}."),
            Some("TRY AGAIN"),
        ),
    };
    with_cx(move |cx| {
        let section = cx.theme().space.section;
        let said = description(said.clone());
        let shown = match press {
            Some(text) => column((
                said,
                row((action(Glyph::Lower, text, fetch::start),)),
            ))
            .boxed(),
            None => column((said,)).boxed(),
        };
        column((shown,))
            .padding(UiRect::all(px(section * 2.0)))
            .fills_width()
    })
    .boxed()
}

fn words(words: String) -> AnyView {
    with_cx(move |cx| {
        let section = cx.theme().space.section;
        column((description(words.clone()),))
            .padding(UiRect::all(px(section * 2.0)))
            .fills_width()
    })
    .boxed()
}

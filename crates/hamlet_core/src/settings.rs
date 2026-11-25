//! The pages of Settings, which each part adds for its own settings,
//! and opening Settings at one.

use bevy::app::App;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, ResMut};
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{Prop, derived};
use hamlet_ui::styles::{close, section};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use crate::panels::ShowPanel;
use crate::roll::moved;

pub fn plugin(app: &mut App) {
    app.init_resource::<SettingsPages>()
        .init_resource::<SettingsPane>()
        .add_observer(open);
}

/// The id of the Settings panel.
pub const PANEL: &str = "settings";

/// The id of the Endpoints page.
pub const ENDPOINTS: &str = "endpoints";
/// The id of the Models page.
pub const MODELS: &str = "models";

/// A page of Settings.
#[derive(Clone)]
pub struct SettingsPage {
    pub id: &'static str,
    pub title: &'static str,
    pub glyph: Glyph,
    /// Where the page stands in the list of pages, lowest first.
    pub order: u32,
    /// What the page holds now, as the list of pages shows it.
    pub now: Option<fn() -> Prop<String>>,
    pub body: fn() -> AnyView,
    /// What stands beside the page's name over it.
    pub beside_title: Option<fn() -> AnyView>,
    /// Whether the page is as wide as a table rather than a page.
    pub wide: bool,
}

impl SettingsPage {
    pub fn new(
        id: &'static str,
        title: &'static str,
        glyph: Glyph,
        order: u32,
        body: fn() -> AnyView,
    ) -> Self {
        Self {
            id,
            title,
            glyph,
            order,
            now: None,
            body,
            beside_title: None,
            wide: false,
        }
    }

    pub fn now(mut self, now: fn() -> Prop<String>) -> Self {
        self.now = Some(now);
        self
    }

    pub fn beside_title(mut self, beside: fn() -> AnyView) -> Self {
        self.beside_title = Some(beside);
        self
    }

    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }
}

/// The pages of Settings, in their order.
#[derive(Resource, Default)]
pub struct SettingsPages(Vec<SettingsPage>);

impl SettingsPages {
    pub fn get(&self, id: &str) -> Option<&SettingsPage> {
        self.0.iter().find(|page| page.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SettingsPage> {
        self.0.iter()
    }
}

/// Adds a page to Settings from a part's own plugin.
pub trait SettingsAppExt {
    fn add_settings_page(&mut self, page: SettingsPage) -> &mut Self;
}

impl SettingsAppExt for App {
    fn add_settings_page(&mut self, page: SettingsPage) -> &mut Self {
        self.init_resource::<SettingsPages>();
        let mut pages =
            self.world_mut().resource_mut::<SettingsPages>();
        let at = pages
            .0
            .partition_point(|other| other.order <= page.order);
        pages.0.insert(at, page);
        self
    }
}

/// The id of the page Settings shows, or none for the list of pages.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SettingsPane(pub Option<&'static str>);

/// Brings Settings forward at the page of this id, or at the list of
/// pages.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct OpenSettings(pub Option<&'static str>);

fn open(
    open: On<OpenSettings>,
    mut pane: ResMut<SettingsPane>,
    mut commands: Commands,
) {
    pane.0 = open.0;
    commands.trigger(ShowPanel(PANEL.into()));
}

/// A part of a page: its name over `value`.
pub fn field(name: &'static str, value: impl View) -> impl View {
    close((section(name), value))
}

/// What `read` makes of `R`, or nothing while there is no `R`.
pub fn shown<R: Resource>(
    read: impl Fn(&R) -> String + Send + Sync + 'static,
) -> Prop<String> {
    derived(move |world: &World| {
        world.get_resource::<R>().map(&read).unwrap_or_default()
    })
    .when(moved::<R>())
    .into()
}

/// A setting's switch or field, and under it its words.
pub fn told(control: impl View, words: impl View) -> impl View {
    with_cx(move |cx| {
        column((control, words))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::FlexStart)
    })
}

/// A setting's control alone, as wide as itself.
pub fn alone(control: impl View) -> impl View {
    column((control,)).align(AlignItems::FlexStart)
}

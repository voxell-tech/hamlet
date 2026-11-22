//! The items of the window's Hamlet menu, which each part adds for
//! what it offers there.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy_fynix::shortcut::Chord;
use hamlet_ui::widgets::icons::Glyph;

pub fn plugin(app: &mut App) {
    app.init_resource::<MenuItems>();
}

/// An item of the Hamlet menu.
#[derive(Clone)]
pub struct MenuItem {
    pub glyph: Glyph,
    pub name: &'static str,
    /// The keys that do the same, shown beside the name.
    pub keys: Option<Chord>,
    /// Where the item stands in the menu, lowest first.
    pub order: u32,
    /// Whether the menu offers the item now.
    pub shown: fn(&World) -> bool,
    pub act: fn(&mut Commands),
}

impl MenuItem {
    pub fn new(
        glyph: Glyph,
        name: &'static str,
        order: u32,
        act: fn(&mut Commands),
    ) -> Self {
        Self {
            glyph,
            name,
            keys: None,
            order,
            shown: |_| true,
            act,
        }
    }

    pub fn keys(mut self, keys: Chord) -> Self {
        self.keys = Some(keys);
        self
    }

    pub fn shown(mut self, shown: fn(&World) -> bool) -> Self {
        self.shown = shown;
        self
    }
}

/// The items of the Hamlet menu, in their order.
#[derive(Resource, Default)]
pub struct MenuItems(Vec<MenuItem>);

impl MenuItems {
    pub fn iter(&self) -> impl Iterator<Item = &MenuItem> {
        self.0.iter()
    }
}

/// Adds an item to the Hamlet menu from a part's own plugin.
pub trait MenuAppExt {
    fn add_menu_item(&mut self, item: MenuItem) -> &mut Self;
}

impl MenuAppExt for App {
    fn add_menu_item(&mut self, item: MenuItem) -> &mut Self {
        self.init_resource::<MenuItems>();
        let mut items = self.world_mut().resource_mut::<MenuItems>();
        let at = items
            .0
            .partition_point(|other| other.order <= item.order);
        items.0.insert(at, item);
        self
    }
}

//! Cards and menus floating over the window. A [`Card`] is a view of
//! the plugin's built on an entity of its own, shown at its
//! [`CardPlace`]: beside the pointer, where the pointer is, or hung
//! over an entity of the scene. A menu opens at the pointer with
//! [`MenuExt::open_menu`].
//!
//! ```ignore
//! fn hover(mut commands: Commands) {
//!     commands.spawn((
//!         Card::new(CardLook::Card, 280.0, hover_card),
//!         CardPlace::Pointer,
//!     ));
//! }
//! ```

use bevy_app::{App, PostUpdate};
use bevy_ecs::prelude::*;

pub use crate::bindings::hamlet::engine::ui::CardLook;
use crate::bindings::hamlet::engine::ui::{
    CardPlace as WitPlace, Node, make_card, open_menu, place_card,
};
use crate::scene::{HostEntity, SceneSync};
use crate::ui::{AnyView, Handler, Key, MenuItem, Ui};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<CardMounts>()
        .init_resource::<MenuShown>()
        .add_message::<CardClosed>()
        .add_observer(unmount)
        .add_systems(
            PostUpdate,
            (make, place).chain().after(SceneSync),
        );
}

/// A card floating over the window, of the view `view` builds given
/// the card's entity.
#[derive(Component, Clone, Copy, Debug)]
#[require(CardPlace)]
pub struct Card {
    pub look: CardLook,
    /// The widest it is drawn, in the interface's pixels.
    pub widest: f32,
    pub view: fn(Entity) -> AnyView,
}

impl Card {
    pub fn new(
        look: CardLook,
        widest: f32,
        view: fn(Entity) -> AnyView,
    ) -> Self {
        Self { look, widest, view }
    }
}

/// Where a [`Card`] is shown.
#[derive(Component, Clone, Copy, PartialEq, Debug, Default)]
pub enum CardPlace {
    #[default]
    Hidden,
    /// Beside the pointer, out of its way, following it; the pointer
    /// reaches what is under it.
    Pointer,
    /// Where the pointer is as it is placed, over everything, until
    /// a press elsewhere or Escape closes it, a [`CardClosed`].
    Here,
    /// `gap` pixels over `entity`, an entity of the scene, in the
    /// first view of the scene shown, cut where the view ends.
    Over { entity: Entity, gap: f32 },
}

/// A card placed [`CardPlace::Here`] was closed by a press elsewhere
/// or Escape; its place is hidden again.
#[derive(Message, Clone, Copy, Debug)]
pub struct CardClosed(pub Entity);

/// Whether a menu opened with [`MenuExt::open_menu`] is open.
#[derive(Resource, Clone, Copy, Default, Debug)]
pub struct MenuShown(pub bool);

/// The engine's card of a [`Card`]: its frame's key, and the place
/// last sent.
#[derive(Component)]
struct HostCard {
    root: Key,
    sent: CardPlace,
}

/// Cards made in the engine, to be built once the app's update ends,
/// and those to take down.
#[derive(Resource, Default)]
pub(crate) struct CardMounts {
    /// The name, the frame's key and the frame of each card made,
    /// and its entity.
    pub(crate) made: Vec<(String, Key, Node, Entity)>,
    pub(crate) gone: Vec<String>,
}

/// The name a card is built under, among the panels.
fn name(card: Entity) -> String {
    format!("card {}", card.to_bits())
}

fn make(
    cards: Query<(Entity, &Card), Without<HostCard>>,
    mut ui: ResMut<Ui>,
    mut mounts: ResMut<CardMounts>,
    mut commands: Commands,
) {
    for (entity, card) in &cards {
        let key = ui.nodes.reserve();
        let node = make_card(key, card.look, card.widest);
        let closed = move |world: &mut World| {
            if let Ok(mut card) = world.get_entity_mut(entity) {
                card.insert(CardPlace::Hidden);
            }
            if let Some(mut host) = world.get_mut::<HostCard>(entity)
            {
                host.sent = CardPlace::Hidden;
            }
            world.write_message(CardClosed(entity));
        };
        ui.handlers
            .entry(key)
            .or_default()
            .push(Handler::Closed(Box::new(closed)));
        mounts.made.push((name(entity), key, node, entity));
        commands.entity(entity).insert(HostCard {
            root: key,
            sent: CardPlace::Hidden,
        });
    }
}

/// Sends each card's place as it changes, once its frame is built.
fn place(
    mut cards: Query<(&CardPlace, &mut HostCard)>,
    hosts: Query<&HostEntity>,
    ui: Res<Ui>,
) {
    for (place, mut host) in &mut cards {
        if host.sent == *place {
            continue;
        }
        let Some(node) = ui.nodes.node(host.root) else {
            continue;
        };
        match *place {
            CardPlace::Hidden => place_card(node, WitPlace::Hidden),
            CardPlace::Pointer => place_card(node, WitPlace::Pointer),
            CardPlace::Here => place_card(node, WitPlace::Here),
            CardPlace::Over { entity, gap } => {
                let Ok(over) = hosts.get(entity) else {
                    continue;
                };
                over.entity.hang_card(Some(node), gap);
            }
        }
        host.sent = *place;
    }
}

fn unmount(
    removed: On<Remove, Card>,
    mut mounts: ResMut<CardMounts>,
) {
    mounts.gone.push(name(removed.entity));
}

/// Menus opened at the pointer.
pub trait MenuExt {
    /// Opens a menu of `items` at the pointer, over everything, each
    /// running what it was given when chosen. [`MenuShown`] holds
    /// while it is open.
    fn open_menu(&mut self, items: Vec<MenuItem>);
}

impl MenuExt for World {
    fn open_menu(&mut self, items: Vec<MenuItem>) {
        let (items, runs) = MenuItem::split(items);
        let mut ui = self.resource_mut::<Ui>();
        let key = ui.nodes.reserve();
        let chose = move |world: &mut World, at: u32| {
            world.resource_mut::<MenuShown>().0 = false;
            if let Some(run) = runs.get(at as usize) {
                run(world);
            }
        };
        let handlers = ui.handlers.entry(key).or_default();
        handlers.push(Handler::Chose(Box::new(chose)));
        handlers.push(Handler::Closed(Box::new(|world| {
            world.resource_mut::<MenuShown>().0 = false;
        })));
        self.resource_mut::<MenuShown>().0 = true;
        open_menu(key, &items);
    }
}

impl MenuExt for Commands<'_, '_> {
    fn open_menu(&mut self, items: Vec<MenuItem>) {
        self.queue(move |world: &mut World| world.open_menu(items));
    }
}

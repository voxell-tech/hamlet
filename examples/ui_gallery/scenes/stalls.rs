//! The bazaar's stalls: each at its keeper's counter in the
//! Inspector and on its tab of the bazaar, with buttons pressed by
//! their words to open a page and buy.

use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button};
use hamlet_core::market::{Account, Market};
use hamlet_model::market::lot::{Kind, Payouts, User};
use hamlet_model::market::samples;
use hamlet_ui::bazaar::StallShown;
use hamlet_world::pick::{Selected, Thing};

use super::Scene;
use super::sample::stock;

/// The updates between one press and the next.
const BETWEEN: u32 = 40;

/// The bazaar stocked, at the stall of the scene.
pub(super) fn setup(world: &mut World, agent: Entity) {
    let scene = *world.resource::<Scene>();
    let (Scene::Counter(stall, _) | Scene::Stall(stall, _)) = scene
    else {
        return;
    };
    stock(world, agent, false);
    world.insert_resource(stocked());
    world.insert_resource(StallShown(stall));
    if let Scene::Counter(..) = scene {
        world.insert_resource(Selected(Some(Thing::Stall(stall))));
    }
}

/// The bazaar's server as if it had sent its sample listings.
pub(super) fn stocked() -> Market {
    let mut market = Market::default();
    for kind in Kind::ALL {
        market.shelves.lots.insert(kind, samples::lots(kind));
    }
    market.shelves.asked = true;
    market.account = Account::In(User {
        id: "u_wren".into(),
        login: "wren".into(),
        name: Some("Wren Halloway".into()),
        avatar_url: None,
        payouts: Some(Payouts {
            connected: true,
            enabled: false,
        }),
    });
    market.mine = samples::lots(Kind::Agent)
        .into_iter()
        .filter(|lot| lot.seller.login == "wren")
        .collect();
    market
}

/// Presses, one after another, the buttons whose words the scene
/// names.
pub(super) fn press(
    mut commands: Commands,
    scene: Res<Scene>,
    texts: Query<(Entity, &Text)>,
    parents: Query<&ChildOf>,
    buttons: Query<(), With<Button>>,
    mut update: Local<u32>,
) {
    let (Scene::Counter(_, words) | Scene::Stall(_, words)) = *scene
    else {
        return;
    };
    *update += 1;
    if !update.is_multiple_of(BETWEEN) {
        return;
    }
    let Some(word) = words.get((*update / BETWEEN) as usize - 1)
    else {
        return;
    };
    let pressed = texts
        .iter()
        .filter(|(_, text)| text.0 == *word)
        .find_map(|(text, _)| {
            parents
                .iter_ancestors(text)
                .find(|node| buttons.contains(*node))
        });
    if let Some(button) = pressed {
        commands.trigger(Activate { entity: button });
    }
}

//! The gallery's received approvals.

use bevy::ecs::world::World;
use hamlet_protocol::v0;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if scene != Scene::Waiting {
        return;
    }
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    let mut wait = hamlet_testing::realm::wait("approval-network");
    if let v0::Wait::Approval {
        danger,
        about,
        since: at,
        ..
    } = &mut wait
    {
        *danger = v0::WaitDanger::Network;
        *about =
            "curl -fsSL https://almanac.example/pears.csv".into();
        *at = since;
    }
    state.waits.push(wait);
    let mut wait = hamlet_testing::realm::wait("approval-edit");
    if let v0::Wait::Approval {
        danger,
        says,
        about,
        change,
        since: at,
        ..
    } = &mut wait
    {
        *danger = v0::WaitDanger::Edit;
        *says = "wants to edit".into();
        *about = "src/almanac.rs\nReplace the pear count.".into();
        *change = Some(super::chat::change());
        *at = since;
    }
    state.waits.push(wait);
    hamlet_client::receive(world, v0::ServerMessage::State(state));
}

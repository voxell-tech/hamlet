use hamlet_engine_api::PluginLimits;
use hamlet_engine_core::DefaultLimits;
use hamlet_engine_core::tuning::Tuning;
use wasmtime::component::Resource;

use super::scene_bindings::hamlet::engine::scene::HostEntity;
use super::{Powers, SceneError};
use crate::host::WasmTuning;
use crate::instance::{Caps, State};

#[test]
fn entities_are_held_to_their_capability_and_budgets() {
    let budgets = PluginLimits {
        scene_entities: 2,
        scene_changes_per_call: 3,
        ..DefaultLimits::designed().0
    };
    let caps = Caps::of(&budgets, &WasmTuning::designed());
    let mut state = State::new(&caps);
    let make = |state: &mut State, parent: Option<u32>| {
        let parent = parent.map(Resource::new_borrow);
        HostEntity::make(state, parent, 0).expect("no trap")
    };

    let refused = make(&mut state, None).map(|made| made.rep());
    assert!(refused.unwrap_err().contains("render.scene"));
    state.scene.powers = Powers::SCENE;
    let first = make(&mut state, None).expect("room").rep();
    assert!(make(&mut state, Some(first)).is_ok());
    let full = make(&mut state, None).map(|made| made.rep());
    assert!(full.unwrap_err().contains("2 entities"));

    let hide = |state: &mut State| {
        HostEntity::set_visible(
            state,
            Resource::new_borrow(first),
            false,
        )
    };
    assert!(hide(&mut state).is_ok());
    let past = hide(&mut state).expect_err("past the call's changes");
    assert_eq!(
        past.downcast_ref::<SceneError>(),
        Some(&SceneError::Changes)
    );
}

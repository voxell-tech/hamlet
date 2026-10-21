use hamlet_engine_api::PluginLimits;
use hamlet_engine_core::DefaultLimits;
use hamlet_engine_core::tuning::Tuning;
use wasmtime::component::Resource;

use super::scene_bindings::hamlet::engine::scene::{
    AlphaMode, HostEntity, HostMaterial, HostMesh, Pbr, Shape,
};
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

#[test]
fn a_dropped_mesh_is_counted_while_an_entity_draws_it() {
    let caps = Caps::of(
        &DefaultLimits::designed().0,
        &WasmTuning::designed(),
    );
    let mut state = State::new(&caps);
    state.scene.powers = Powers::SCENE;
    let cube = |state: &mut State| {
        HostMesh::shape(state, Shape::Cuboid((1.0, 1.0, 1.0)))
            .expect("no trap")
    };
    let mesh = cube(&mut state).expect("room").rep();
    state.scene.caps.gpu_memory = state.scene.counts.gpu_bytes;
    let pbr = Pbr {
        base_color: (1.0, 1.0, 1.0, 1.0),
        emissive: (0.0, 0.0, 0.0, 1.0),
        roughness: 0.5,
        metallic: 0.0,
        alpha_mode: AlphaMode::Opaque,
        alpha_cutoff: 0.5,
        unlit: false,
        double_sided: false,
        fog: true,
    };
    let material = HostMaterial::make(&mut state, pbr)
        .expect("no trap")
        .expect("room")
        .rep();
    let entity = HostEntity::make(&mut state, None, 0)
        .expect("no trap")
        .expect("room")
        .rep();
    HostEntity::draw(
        &mut state,
        Resource::new_borrow(entity),
        Resource::new_borrow(mesh),
        Resource::new_borrow(material),
    )
    .expect("drawn");
    HostMesh::drop(&mut state, Resource::new_own(mesh))
        .expect("no trap");
    HostMaterial::drop(&mut state, Resource::new_own(material))
        .expect("no trap");

    assert!(cube(&mut state).unwrap_err().contains("GPU memory"));
    HostEntity::undraw(&mut state, Resource::new_borrow(entity))
        .expect("undrawn");
    assert!(cube(&mut state).is_ok());
    assert_eq!(state.scene.counts.materials, 0);
}

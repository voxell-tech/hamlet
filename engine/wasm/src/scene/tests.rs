use hamlet_engine_api::PluginLimits;
use hamlet_engine_core::Guards;
use hamlet_engine_core::tuning::Tuning;
use wasmtime::component::Resource;

use super::scene_bindings::hamlet::engine::scene::{
    AlphaMode, Decal, HostEntity, HostMaterial, HostMesh, Pbr, Shape,
};
use super::{Powers, SceneError};
use crate::host::WasmTuning;
use crate::instance::{Caps, State};

/// The caps of a plugin held to `limits`.
fn caps(limits: &PluginLimits) -> Caps {
    Caps::of(limits, &Guards::designed(), &WasmTuning::designed())
}

#[test]
fn entities_are_held_to_their_capability_and_the_limit_set() {
    let limits = PluginLimits {
        scene_entities: Some(2),
        ..PluginLimits::default()
    };
    let mut state = State::new(&caps(&limits));
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
}

#[test]
fn a_dropped_mesh_is_counted_while_an_entity_draws_it() {
    let mut state = State::new(&caps(&PluginLimits::default()));
    state.scene.powers = Powers::SCENE;
    let cube = |state: &mut State| {
        HostMesh::shape(state, Shape::Cuboid((1.0, 1.0, 1.0)))
            .expect("no trap")
    };
    let mesh = cube(&mut state).expect("room").rep();
    state.scene.caps.gpu_memory = Some(state.scene.counts.gpu_bytes);
    let pbr = Pbr {
        base_color: (1.0, 1.0, 1.0, 1.0),
        emissive: (0.0, 0.0, 0.0, 1.0),
        roughness: 0.5,
        metallic: 0.0,
        reflectance: 0.5,
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
    assert!(state.scene.gone.is_empty());
}

#[test]
fn a_decal_is_laid_only_with_a_decals_material() {
    let mut state = State::new(&caps(&PluginLimits::default()));
    state.scene.powers = Powers::SCENE;
    let decal = Decal {
        base_color: (1.0, 1.0, 1.0, 1.0),
        emissive: (0.0, 0.0, 0.0, 1.0),
        unlit: true,
        fog: false,
        depth_fade: 0.5,
    };
    let material = HostMaterial::decal(&mut state, decal)
        .expect("no trap")
        .expect("room")
        .rep();
    let mesh = HostMesh::shape(&mut state, Shape::Sphere(1.0))
        .expect("no trap")
        .expect("room")
        .rep();
    let entity = HostEntity::make(&mut state, None, 0)
        .expect("no trap")
        .expect("room")
        .rep();
    let at = || Resource::new_borrow(entity);

    let drawn = HostEntity::draw(
        &mut state,
        at(),
        Resource::new_borrow(mesh),
        Resource::new_borrow(material),
    );
    assert!(drawn.is_err());
    HostEntity::draw_decal(
        &mut state,
        at(),
        Resource::new_borrow(material),
    )
    .expect("laid");
    let flat = Decal {
        depth_fade: 0.0,
        ..decal
    };
    let set = HostMaterial::set_decal(
        &mut state,
        Resource::new_borrow(material),
        flat,
    );
    assert_eq!(
        set.expect_err("no depth to fade over")
            .downcast_ref::<SceneError>(),
        Some(&SceneError::Invalid)
    );
    HostMaterial::drop(&mut state, Resource::new_own(material))
        .expect("no trap");
    assert!(state.scene.gone.contains_key(&material));
}

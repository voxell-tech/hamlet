//! What each hamlet and agent wears, as the server keeps it, and
//! what each hamlet wears put on the houses of its town.

use bevy::ecs::system::SystemParam;
use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;
use hamlet_client::realm::{AgentNumber, HamletId, Outfit};
use hamlet_protocol::client::Looks as Outfitted;

use super::Sited;
use crate::dress::{Dressing, Looks, Worn};
use crate::generator::attire::{Attire, Attires, Shell};
use crate::wardrobe::Target;

/// Keeps [`Looks`] what every agent, by name, and every hamlet, by
/// id, wears, once the server tells of a change.
pub(super) fn gather(
    agents: Query<(&Name, &Outfit), With<AgentNumber>>,
    hamlets: Query<(&HamletId, &Outfit)>,
    mut looks: ResMut<Looks>,
) {
    let worn = |outfit: &Outfitted| Worn {
        skin: outfit.skin.clone(),
        mesh: outfit.mesh.clone(),
        emote: outfit.emote.clone(),
    };
    let by_agent = agents
        .iter()
        .filter(|(_, outfit)| !outfit.0.is_bare())
        .map(|(name, outfit)| (name.to_string(), worn(&outfit.0)))
        .collect();
    let by_hamlet = hamlets
        .iter()
        .filter(|(_, outfit)| !outfit.0.is_bare())
        .map(|(id, outfit)| (id.0.clone(), worn(&outfit.0)))
        .collect();
    if looks.agents != by_agent {
        looks.agents = by_agent;
    }
    if looks.hamlets != by_hamlet {
        looks.hamlets = by_hamlet;
    }
}

/// The assets a worn mesh is read from.
#[derive(SystemParam)]
pub(super) struct Gltfs<'w> {
    gltfs: Res<'w, Assets<Gltf>>,
    gltf_meshes: Res<'w, Assets<GltfMesh>>,
    nodes: Res<'w, Assets<GltfNode>>,
    meshes: Res<'w, Assets<Mesh>>,
    server: Res<'w, AssetServer>,
}

/// Puts on each town what its hamlet wears, once what is worn, the
/// hamlets or a worn mesh's loading changes it.
pub(super) fn wear(
    dressing: Res<Dressing>,
    hamlets: Query<(&HamletId, &Sited)>,
    moved: Query<(), Or<(Changed<HamletId>, Changed<Sited>)>>,
    mut gone: RemovedComponents<Sited>,
    gltfs: Gltfs,
    mut attires: ResMut<Attires>,
    // A worn mesh is still loading.
    mut loading: Local<bool>,
) {
    let gone = gone.read().count() > 0;
    if !(*loading
        || dressing.is_changed()
        || !moved.is_empty()
        || gone)
    {
        return;
    }
    *loading = false;
    // Only a change is written, so the towns are drawn again only
    // then.
    let mut put = |town, attire: Attire| {
        if !attires.wears(town, &attire) {
            attires.wear(town, attire);
        }
    };
    let mut kept = Vec::new();
    for (hamlet, sited) in &hamlets {
        kept.push(sited.0);
        let Some(look) = dressing.look(Target::Hamlet, &hamlet.0)
        else {
            put(sited.0, Attire::default());
            continue;
        };
        let mesh = look.mesh.as_ref().and_then(|gltf| {
            let shell = Shell::of(
                gltf,
                &gltfs.gltfs,
                &gltfs.gltf_meshes,
                &gltfs.nodes,
                &gltfs.meshes,
            );
            *loading |= shell.is_none()
                && !gltfs.server.is_loaded_with_dependencies(gltf)
                && !gltfs.server.load_state(gltf).is_failed();
            shell
        });
        put(
            sited.0,
            Attire {
                skin: look.skin.clone(),
                mesh,
                emote: look.emote.clone(),
            },
        );
    }
    let left = attires
        .towns()
        .filter(|town| !kept.contains(town))
        .collect::<Vec<_>>();
    for town in left {
        attires.wear(town, Attire::default());
    }
}

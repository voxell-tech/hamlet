//! What each hamlet wears, from the wardrobe by its name, put on
//! the houses of its town.

use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;
use hamlet_core::agents::Settlement;
use hamlet_core::wardrobe::Wardrobe;
use hamlet_model::wardrobe::{Emote, Target};

use super::Sited;
use crate::dress::{self, Dressing};
use crate::generator::attire::{Attire, Attires, Shell};

/// Puts on each town what its hamlet wears, once the wardrobe, the
/// hamlets or a worn mesh's loading changes it.
pub(super) fn wear(
    wardrobe: Option<Res<Wardrobe>>,
    dressing: Res<Dressing>,
    hamlets: Query<(&Settlement, &Sited)>,
    moved: Query<(), Or<(Changed<Settlement>, Changed<Sited>)>>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<Mesh>>,
    server: Res<AssetServer>,
    mut attires: ResMut<Attires>,
    // A worn mesh is still loading.
    mut loading: Local<bool>,
) {
    let Some(wardrobe) = wardrobe else {
        return;
    };
    if !(*loading
        || wardrobe.is_changed()
        || dressing.is_changed()
        || !moved.is_empty())
    {
        return;
    }
    *loading = false;
    let mut kept = Vec::new();
    for (hamlet, sited) in &hamlets {
        let look =
            dressing.look(&wardrobe, Target::Hamlet, &hamlet.name);
        let mesh = look.mesh.and_then(|(_, gltf)| {
            let shell = Shell::of(
                &gltf,
                &gltfs,
                &gltf_meshes,
                &nodes,
                &meshes,
            );
            *loading |= shell.is_none()
                && !dress::loaded(&gltf, &server)
                && !server.load_state(&gltf).is_failed();
            shell
        });
        let emote = look.emote.filter(|emote| {
            matches!(
                emote,
                Emote::Smoke(_)
                    | Emote::Lanterns(_)
                    | Emote::Banners(_)
            )
        });
        let attire = Attire {
            skin: look.skin.cloned(),
            mesh,
            emote: emote.cloned(),
        };
        if !attires.wears(sited.0, &attire) {
            attires.wear(sited.0, attire);
        }
        kept.push(sited.0);
    }
    let gone = attires
        .towns()
        .filter(|town| !kept.contains(town))
        .collect::<Vec<_>>();
    for town in gone {
        attires.wear(town, Attire::default());
    }
}

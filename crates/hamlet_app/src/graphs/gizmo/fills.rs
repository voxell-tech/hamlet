//! Filled shapes: a mesh drawn over the scene, for the gizmo and the
//! overlay.

use bevy::asset::{Asset, Assets, RenderAssetUsages};
use bevy::camera::visibility::{
    NoFrustumCulling, RenderLayers, Visibility,
};
use bevy::color::{Color, ColorToComponents};
use bevy::ecs::bundle::Bundle;
use bevy::ecs::change_detection::{DetectChangesMut, Mut};
use bevy::ecs::component::Component;
use bevy::ecs::system::{Commands, ResMut};
use bevy::light::NotShadowCaster;
use bevy::material::AlphaMode;
use bevy::math::Vec3;
use bevy::mesh::{
    Mesh, Mesh3d, MeshVertexBufferLayoutRef, PrimitiveTopology,
};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey,
    MaterialExtensionPipeline, MeshMaterial3d, StandardMaterial,
};
use bevy::picking::Pickable;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};

use super::super::preview::LAYER;
use super::Arc;

/// Sides of the cone that tips a translation handle.
const CONE_SIDES: usize = 12;

/// Sides of a disc.
const DISC_SIDES: usize = 16;

/// The material of filled shapes.
pub(in crate::graphs) type FillMaterial =
    ExtendedMaterial<StandardMaterial, OnTop>;

/// Extension of a [`StandardMaterial`] that draws over whatever is in
/// front of it.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub(in crate::graphs) struct OnTop {}

impl MaterialExtension for OnTop {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        // The theme's colours as they are, like the lines beside
        // them.
        if let Some(fragment) = &mut descriptor.fragment {
            fragment
                .shader_defs
                .retain(|def| *def != "TONEMAP_IN_SHADER".into());
        }
        Ok(())
    }
}

/// Marker component for the mesh of the gizmo's filled shapes.
#[derive(Component)]
pub(super) struct HandleFills;

pub(super) fn spawn_fills(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FillMaterial>>,
) {
    spawn_shape(
        &mut commands,
        &mut meshes,
        &mut materials,
        HandleFills,
    );
}

/// Spawns an entity for a mesh of filled shapes, marked by `marker`.
pub(in crate::graphs) fn spawn_shape(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FillMaterial>,
    marker: impl Bundle,
) {
    commands.spawn((
        marker,
        Mesh3d(meshes.add(Fills::default().mesh())),
        MeshMaterial3d(materials.add(FillMaterial {
            base: StandardMaterial {
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..StandardMaterial::default()
            },
            extension: OnTop {},
        })),
        RenderLayers::layer(LAYER),
        Visibility::Hidden,
        // Its triangles are in the world, wherever the subject is.
        NoFrustumCulling,
        NotShadowCaster,
        Pickable::IGNORE,
    ));
}

/// The triangles of filled shapes, in the world. Only the side a
/// triangle winds anticlockwise from is drawn.
#[derive(Default, PartialEq)]
pub(in crate::graphs) struct Fills {
    pub(super) positions: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
}

impl Fills {
    pub(super) fn triangle(
        &mut self,
        corners: [Vec3; 3],
        color: Color,
    ) {
        self.positions.extend(corners);
        self.colors.extend([color.to_linear().to_f32_array(); 3]);
    }

    pub(super) fn quad(
        &mut self,
        [a, b, c, d]: [Vec3; 4],
        color: Color,
    ) {
        self.triangle([a, b, c], color);
        self.triangle([a, c, d], color);
    }

    /// A flat fan from `hub` out to each pair of points along `rim`,
    /// seen from both sides.
    pub(in crate::graphs) fn fan(
        &mut self,
        hub: Vec3,
        rim: impl IntoIterator<Item = Vec3>,
        color: Color,
    ) {
        let rim = rim.into_iter().collect::<Vec<_>>();
        for ends in rim.windows(2) {
            self.triangle([hub, ends[0], ends[1]], color);
            self.triangle([hub, ends[1], ends[0]], color);
        }
    }

    /// A disc of `radius` around `centre`, square to `normal`.
    pub(in crate::graphs) fn disc(
        &mut self,
        centre: Vec3,
        normal: Vec3,
        radius: f32,
        color: Color,
    ) {
        self.fan(
            centre,
            Arc::full(normal).points(centre, radius, DISC_SIDES),
            color,
        );
    }

    /// A ribbon through `spine`, each of its points with the offset
    /// from it to one edge, seen from both sides.
    pub(in crate::graphs) fn ribbon(
        &mut self,
        spine: impl IntoIterator<Item = (Vec3, Vec3)>,
        color: Color,
    ) {
        let spine = spine.into_iter().collect::<Vec<_>>();
        for ends in spine.windows(2) {
            let [(a, from), (b, to)] = [ends[0], ends[1]];
            self.fan(a - from, [b - to, b + to, a + from], color);
        }
    }

    /// A cone from a base of `radius` around `base` up to `apex`.
    pub(super) fn cone(
        &mut self,
        base: Vec3,
        apex: Vec3,
        radius: f32,
        color: Color,
    ) {
        let Some(axis) = (apex - base).try_normalize() else {
            return;
        };
        let rim = Arc::full(axis)
            .points(base, radius, CONE_SIDES)
            .collect::<Vec<_>>();
        for ends in rim.windows(2) {
            self.triangle([apex, ends[0], ends[1]], color);
            self.triangle([base, ends[1], ends[0]], color);
        }
    }

    /// A box around `centre`, reaching each of `halves` either way.
    pub(super) fn cuboid(
        &mut self,
        centre: Vec3,
        halves: [Vec3; 3],
        color: Color,
    ) {
        for side in 0..3 {
            let out = halves[side];
            let across = halves[(side + 1) % 3];
            let up = halves[(side + 2) % 3];
            let face = |out: Vec3, across: Vec3| {
                [
                    centre + out - across - up,
                    centre + out + across - up,
                    centre + out + across + up,
                    centre + out - across + up,
                ]
            };
            self.quad(face(out, across), color);
            self.quad(face(-out, -across), color);
        }
    }

    /// Shows these as the mesh of `holder`, which is hidden when
    /// there is nothing to draw, and keeps them in `drawn`.
    pub(in crate::graphs) fn show(
        self,
        drawn: &mut Self,
        holder: &Mesh3d,
        mut visibility: Mut<Visibility>,
        meshes: &mut Assets<Mesh>,
    ) {
        let empty = self.positions.is_empty();
        visibility.set_if_neq(if empty {
            Visibility::Hidden
        } else {
            Visibility::Visible
        });
        if !empty
            && *drawn != self
            && let Some(mut mesh) = meshes.get_mut(&holder.0)
        {
            *mesh = self.mesh();
            *drawn = self;
        }
    }

    pub(super) fn mesh(&self) -> Mesh {
        let (mut positions, mut colors) =
            (self.positions.clone(), self.colors.clone());
        // One triangle of no size when there is nothing to draw: the
        // renderer has no room to give a mesh with no vertices, and
        // logs an error on copying into it.
        if positions.is_empty() {
            positions = vec![Vec3::ZERO; 3];
            colors = vec![[0.0; 4]; 3];
        }
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    }
}

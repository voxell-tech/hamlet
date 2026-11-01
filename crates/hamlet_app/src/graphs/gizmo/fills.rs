//! The gizmo's filled shapes: one mesh drawn over the scene.

use bevy::asset::{Asset, Assets, RenderAssetUsages};
use bevy::camera::visibility::{
    NoFrustumCulling, RenderLayers, Visibility,
};
use bevy::color::{Color, ColorToComponents};
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

/// The material of the gizmo's filled shapes.
pub(super) type FillMaterial =
    ExtendedMaterial<StandardMaterial, OnTop>;

/// Extension of a [`StandardMaterial`] that draws over whatever is in
/// front of it.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub(super) struct OnTop {}

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
    commands.spawn((
        HandleFills,
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

/// The triangles of the gizmo's filled shapes, in the world. Only
/// the side a triangle winds anticlockwise from is drawn.
#[derive(Default, PartialEq)]
pub(super) struct Fills {
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
    pub(super) fn fan(
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

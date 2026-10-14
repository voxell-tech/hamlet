//! Flags that wave: cloth hung from its hoist and blown along by a
//! vertex shader, with its shadow.

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::VisibilitySystems;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Face, ShaderType};
use bevy::shader::ShaderRef;
use bevy::window::RequestRedraw;

use crate::wind::{WindSettings, downwind};
use crate::{Now, VfxSystems};

pub type FlagMaterial = ExtendedMaterial<StandardMaterial, FlagExt>;

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "flag.wgsl");
    app.add_plugins(MaterialPlugin::<FlagMaterial>::default())
        .init_resource::<FlagSettings>()
        .init_resource::<Waving>()
        .add_systems(
            Update,
            wave.run_if(any_with_component::<Cloth>)
                .in_set(VfxSystems),
        )
        .add_systems(
            PostUpdate,
            keep_waving.run_if(any_with_component::<Cloth>).after(
                VisibilitySystems::MarkNewlyHiddenEntitiesInvisible,
            ),
        );
}

/// The waving of a flag.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagSettings {
    /// Speed of the waves along the cloth, the swing of its fly end
    /// as a share of its length, and the number of waves it holds.
    pub speed: f32,
    pub swing: f32,
    pub waves: f32,
    /// Stripes along the cloth, and how far the fly end is cut back
    /// at its top and bottom, as a share of its height.
    pub stripes: usize,
    pub taper: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct FlagExt {
    #[uniform(100)]
    pub flag: FlagUniform,
}

/// Mirrors `Flag` in `flag.wgsl`; the fields are explained there.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct FlagUniform {
    pub wave: Vec4,
    pub shape: Vec4,
}

impl MaterialExtension for FlagExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/flag.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/flag.wgsl".into()
    }
}

/// A flag's cloth.
#[derive(Component)]
struct Cloth;

/// The material the flags of each length share.
#[derive(Resource, Default)]
pub struct Waving(Vec<(f32, Handle<FlagMaterial>)>);

impl Waving {
    fn of(
        &mut self,
        materials: &mut Assets<FlagMaterial>,
        set: &FlagSettings,
        now: f32,
        long: f32,
    ) -> Handle<FlagMaterial> {
        if let Some((_, waving)) =
            self.0.iter().find(|(length, _)| *length == long)
        {
            return waving.clone();
        }
        let waving = materials.add(ExtendedMaterial {
            base: StandardMaterial {
                perceptual_roughness: 0.92,
                reflectance: 0.18,
                double_sided: true,
                cull_mode: None::<Face>,
                ..default()
            },
            extension: FlagExt {
                flag: uniform(set, now, long),
            },
        });
        self.0.push((long, waving.clone()));
        waving
    }
}

/// The cloth of a flag `long` by `tall`, hung by its top corner at
/// the origin and lying along x, in stripes of `colours` in turn.
/// The colours are linear RGB.
pub fn cloth(
    set: &FlagSettings,
    long: f32,
    tall: f32,
    colours: &[Vec3],
) -> Mesh {
    /// Columns of points to a stripe, so that a stripe bends.
    const FINE: usize = 4;
    let stripes = set.stripes.max(1);
    let (mut at, mut uv, mut tint) =
        (Vec::new(), Vec::new(), Vec::new());
    let mut indices = Vec::new();
    for stripe in 0..stripes {
        let colour = colours
            .get(stripe % colours.len().max(1))
            .copied()
            .unwrap_or(Vec3::ONE);
        for column in 0..=FINE {
            let u = (stripe as f32 + column as f32 / FINE as f32)
                / stripes as f32;
            // Only the last stripe is cut back.
            let cut = set.taper
                * 0.5
                * ((u * stripes as f32) - (stripes - 1) as f32)
                    .max(0.0);
            for v in [cut, 1.0 - cut] {
                at.push([u * long, -v * tall, 0.0]);
                uv.push([u, v]);
                tint.push(colour.extend(1.0).to_array());
            }
        }
        let first = (stripe * (FINE + 1) * 2) as u32;
        for column in 0..FINE as u32 {
            let a = first + column * 2;
            indices.extend([a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    let normals = vec![[0.0, 0.0, 1.0]; at.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, at)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, tint)
    .with_inserted_indices(Indices::U32(indices))
}

fn uniform(set: &FlagSettings, now: f32, long: f32) -> FlagUniform {
    FlagUniform {
        wave: Vec4::new(now, set.speed, set.swing, set.waves),
        shape: Vec4::new(long, 0.0, 0.0, 0.0),
    }
}

/// The bundle of a flag whose cloth is `mesh`, `long` from hoist to
/// fly. Its own transform puts the hoist's top where it hangs.
pub fn flag(
    waving: &mut Waving,
    materials: &mut Assets<FlagMaterial>,
    set: &FlagSettings,
    now: f32,
    mesh: Handle<Mesh>,
    long: f32,
) -> impl Bundle + use<> {
    (
        Cloth,
        Mesh3d(mesh),
        MeshMaterial3d(waving.of(materials, set, now, long)),
        Visibility::Inherited,
    )
}

/// Tells the flags the time, while it moves, and turns each down
/// the wind whichever way what it hangs from faces.
fn wave(
    now: Res<Now>,
    set: Res<FlagSettings>,
    wind: Res<WindSettings>,
    waving: Res<Waving>,
    mut flags: Query<(&mut Transform, Option<&ChildOf>), With<Cloth>>,
    masts: Query<&GlobalTransform>,
    mut materials: ResMut<Assets<FlagMaterial>>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(now.0) == Some(now.0) && !set.is_changed() {
        return;
    }
    for (long, waving) in &waving.0 {
        let next = uniform(&set, now.0, *long);
        if let Some(mut waving) = materials.get_mut(waving)
            && waving.extension.flag != next
        {
            waving.extension.flag = next;
        }
    }
    let blown = downwind(wind.blowing(now.0));
    for (mut at, mast) in &mut flags {
        let mast = mast
            .and_then(|mast| masts.get(mast.parent()).ok())
            .map_or(Quat::IDENTITY, |mast| mast.rotation());
        let turned = mast.inverse() * blown;
        if at.rotation != turned {
            at.rotation = turned;
        }
    }
}

/// Keeps frames coming while a flag that waves is in view.
fn keep_waving(
    now: Res<Now>,
    set: Res<FlagSettings>,
    flags: Query<&ViewVisibility, With<Cloth>>,
    mut told: Local<Option<f32>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if told.replace(now.0) == Some(now.0) && !set.is_changed() {
        return;
    }
    if flags.iter().any(|seen| seen.get()) {
        redraw.write(RequestRedraw);
    }
}

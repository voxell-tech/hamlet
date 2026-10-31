//! The foam on the water, kept from frame to frame. Each frame
//! `foam.wesl` makes two maps of it about the eye from the last
//! frame's: a near one, fine, and a far one, coarse, over twice the
//! ground. The water draws the newest of each. Two textures take turns
//! for each map, one read while the other is made.
//!
//! Ported from an MIT-licensed water shader; its notice is in
//! `water/LICENSE`.

use bevy::asset::{
    RenderAssetUsages, embedded_asset, load_embedded_asset,
};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::extract_resource::{
    ExtractResource, ExtractResourcePlugin,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, CachedComputePipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor, Extent3d,
    PipelineCache, SamplerBindingType, ShaderStages, ShaderType,
    StorageTextureAccess, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages, UniformBuffer,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
    RenderQueue,
};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::water::{WaterMaterial, WaterSettings, Waves, flow};
use crate::{Now, VfxSystems};

/// Texels along the side of the near map and the far one.
const SIDES: [u32; 2] = [1024, 512];

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "foam.wesl");
    app.add_plugins(ExtractResourcePlugin::<FoamMaps>::default())
        .add_systems(Startup, make)
        .add_systems(Update, follow.after(flow).in_set(VfxSystems));
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Render,
            bind.in_set(RenderSystems::PrepareBindGroups),
        )
        // Before the cameras draw, so the water draws this frame's maps.
        .add_systems(RenderGraph, run.in_set(RenderGraphSystems::Begin));
}

/// Mirrors `Foam` in `foam.wesl`.
#[derive(ShaderType, Clone, Default)]
struct FoamUniform {
    /// The map's middle on the world's map, the metres across it, and
    /// the seconds since the last map was made.
    middle: Vec4,
    /// How far the map has moved since, as a share of its side, over
    /// x and z; the time; and [`WaterSettings::scale`].
    shift: Vec4,
    /// The foam's `decay`, `erosion` and `streak`.
    fade: Vec4,
    /// The corner of the shore's field on the map, and its size.
    field: Vec4,
    /// [`WaterSettings::bend`] and `calm`.
    bend: Vec4,
    /// The way the wind blows, as the water has it.
    wind: Vec4,
}

/// One map: its two textures, and what this frame's is made from.
#[derive(Clone)]
struct Map {
    textures: [Handle<Image>; 2],
    foam: FoamUniform,
}

/// The near map and the far one.
#[derive(Resource, ExtractResource, Clone)]
#[extract_app(RenderApp)]
struct FoamMaps {
    maps: [Map; 2],
    /// Which texture of each map this frame's is made in. The other
    /// holds the last frame's.
    next: usize,
    /// The noise foam is born and eaten by.
    noise: Handle<Image>,
    /// The field of the distance to the shore, which the waves turn
    /// to.
    shore: Handle<Image>,
    /// Whether the maps are made this frame: not while the clock
    /// stands.
    live: bool,
}

fn make(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut texture = |side: u32| {
        let mut image = Image::new_fill(
            Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0; 8],
            TextureFormat::Rgba16Float,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage |=
            TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
        image.sampler =
            ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
        images.add(image)
    };
    let maps = SIDES.map(|side| Map {
        textures: [texture(side), texture(side)],
        foam: FoamUniform::default(),
    });
    commands.insert_resource(FoamMaps {
        maps,
        next: 0,
        noise: Handle::default(),
        shore: Handle::default(),
        live: false,
    });
}

/// Lays the next maps about the sea, which keeps under the eye, and
/// has the water draw them.
fn follow(
    now: Res<Now>,
    set: Res<WaterSettings>,
    mut maps: ResMut<FoamMaps>,
    seas: Query<(&GlobalTransform, &MeshMaterial3d<WaterMaterial>), With<Waves>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut last: Local<Option<(f32, Vec2)>>,
) {
    maps.live = false;
    let Some((at, material)) = seas.iter().next() else {
        return;
    };
    let middle = at.translation().xz().round();
    if *last == Some((now.0, middle)) {
        return;
    }
    let Some(mut water) = materials.get_mut(&material.0) else {
        return;
    };
    let (seconds, moved) = match *last {
        Some((then, was)) if now.0 >= then => {
            (now.0 - then, middle - was)
        }
        _ => (0.0, Vec2::ZERO),
    };
    *last = Some((now.0, middle));
    let next = 1 - maps.next;
    maps.next = next;
    maps.noise = water.extension.noise.clone();
    maps.shore = water.extension.shore.clone();
    let field = water.extension.water.field;
    let wind = water.extension.water.wind;
    let coverage = [set.foam.near, set.foam.far];
    for (map, coverage) in maps.maps.iter_mut().zip(coverage) {
        let shift = moved / coverage;
        map.foam = FoamUniform {
            middle: Vec4::new(middle.x, middle.y, coverage, seconds),
            shift: Vec4::new(shift.x, shift.y, now.0, set.scale),
            fade: Vec4::new(
                set.foam.decay,
                set.foam.erosion,
                set.foam.streak.0,
                set.foam.streak.1,
            ),
            field,
            bend: Vec4::new(set.bend, set.calm.0, set.calm.1, 0.0),
            wind,
        };
    }
    maps.live = true;
    let water = &mut water.extension;
    water.foam_near = maps.maps[0].textures[next].clone();
    water.foam_far = maps.maps[1].textures[next].clone();
    water.water.foam =
        Vec4::new(middle.x, middle.y, coverage[0], coverage[1]);
}

/// The pass, made once.
#[derive(Resource)]
struct Pass {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
}

fn prepare(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "foam",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<FoamUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_storage_2d(
                    TextureFormat::Rgba16Float,
                    StorageTextureAccess::WriteOnly,
                ),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let pipeline =
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("foam".into()),
            layout: vec![layout.clone()],
            shader: load_embedded_asset!(&*assets, "foam.wesl"),
            ..default()
        });
    commands.insert_resource(Pass { layout, pipeline });
}

/// This frame's bind groups, each with the side of its map, when the
/// maps are to be made.
#[derive(Resource)]
struct Bound(Vec<(BindGroup, u32)>);

fn bind(
    mut commands: Commands,
    maps: Option<Res<FoamMaps>>,
    pass: Res<Pass>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    mut buffers: Local<[UniformBuffer<FoamUniform>; 2]>,
) {
    let mut groups = Vec::new();
    if let Some(maps) = maps.filter(|maps| maps.live)
        && let Some(noise) = images.get(&maps.noise)
        && let Some(shore) = images.get(&maps.shore)
    {
        for ((map, buffer), side) in
            maps.maps.iter().zip(buffers.iter_mut()).zip(SIDES)
        {
            let (Some(last), Some(next)) = (
                images.get(&map.textures[1 - maps.next]),
                images.get(&map.textures[maps.next]),
            ) else {
                continue;
            };
            buffer.set(map.foam.clone());
            buffer.write_buffer(&device, &queue);
            let Some(foam) = buffer.binding() else {
                continue;
            };
            let group = device.create_bind_group(
                Some("foam"),
                &cache.get_bind_group_layout(&pass.layout),
                &BindGroupEntries::sequential((
                    foam,
                    &noise.texture_view,
                    &noise.sampler,
                    &last.texture_view,
                    &last.sampler,
                    &next.texture_view,
                    &shore.texture_view,
                    &shore.sampler,
                )),
            );
            groups.push((group, side));
        }
    }
    commands.insert_resource(Bound(groups));
}

/// Makes this frame's maps.
fn run(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pass: Res<Pass>,
    bound: Option<Res<Bound>>,
) {
    let Some(bound) = bound.filter(|bound| !bound.0.is_empty()) else {
        return;
    };
    let Some(pipeline) = cache.get_compute_pipeline(pass.pipeline)
    else {
        return;
    };
    let mut compute = context.command_encoder().begin_compute_pass(
        &ComputePassDescriptor {
            label: Some("foam"),
            ..default()
        },
    );
    compute.set_pipeline(pipeline);
    for (group, side) in &bound.0 {
        compute.set_bind_group(0, group, &[]);
        compute.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), 1);
    }
}

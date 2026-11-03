//! The land's pages on the GPU: for each size of patch the ground is
//! drawn in, a page of the land about the eye, one texel to a point
//! of that size's grid, made by `relief.wesl` with the same heights
//! as [`LandData::height_wide`]. Each texel holds the height, the
//! share of woods and the share of heath there. A page is made anew
//! only when the eye has moved far enough over it, or the land has
//! changed; the ground's shader reads the pages to raise its patches.
//!
//! [`LandData::height_wide`]: crate::generator::land::LandData::height_wide

use bevy::asset::{
    RenderAssetUsages, embedded_asset, load_embedded_asset,
};
use bevy::prelude::*;
use bevy::render::extract_resource::{
    ExtractResource, ExtractResourcePlugin,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::encase::StorageBuffer;
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, CachedComputePipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor, Extent3d,
    PipelineCache, ShaderStages, ShaderType, StorageTextureAccess,
    TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, TextureViewDimension, UniformBuffer,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
    RenderQueue,
};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::texture::GpuImage;
use bevy::render::{RenderApp, RenderStartup};
use hamlet_vfx::grass::GrassPlaceSystems;
use hamlet_vfx::ground::{GroundMaterial, LEVELS, PagesUniform};
use hamlet_vfx::patch;
use hamlet_vfx::water::WaterMaterial;

use super::land::Land;
use super::{DetailSettings, Kit};
use crate::WorldCamera;

/// Texels along the side of a page. A patch is drawn within
/// [`patch::SPLIT`] times twice its side, and its diagonal, of the
/// eye, about nine of its sides, and its size's texel is its side
/// over [`patch::QUADS`]: so a page reaches about 285 texels each
/// way, and [`STEP`] more while the eye moves over it.
const SIDE: u32 = 704;

/// Texels the eye moves over a page before it is made anew about it.
const STEP: f32 = 32.0;

pub(super) fn plugin(app: &mut App) {
    embedded_asset!(app, "relief.wesl");
    app.add_plugins(ExtractResourcePlugin::<Job>::default())
        .add_systems(Update, follow.in_set(PagesLaid));
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .add_systems(RenderStartup, prepare)
        // Before the cameras draw and the grass is placed, so both
        // read this frame's pages.
        .add_systems(
            RenderGraph,
            run.in_set(RenderGraphSystems::Begin)
                .before(GrassPlaceSystems),
        );
}

/// What the pass reads, as `Relief` in `relief.wesl`.
#[derive(ShaderType, Clone, Default)]
struct ReliefInput {
    /// `plain_y`, `hills` and the scale of the `grain`.
    plains: Vec4,
    /// The height of the `grain`, the reach of the `plains` and the
    /// `edge` of them.
    rise: Vec4,
    /// `mounds`, `mound_length` and `ragged`.
    mounds: Vec4,
    /// `mound_height` and the `island`'s reach.
    tops: Vec4,
    /// `mound_size` and `mound_skirt`.
    sizes: Vec4,
    /// `coast`, `sea_floor` and `water_y`.
    sea: Vec4,
    /// `shelf` and `rim`.
    rim: Vec4,
    /// `site_y`, the towns' radius, and the way the rivers run.
    town: Vec4,
    /// `river_gap`, `bends` and the scale of the `kinks`.
    river: Vec4,
    /// The reach of the `kinks`, `drift_scale` and `river_drift`.
    drift: Vec4,
    /// `wilds` and `heath`.
    wilds: Vec4,
    /// The seed, and how many sites, shores and held rounds there
    /// are in `points`.
    counts: UVec4,
    #[shader(size(runtime))]
    points: Vec<Vec4>,
}

impl ReliefInput {
    fn of(land: &Land) -> Self {
        let set = &land.set.land;
        let dir = land.river_dir();
        let point = |at: Vec2| Vec4::new(at.x, at.y, 0.0, 0.0);
        let sites = land
            .sites
            .iter()
            .map(|s| point(s.at))
            .collect::<Vec<_>>();
        let shores = land.shores().map(point).collect::<Vec<_>>();
        let holds = land.hold_levels().collect::<Vec<_>>();
        let counts = UVec4::new(
            land.seed,
            sites.len() as u32,
            shores.len() as u32,
            holds.len() as u32,
        );
        let points = [sites, shores, holds].concat();
        Self {
            plains: Vec4::new(
                set.plain_y,
                set.hills.0,
                set.hills.1,
                set.grain.0,
            ),
            rise: Vec4::new(
                set.grain.1,
                set.plains,
                set.edge.0,
                set.edge.1,
            ),
            mounds: Vec4::new(
                set.mounds.0,
                set.mounds.1,
                set.mound_length,
                set.ragged,
            ),
            tops: Vec4::new(
                set.mound_height.0,
                set.mound_height.1,
                set.island,
                0.0,
            ),
            sizes: Vec4::new(
                set.mound_size.0,
                set.mound_size.1,
                set.mound_skirt.0,
                set.mound_skirt.1,
            ),
            sea: Vec4::new(
                set.coast.0,
                set.coast.1,
                set.sea_floor,
                set.water_y,
            ),
            rim: Vec4::new(set.shelf, set.rim.0, set.rim.1, 0.0),
            town: Vec4::new(
                set.site_y,
                land.set.town.radius,
                dir.x,
                dir.y,
            ),
            river: Vec4::new(
                set.river_gap,
                set.bends.0,
                set.bends.1,
                set.kinks.0,
            ),
            drift: Vec4::new(
                set.kinks.1,
                set.drift_scale,
                set.river_drift,
                0.0,
            ),
            wilds: Vec4::new(set.wilds, set.heath, 0.0, 0.0),
            counts,
            points,
        }
    }
}

/// Mirrors `Level` in `relief.wesl`: the corner of a page on the map,
/// the width of its texel, and how much wider its rivers are drawn,
/// so that a page too coarse to hold a river's bed does not break it
/// into pieces.
#[derive(ShaderType, Clone, Copy, Default)]
struct LevelInput {
    page: Vec4,
}

/// The pages made this frame: the land they are made from, the
/// texture they are made in, and the page of each size made anew.
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(RenderApp)]
struct Job {
    input: Handle<ShaderBuffer>,
    pages: Handle<Image>,
    /// Each size made anew, with its page's corner and texel.
    made: Vec<(u32, LevelInput)>,
}

/// The texture the pages are in, one layer for each size, and the
/// corner each was last made about.
#[derive(Resource)]
struct Pages {
    image: Handle<Image>,
    corners: [Option<Vec2>; LEVELS],
}

/// The width of a texel of the pages of size `level`, for a land
/// whose finest grid is `cell` wide.
fn texel(cell: f32, level: usize) -> f32 {
    cell * (1u32 << level) as f32
}

/// Laying the pages and telling the ground and the water where they
/// lie. What reads where they lie from either runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PagesLaid;

/// Lays the pages about the eye, makes anew those it has moved far
/// enough over and all of them when the land changes, and tells the
/// ground and the water where they lie.
fn follow(
    mut commands: Commands,
    land: Res<Land>,
    detail: Res<DetailSettings>,
    kit: Option<Res<Kit>>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    images: Option<ResMut<Assets<Image>>>,
    buffers: Option<ResMut<Assets<ShaderBuffer>>>,
    (mut grounds, mut waters): (
        ResMut<Assets<GroundMaterial>>,
        ResMut<Assets<WaterMaterial>>,
    ),
    pages: Option<ResMut<Pages>>,
    job: Option<Res<Job>>,
) {
    let (Some(kit), Some(mut images), Some(mut buffers), Some(eye)) =
        (kit, images, buffers, camera.iter().next())
    else {
        return;
    };
    let Some(mut pages) = pages else {
        commands.insert_resource(Pages {
            image: images.add(texture()),
            corners: [None; LEVELS],
        });
        return;
    };
    let cell = land.set.land.cell;
    let renew = land.is_changed() || job.is_none();
    let mut made = Vec::new();
    for level in 0..LEVELS {
        let texel = texel(cell, level);
        let step = texel * STEP;
        let corner = (eye.translation().xz() / step).round() * step
            - SIDE as f32 * 0.5 * texel;
        if renew || pages.corners[level] != Some(corner) {
            pages.corners[level] = Some(corner);
            made.push((
                level as u32,
                LevelInput {
                    page: Vec4::new(
                        corner.x,
                        corner.y,
                        texel,
                        ((texel - 3.0) * 0.5).max(0.0),
                    ),
                },
            ));
        }
    }
    let input = match &job {
        Some(job) if !renew => job.input.clone(),
        _ => {
            let mut bytes = StorageBuffer::new(Vec::<u8>::new());
            if bytes.write(&ReliefInput::of(&land)).is_err() {
                return;
            }
            let mut input = ShaderBuffer::from(bytes.into_inner());
            input.asset_usage = RenderAssetUsages::RENDER_WORLD;
            buffers.add(input)
        }
    };
    if renew {
        info!("relief: pages made anew for the land");
    }
    let at = std::array::from_fn(|level| {
        let corner = pages.corners[level].unwrap_or_default();
        Vec4::new(corner.x, corner.y, texel(cell, level), 0.0)
    });
    let next = PagesUniform {
        at,
        shape: Vec4::new(
            cell * patch::QUADS as f32,
            land.set.land.plain_y,
            detail.tile_sink.0,
            detail.tile_sink.1,
        ),
        water: Vec4::new(land.set.land.water_y, 0.0, 0.0, 0.0),
    };
    if let Some(mut ground) = grounds.get_mut(&kit.ground) {
        if ground.extension.pages_at != next {
            ground.extension.pages_at = next.clone();
        }
        if ground.extension.pages != pages.image {
            ground.extension.pages = pages.image.clone();
        }
    }
    // The water finds the depth of the ground under it from them too.
    if let Some(mut water) = waters.get_mut(&kit.water) {
        if water.extension.pages_at != next {
            water.extension.pages_at = next;
        }
        if water.extension.pages != pages.image {
            water.extension.pages = pages.image.clone();
        }
    }
    commands.insert_resource(Job {
        input,
        pages: pages.image.clone(),
        made,
    });
}

/// The pages' texture: a layer for each size, each texel the height's
/// bits and the woods and the heath as two shares of 16 bits, written
/// by the pass and read by the ground's shader.
fn texture() -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: LEVELS as u32,
        },
        TextureDimension::D2,
        TextureFormat::Rg32Uint,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING
        | TextureUsages::TEXTURE_BINDING;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image
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
        "relief",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<ReliefInput>(false),
                uniform_buffer::<LevelInput>(false),
                texture_storage_2d(
                    TextureFormat::Rg32Uint,
                    StorageTextureAccess::WriteOnly,
                ),
            ),
        ),
    );
    let pipeline =
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("relief".into()),
            layout: vec![layout.clone()],
            shader: load_embedded_asset!(&*assets, "relief.wesl"),
            ..default()
        });
    commands.insert_resource(Pass { layout, pipeline });
}

/// The pages asked for and not yet made: kept until the pass and the
/// land it is made from are ready, so that none asked for before then
/// is lost.
#[derive(Default)]
struct Waiting([Option<LevelInput>; LEVELS]);

/// Makes the pages asked for, this frame's and any still waiting.
fn run(
    mut context: RenderContext,
    job: Option<Res<Job>>,
    mut waiting: Local<Waiting>,
    mut uniforms: Local<Vec<UniformBuffer<LevelInput>>>,
    pass: Res<Pass>,
    (device, queue, cache): (
        Res<RenderDevice>,
        Res<RenderQueue>,
        Res<PipelineCache>,
    ),
    (buffers, images): (
        Res<RenderAssets<GpuShaderBuffer>>,
        Res<RenderAssets<GpuImage>>,
    ),
) {
    let Some(job) = job else {
        return;
    };
    for &(level, at) in &job.made {
        waiting.0[level as usize] = Some(at);
    }
    let (Some(pipeline), Some(input), Some(pages)) = (
        cache.get_compute_pipeline(pass.pipeline),
        buffers.get(&job.input),
        images.get(&job.pages),
    ) else {
        return;
    };
    if waiting.0.iter().all(Option::is_none) {
        return;
    }
    uniforms.resize_with(LEVELS, UniformBuffer::default);
    let layout = cache.get_bind_group_layout(&pass.layout);
    let mut compute = context.command_encoder().begin_compute_pass(
        &ComputePassDescriptor {
            label: Some("relief"),
            ..default()
        },
    );
    compute.set_pipeline(pipeline);
    for (level, at) in waiting.0.iter_mut().enumerate() {
        let Some(at) = at.take() else {
            continue;
        };
        let uniform = &mut uniforms[level];
        uniform.set(at);
        uniform.write_buffer(&device, &queue);
        let Some(at) = uniform.binding() else {
            continue;
        };
        let layer =
            pages.texture.create_view(&TextureViewDescriptor {
                dimension: Some(TextureViewDimension::D2),
                base_array_layer: level as u32,
                array_layer_count: Some(1),
                ..default()
            });
        let group = device.create_bind_group(
            Some("relief"),
            &layout,
            &BindGroupEntries::sequential((
                input.buffer.as_entire_buffer_binding(),
                at,
                &layer,
            )),
        );
        compute.set_bind_group(0, &group, &[]);
        compute.dispatch_workgroups(
            SIDE.div_ceil(8),
            SIDE.div_ceil(8),
            1,
        );
    }
}

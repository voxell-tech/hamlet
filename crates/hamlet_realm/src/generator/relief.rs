//! The land's heights made on the GPU: a page of them, one per texel,
//! by `relief.wesl`, and read back for the CPU. For now it is a trial
//! beside [`Land::height`], which it is checked against: it logs how
//! long the page takes to come back and how far it is from the CPU's
//! heights. It is to become the land's one source of heights
//! (`hamlet_backlog/terrain.md`).

use std::time::Instant;

use bevy::asset::{
    RenderAssetUsages, embedded_asset, load_embedded_asset,
};
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::extract_resource::{
    ExtractResource, ExtractResourcePlugin,
};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only, texture_storage_2d,
};
use bevy::render::render_resource::encase::StorageBuffer;
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, CachedComputePipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor, Extent3d,
    PipelineCache, ShaderStages, ShaderType, StorageTextureAccess,
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph,
};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use super::land::Land;

/// Texels along the side of a page.
const SIDE: u32 = 512;
/// World units across one texel.
const TEXEL: f32 = 0.5;
/// Heights compared with the CPU's when a page comes back.
const CHECKS: u32 = 1000;

pub(super) fn plugin(app: &mut App) {
    embedded_asset!(app, "relief.wesl");
    app.add_plugins(ExtractResourcePlugin::<Job>::default())
        .add_systems(Update, ask.run_if(resource_changed::<Land>));
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Render,
            bind.in_set(RenderSystems::PrepareBindGroups),
        )
        .add_systems(RenderGraph, run);
}

/// What the pass reads, as `Relief` in `relief.wesl`.
#[derive(ShaderType, Clone, Default)]
struct ReliefInput {
    /// The page's corner on the map, the width of a texel, and how
    /// much wider the rivers are drawn.
    page: Vec4,
    /// `plain_y`, `hills` and the scale of the `grain`.
    plains: Vec4,
    /// The height of the `grain`, the reach of the `plains` and the
    /// `edge` of them.
    rise: Vec4,
    /// `mounds`.
    mounds: Vec4,
    /// `mound_height`, the `island`'s reach and the `shore`.
    tops: Vec4,
    /// `coast`, `sea_floor` and `water_y`.
    sea: Vec4,
    /// `site_y`, the towns' radius, and the way the rivers run.
    town: Vec4,
    /// `river_gap`, `bends` and the scale of the `kinks`.
    river: Vec4,
    /// The reach of the `kinks`, `drift_scale` and `river_drift`.
    drift: Vec4,
    /// The seed, and how many sites, shores and held rounds there
    /// are in `points`.
    counts: UVec4,
    #[shader(size(runtime))]
    points: Vec<Vec4>,
}

impl ReliefInput {
    fn of(land: &Land, corner: Vec2) -> Self {
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
            page: Vec4::new(corner.x, corner.y, TEXEL, 0.0),
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
                set.mounds.2,
                0.0,
            ),
            tops: Vec4::new(
                set.mound_height.0,
                set.mound_height.1,
                set.island,
                set.shore,
            ),
            sea: Vec4::new(
                set.coast.0,
                set.coast.1,
                set.sea_floor,
                set.water_y,
            ),
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
            counts,
            points,
        }
    }
}

/// The page asked for: its input and the texture it is drawn into,
/// both new for each, and a number that tells one from the next.
#[derive(Resource, ExtractResource, Clone)]
#[extract_app(RenderApp)]
struct Job {
    input: Handle<ShaderBuffer>,
    heights: Handle<Image>,
    number: u32,
    corner: Vec2,
    asked: (Instant, u32),
}

/// Asks for the page about the middle anew whenever the land changes.
fn ask(
    mut commands: Commands,
    land: Res<Land>,
    frame: Res<FrameCount>,
    images: Option<ResMut<Assets<Image>>>,
    buffers: Option<ResMut<Assets<ShaderBuffer>>>,
    job: Option<Res<Job>>,
    readers: Query<Entity, With<Readback>>,
) {
    let (Some(mut images), Some(mut buffers)) = (images, buffers)
    else {
        return;
    };
    for reader in &readers {
        commands.entity(reader).despawn();
    }
    let corner = Vec2::splat(-(SIDE as f32) * TEXEL * 0.5);
    let mut bytes = StorageBuffer::new(Vec::<u8>::new());
    if bytes.write(&ReliefInput::of(&land, corner)).is_err() {
        return;
    }
    let mut input = ShaderBuffer::from(bytes.into_inner());
    input.asset_usage = RenderAssetUsages::RENDER_WORLD;
    // Filled with NaN, so that a page read back before the pass has
    // run is told apart.
    let mut heights = Image::new_fill(
        Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &f32::NAN.to_le_bytes(),
        TextureFormat::R32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    heights.texture_descriptor.usage |= TextureUsages::COPY_SRC
        | TextureUsages::STORAGE_BINDING
        | TextureUsages::TEXTURE_BINDING;
    let heights = images.add(heights);
    let number = job.map_or(0, |job| job.number + 1);
    commands.insert_resource(Job {
        input: buffers.add(input),
        heights: heights.clone(),
        number,
        corner,
        asked: (Instant::now(), frame.0),
    });
    commands
        .spawn((Readback::texture(heights), Page(number)))
        .observe(check);
}

/// The number of the page a readback is of.
#[derive(Component)]
struct Page(u32);

/// Takes a page that came back, once the pass has run on it: logs how
/// long it took and how far it lies from the CPU's heights.
fn check(
    back: On<ReadbackComplete>,
    mut commands: Commands,
    land: Res<Land>,
    job: Res<Job>,
    frame: Res<FrameCount>,
    pages: Query<&Page>,
) {
    // A page asked for before the land last changed is let go.
    if pages
        .get(back.entity)
        .is_ok_and(|page| page.0 != job.number)
    {
        return;
    }
    let heights = back
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect::<Vec<_>>();
    if heights.first().is_none_or(|h| h.is_nan()) {
        return;
    }
    commands.entity(back.entity).despawn();
    let (since, from) = job.asked;
    let (mut worst, mut sum) = (0f32, 0f32);
    let cpu = Instant::now();
    for k in 0..CHECKS {
        let (x, y) = (k * 7919 % SIDE, k * 104_729 % SIDE);
        let p = job.corner
            + (Vec2::new(x as f32, y as f32) + 0.5) * TEXEL;
        let off =
            (heights[(y * SIDE + x) as usize] - land.height(p)).abs();
        worst = worst.max(off);
        sum += off;
    }
    let cpu = cpu.elapsed().as_secs_f64() / CHECKS as f64
        * f64::from(SIDE * SIDE);
    let (low, high) =
        heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &h| {
            (lo.min(h), hi.max(h))
        });
    info!(
        "relief: page {} back after {} frames, {:.1} ms; heights \
         from {low:.2} to {high:.2}; off the CPU by {:.6} on \
         average and {:.6} at most; the CPU would take {:.0} ms for \
         the page",
        job.number,
        frame.0.wrapping_sub(from),
        since.elapsed().as_secs_f64() * 1e3,
        sum / CHECKS as f32,
        worst,
        cpu * 1e3,
    );
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
                texture_storage_2d(
                    TextureFormat::R32Float,
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

/// The bind group of the page asked for, and its number.
#[derive(Resource)]
struct Bound(BindGroup, u32);

fn bind(
    mut commands: Commands,
    job: Option<Res<Job>>,
    bound: Option<Res<Bound>>,
    pass: Res<Pass>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    images: Res<RenderAssets<GpuImage>>,
) {
    let Some(job) = job else {
        return;
    };
    if bound.is_some_and(|bound| bound.1 == job.number) {
        return;
    }
    let (Some(input), Some(heights)) =
        (buffers.get(&job.input), images.get(&job.heights))
    else {
        return;
    };
    let group = device.create_bind_group(
        Some("relief"),
        &cache.get_bind_group_layout(&pass.layout),
        &BindGroupEntries::sequential((
            input.buffer.as_entire_buffer_binding(),
            &heights.texture_view,
        )),
    );
    commands.insert_resource(Bound(group, job.number));
}

/// Runs the pass once for each page asked for.
fn run(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pass: Res<Pass>,
    bound: Option<Res<Bound>>,
    mut done: Local<Option<u32>>,
) {
    let Some(bound) = bound else {
        return;
    };
    if *done == Some(bound.1) {
        return;
    }
    let Some(pipeline) = cache.get_compute_pipeline(pass.pipeline)
    else {
        return;
    };
    let mut compute = context.command_encoder().begin_compute_pass(
        &ComputePassDescriptor {
            label: Some("relief"),
            ..default()
        },
    );
    compute.set_pipeline(pipeline);
    compute.set_bind_group(0, &bound.0, &[]);
    compute.dispatch_workgroups(SIDE / 8, SIDE / 8, 1);
    *done = Some(bound.1);
}

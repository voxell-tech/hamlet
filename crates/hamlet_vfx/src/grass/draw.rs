//! The grass's two passes. Before the cameras draw,
//! `grass_place.wesl` looks at every place a tuft might stand about
//! the eye and lists each it keeps under its shape and detail,
//! counting each list into the arguments of its own draw. Then, in
//! each camera's opaque pass, each list is drawn as many times as it
//! holds, as `grass.wesl` has it: the world never learns how many
//! tufts there are.
//!
//! The grass is lit as everything else is, through the mesh
//! pipeline's view bindings, but has no material and is not in the
//! depth prepass.

use bevy::core_pipeline::core_3d::{
    Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey,
};
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::mesh::{
    Indices, MeshVertexBufferLayoutRef, MeshVertexBufferLayouts,
    PrimitiveTopology,
};
use bevy::pbr::{
    MeshPipeline, MeshPipelineKey, MeshPipelineSystems,
    SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
    ViewKeyCache,
};
use bevy::prelude::*;
use bevy::render::mesh::allocator::MeshSlabs;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, BinnedRenderPhaseType, DrawFunctions,
    InputUniformIndex, PhaseItem, RenderCommand, RenderCommandResult,
    SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
};
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only_sized, storage_buffer_sized,
    texture_2d, texture_2d_array, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, Buffer, BufferDescriptor,
    BufferInitDescriptor, BufferUsages, CachedComputePipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor,
    DynamicUniformBuffer, IndexFormat, PipelineCache,
    RenderPipelineDescriptor, SamplerBindingType, ShaderStages,
    ShaderType, SpecializedMeshPipeline,
    SpecializedMeshPipelineError, SpecializedMeshPipelines,
    TextureSampleType, UniformBuffer,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
    RenderQueue,
};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::ShaderDefVal;

use super::{DETAILS, Grass, GrassTufts, GrassUniform, SHAPES};
use crate::ground::{PagesUniform, SoilUniform};
use crate::wind::WindUniform;

/// Lists of tufts: one for each shape at each detail, the finest
/// first. `LISTS` in `grass_place.wesl`.
const LISTS: usize = SHAPES * DETAILS;

/// The most tufts a list holds; any more in sight are not drawn.
/// `CAPACITY` in `grass_place.wesl`.
const CAPACITY: u32 = 8192;

/// Words in a list's draw's arguments: its tuft's indices, the tufts
/// in it, its tuft's first index and first point, and its first tuft.
const ARGUMENTS: usize = 5;

/// Places a tuft might stand that the pass looks at together, along
/// each side. `@workgroup_size` of `place` in `grass_place.wesl`.
const GROUP: u32 = 8;

pub(super) fn plugin(app: &mut App) {
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .add_render_command::<Opaque3d, DrawGrass>()
        .init_resource::<SpecializedMeshPipelines<GrassPipeline>>()
        .add_systems(RenderStartup, start.after(MeshPipelineSystems))
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareResources),
                bind.in_set(RenderSystems::PrepareBindGroups),
                queue.in_set(RenderSystems::QueueMeshes),
            ),
        )
        // Before the cameras draw, so they draw this frame's lists.
        .add_systems(
            RenderGraph,
            place.in_set(RenderGraphSystems::Begin),
        );
}

/// What the pass reads besides the grass, as `Place` in
/// `grass_place.wesl`: the place on the map, in tufts, its first look
/// is at, how many it looks at each way, and the planes the eye sees
/// between.
#[derive(ShaderType, Clone, Default)]
struct PlaceInput {
    origin: IVec2,
    size: UVec2,
    sight: [Vec4; 6],
}

/// Where a list's tufts begin in the lists, as `First` in
/// `grass.wesl`.
#[derive(ShaderType, Clone, Default)]
struct First {
    tuft: u32,
}

/// The draw's pipeline: the mesh pipeline's, with the grass's own
/// bindings after the view's.
#[derive(Resource, Clone)]
struct GrassPipeline {
    mesh: MeshPipeline,
    shader: Handle<Shader>,
    layout: BindGroupLayoutDescriptor,
}

/// The pass, made once: its bindings, and its two steps, which list
/// the tufts and then keep each list's count within what it holds.
#[derive(Resource)]
struct PlacePass {
    layout: BindGroupLayoutDescriptor,
    place: CachedComputePipelineId,
    cap: CachedComputePipelineId,
}

/// The render world's own stand-in for the grass in the opaque pass.
#[derive(Resource)]
struct GrassItem(Entity);

/// Entries for the bindings the pass and the draw share, as
/// `grass_shape.wesl` and the libraries it reads declare them.
macro_rules! shared_entries {
    () => {
        (
            (100, uniform_buffer::<GrassUniform>(false)),
            (101, uniform_buffer::<WindUniform>(false)),
            (103, uniform_buffer::<SoilUniform>(false)),
            (104, texture_2d_array(TextureSampleType::Uint)),
            (105, uniform_buffer::<PagesUniform>(false)),
            (
                106,
                texture_2d(TextureSampleType::Float {
                    filterable: true,
                }),
            ),
            (107, sampler(SamplerBindingType::Filtering)),
        )
    };
}

fn start(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cache: Res<PipelineCache>,
    mesh: Res<MeshPipeline>,
) {
    let (grass, wind, soil, pages, pages_at, mask, mask_sampler) =
        shared_entries!();
    let layout = BindGroupLayoutDescriptor::new(
        "grass place",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                grass,
                wind,
                soil,
                pages,
                pages_at,
                mask,
                mask_sampler,
                (120, uniform_buffer::<PlaceInput>(false)),
                (121, storage_buffer_sized(false, None)),
                (122, storage_buffer_sized(false, None)),
            ),
        ),
    );
    // Embedded beside `grass.rs`, not in this module's own folder.
    let shader =
        assets.load("embedded://hamlet_vfx/grass_place.wesl");
    let step = |entry: &'static str| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("grass {entry}").into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            shader_defs: vec![ShaderDefVal::UInt(
                "MATERIAL_BIND_GROUP".into(),
                0,
            )],
            entry_point: Some(entry.into()),
            ..default()
        })
    };
    let (place, cap) = (step("place"), step("cap"));
    commands.insert_resource(PlacePass { layout, place, cap });

    let (grass, wind, soil, pages, pages_at, mask, mask_sampler) =
        shared_entries!();
    commands.insert_resource(GrassPipeline {
        mesh: mesh.clone(),
        shader: assets.load("embedded://hamlet_vfx/grass.wesl"),
        layout: BindGroupLayoutDescriptor::new(
            "grass draw",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::VERTEX_FRAGMENT,
                (
                    grass,
                    wind,
                    soil,
                    pages,
                    pages_at,
                    mask,
                    mask_sampler,
                    (
                        110,
                        storage_buffer_read_only_sized(false, None),
                    ),
                    (111, uniform_buffer::<First>(true)),
                ),
            ),
        ),
    });
    let item = commands.spawn_empty().id();
    commands.insert_resource(GrassItem(item));
}

impl SpecializedMeshPipeline for GrassPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError>
    {
        let mut descriptor = self.mesh.specialize(key, layout)?;
        descriptor.label = Some("grass".into());
        // The view's bindings, then the grass's in place of a mesh's.
        descriptor.layout.truncate(2);
        descriptor.layout.push(self.layout.clone());
        let group =
            ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 2);
        descriptor.vertex.shader = self.shader.clone();
        descriptor.vertex.shader_defs.push(group.clone());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = self.shader.clone();
            fragment.shader_defs.push(group);
        }
        // A blade is seen from both sides.
        descriptor.primitive.cull_mode = None;
        Ok(descriptor)
    }
}

/// The lists and what they are drawn with: made anew when the tufts
/// are, written each frame.
#[derive(Resource)]
struct Lists {
    /// Each tuft kept, as its place on the map, in tufts, list by
    /// list, [`CAPACITY`] to a list.
    tufts: Buffer,
    /// Each list's draw's arguments.
    arguments: Buffer,
    /// The arguments as a frame begins, the lists empty.
    blank: Vec<u8>,
    vertices: Buffer,
    indices: Buffer,
    layout: MeshVertexBufferLayoutRef,
    grass: UniformBuffer<GrassUniform>,
    wind: UniformBuffer<WindUniform>,
    soil: UniformBuffer<SoilUniform>,
    pages_at: UniformBuffer<PagesUniform>,
    input: UniformBuffer<PlaceInput>,
    firsts: DynamicUniformBuffer<First>,
    /// Each list's offset into `firsts`.
    offsets: Vec<u32>,
    /// How many places the pass looks at each way.
    size: UVec2,
    /// Made each frame once all they bind is ready, and kept while
    /// any of it is missing.
    groups: Option<(BindGroup, BindGroup)>,
}

/// Makes the lists anew when the tufts are, and writes this frame's
/// grass, sight and empty lists.
fn prepare(
    mut commands: Commands,
    grass: Option<Res<Grass>>,
    tufts: Option<Res<GrassTufts>>,
    lists: Option<ResMut<Lists>>,
    mut layouts: ResMut<MeshVertexBufferLayouts>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let (Some(grass), Some(tufts)) = (grass, tufts) else {
        return;
    };
    let mut lists = match lists {
        Some(lists) if !tufts.is_changed() => lists,
        _ => {
            if let Some(lists) =
                made(&tufts, &mut layouts, &device, &queue)
            {
                commands.insert_resource(lists);
            }
            return;
        }
    };
    let lists = &mut *lists;
    lists.grass.set(grass.grass.clone());
    lists.wind.set(grass.wind.clone());
    lists.soil.set(grass.soil.clone());
    lists.pages_at.set(grass.pages_at.clone());
    // Every place a tuft might stand within the eye's reach.
    let step = grass.grass.grow.x.max(0.05);
    let eye = grass.grass.reach.xz();
    let far = grass.grass.reach.w;
    let origin = ((eye - far) / step).floor().as_ivec2();
    let size = UVec2::splat((2.0 * far / step).ceil() as u32 + 2);
    lists.size = size;
    lists.input.set(PlaceInput {
        origin,
        size,
        sight: grass.sight,
    });
    lists.grass.write_buffer(&device, &queue);
    lists.wind.write_buffer(&device, &queue);
    lists.soil.write_buffer(&device, &queue);
    lists.pages_at.write_buffer(&device, &queue);
    lists.input.write_buffer(&device, &queue);
    queue.write_buffer(&lists.arguments, 0, &lists.blank);
}

/// The lists for `tufts`: their tufts' points and indices one after
/// another, and empty lists.
fn made(
    tufts: &GrassTufts,
    layouts: &mut MeshVertexBufferLayouts,
    device: &RenderDevice,
    queue: &RenderQueue,
) -> Option<Lists> {
    let first = tufts.0.first()?;
    let layout = first.get_mesh_vertex_buffer_layout(layouts);
    let mut points = Vec::new();
    let mut indices = Vec::<u8>::new();
    let mut blank = Vec::new();
    let (mut index, mut point) = (0u32, 0u32);
    for tuft in tufts.0.iter().take(LISTS) {
        let count = tuft.indices().map_or(0, Indices::len) as u32;
        let words = [count, 0, index, point, 0];
        blank.extend(words.iter().flat_map(|w| w.to_le_bytes()));
        points.extend(tuft.create_packed_vertex_buffer_data());
        indices.extend(tuft.get_index_buffer_bytes()?);
        index += count;
        point += tuft.count_vertices() as u32;
    }
    if blank.len() != LISTS * ARGUMENTS * 4 {
        return None;
    }
    let vertices =
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("grass points"),
            contents: &points,
            usage: BufferUsages::VERTEX,
        });
    let indices =
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("grass indices"),
            contents: &indices,
            usage: BufferUsages::INDEX,
        });
    let arguments = device.create_buffer(&BufferDescriptor {
        label: Some("grass lists' draws"),
        size: blank.len() as u64,
        usage: BufferUsages::STORAGE
            | BufferUsages::INDIRECT
            | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let tufts = device.create_buffer(&BufferDescriptor {
        label: Some("grass lists"),
        size: (LISTS as u64) * u64::from(CAPACITY) * 8,
        usage: BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let mut firsts = DynamicUniformBuffer::default();
    let offsets = (0..LISTS as u32)
        .map(|list| {
            firsts.push(&First {
                tuft: list * CAPACITY,
            })
        })
        .collect();
    firsts.write_buffer(device, queue);
    Some(Lists {
        tufts,
        arguments,
        blank,
        vertices,
        indices,
        layout,
        grass: default(),
        wind: default(),
        soil: default(),
        pages_at: default(),
        input: default(),
        firsts,
        offsets,
        size: UVec2::ZERO,
        groups: None,
    })
}

/// Binds this frame's grass, pages and mask to the pass and the draw,
/// once all of them are ready.
fn bind(
    grass: Option<Res<Grass>>,
    lists: Option<ResMut<Lists>>,
    images: Res<RenderAssets<GpuImage>>,
    pass: Res<PlacePass>,
    pipeline: Res<GrassPipeline>,
    (device, cache): (Res<RenderDevice>, Res<PipelineCache>),
) {
    let (Some(grass), Some(mut lists)) = (grass, lists) else {
        return;
    };
    let lists = &mut *lists;
    // While the pages or the mask are made anew they may be missing
    // for a frame: last frame's bindings are kept, rather than none
    // drawn.
    let (Some(pages), Some(mask)) =
        (images.get(&grass.pages), images.get(&grass.mask))
    else {
        return;
    };
    let (
        Some(grass),
        Some(wind),
        Some(soil),
        Some(pages_at),
        Some(input),
        Some(first),
    ) = (
        lists.grass.binding(),
        lists.wind.binding(),
        lists.soil.binding(),
        lists.pages_at.binding(),
        lists.input.binding(),
        lists.firsts.binding(),
    )
    else {
        return;
    };
    let placing = device.create_bind_group(
        Some("grass place"),
        &cache.get_bind_group_layout(&pass.layout),
        &BindGroupEntries::with_indices((
            (100, grass.clone()),
            (101, wind.clone()),
            (103, soil.clone()),
            (104, &pages.texture_view),
            (105, pages_at.clone()),
            (106, &mask.texture_view),
            (107, &mask.sampler),
            (120, input),
            (121, lists.tufts.as_entire_buffer_binding()),
            (122, lists.arguments.as_entire_buffer_binding()),
        )),
    );
    let drawing = device.create_bind_group(
        Some("grass draw"),
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::with_indices((
            (100, grass),
            (101, wind),
            (103, soil),
            (104, &pages.texture_view),
            (105, pages_at),
            (106, &mask.texture_view),
            (107, &mask.sampler),
            (110, lists.tufts.as_entire_buffer_binding()),
            (111, first),
        )),
    );
    lists.groups = Some((placing, drawing));
}

/// Lists the tufts kept this frame, then keeps each list's count
/// within what it holds.
fn place(
    mut context: RenderContext,
    grass: Option<Res<Grass>>,
    lists: Option<Res<Lists>>,
    pass: Res<PlacePass>,
    cache: Res<PipelineCache>,
) {
    let (Some(grass), Some(lists)) = (grass, lists) else {
        return;
    };
    let (Some((placing, _)), Some(place), Some(cap)) = (
        &lists.groups,
        cache.get_compute_pipeline(pass.place),
        cache.get_compute_pipeline(pass.cap),
    ) else {
        return;
    };
    if !grass.shown {
        return;
    }
    let mut compute = context.command_encoder().begin_compute_pass(
        &ComputePassDescriptor {
            label: Some("grass place"),
            ..default()
        },
    );
    compute.set_bind_group(0, placing, &[]);
    compute.set_pipeline(place);
    compute.dispatch_workgroups(
        lists.size.x.div_ceil(GROUP),
        lists.size.y.div_ceil(GROUP),
        1,
    );
    compute.set_pipeline(cap);
    compute.dispatch_workgroups(1, 1, 1);
}

/// Puts the grass in each camera's opaque pass, anew each frame, so
/// that it is drawn with the pipeline the view now calls for, and
/// takes it out when it is not drawn.
fn queue(
    grass: Option<Res<Grass>>,
    lists: Option<Res<Lists>>,
    item: Res<GrassItem>,
    pipeline: Res<GrassPipeline>,
    mut pipelines: ResMut<SpecializedMeshPipelines<GrassPipeline>>,
    cache: Res<PipelineCache>,
    draws: Res<DrawFunctions<Opaque3d>>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    views: Query<&ExtractedView>,
    keys: Res<ViewKeyCache>,
) {
    let draw = draws.read().id::<DrawGrass>();
    let main = MainEntity::from(item.0);
    let drawn = grass.is_some_and(|grass| grass.shown);
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity)
        else {
            continue;
        };
        phase.remove(main);
        let (Some(lists), Some(&key), true) =
            (&lists, keys.get(&view.retained_view_entity), drawn)
        else {
            continue;
        };
        let key = key
            | MeshPipelineKey::from_primitive_topology_and_strip_index(
                PrimitiveTopology::TriangleList,
                None,
            );
        let Ok(id) = pipelines.specialize(
            &cache,
            &pipeline,
            key,
            &lists.layout,
        ) else {
            continue;
        };
        phase.add(
            Opaque3dBatchSetKey {
                draw_function: draw,
                pipeline: id,
                material_bind_group_index: None,
                lightmap_slab: None,
                slabs: MeshSlabs::default(),
            },
            Opaque3dBinKey {
                asset_id: AssetId::<Mesh>::default().untyped(),
            },
            (item.0, main),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
        );
    }
}

type DrawGrass = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    DrawLists,
);

/// Draws each list as many times as the pass counted into it.
struct DrawLists;

impl<P: PhaseItem> RenderCommand<P> for DrawLists {
    type Param = SRes<Lists>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _: &P,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        lists: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let lists = lists.into_inner();
        let Some((_, drawing)) = &lists.groups else {
            return RenderCommandResult::Skip;
        };
        pass.set_vertex_buffer(0, lists.vertices.slice(..));
        pass.set_index_buffer(
            lists.indices.slice(..),
            IndexFormat::Uint32,
        );
        for (list, &offset) in lists.offsets.iter().enumerate() {
            pass.set_bind_group(2, drawing, &[offset]);
            pass.draw_indexed_indirect(
                &lists.arguments,
                (list * ARGUMENTS * 4) as u64,
            );
        }
        RenderCommandResult::Success
    }
}

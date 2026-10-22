//! A plugin's scene values, as the WIT has them, made into Bevy's,
//! each checked: a number that is not finite, or out of its range, is
//! refused.

use bevy::asset::RenderAssetUsages;
use bevy::camera::{
    ClearColorConfig, Exposure, OrthographicProjection,
    PerspectiveProjection, Projection as Lens, ScalingMode,
};
use bevy::color::{Color, LinearRgba};
use bevy::core_pipeline::tonemapping::Tonemapping as Mapping;
use bevy::light::{
    AmbientLight, DirectionalLight, PointLight, SpotLight,
};
use bevy::material::AlphaMode as Blend;
use bevy::math::Quat;
use bevy::math::primitives::{
    Capsule3d, Cone, Cuboid, Cylinder, Plane3d, Sphere, Torus,
};
use bevy::mesh::{
    Indices, Mesh, MeshBuilder, Meshable, PrimitiveTopology,
};
use bevy::pbr::{
    DistanceFog, FogFalloff as Falloff, StandardMaterial,
};
use bevy::post_process::bloom::{Bloom as Glow, BloomPrefilter};
use bevy::render::render_resource::Face;
use bevy::render::view::Msaa;
use bevy::transform::components::Transform as Placed;
use hamlet_engine_render::{
    CameraSetup, Hit as Met, PointerButton, SceneEvent, SceneLight,
};

use crate::scene::scene_bindings::exports::hamlet::engine::scene_guest::{
    Hit, MouseButton, Pointer, Press, SceneInput, Wheel,
};
use crate::scene::scene_bindings::hamlet::engine::scene::{
    AlphaMode, CameraSettings, FogFalloff, Light, MeshData, Pbr,
    Projection, Rgba, Shape, Tonemapping, Transform,
};

/// A mesh, and what it holds against the budgets.
pub(crate) struct Made {
    pub(crate) mesh: Mesh,
    pub(crate) vertices: usize,
    /// Bytes of its vertices and indices on the GPU.
    pub(crate) bytes: u64,
}

impl Made {
    fn of(mesh: Mesh) -> Self {
        let indices = mesh.indices().map_or(0, Indices::len);
        Self {
            vertices: mesh.count_vertices(),
            bytes: (mesh.get_vertex_buffer_size() + indices * 4)
                as u64,
            mesh,
        }
    }
}

impl MeshData {
    /// Bytes the plugin handed over.
    pub(crate) fn bytes(&self) -> usize {
        size_of_val(self.positions.as_slice())
            + size_of_val(self.normals.as_slice())
            + size_of_val(self.uvs.as_slice())
            + size_of_val(self.colors.as_slice())
            + size_of_val(self.indices.as_slice())
    }
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn positive(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite() && *value > 0.0)
}

fn color((r, g, b, a): Rgba) -> Option<LinearRgba> {
    finite(&[r, g, b, a]).then(|| LinearRgba::new(r, g, b, a))
}

pub(crate) fn shape(shape: Shape) -> Result<Made, String> {
    let refused = || Err("a shape's sizes are positive".to_owned());
    let mesh = match shape {
        Shape::Cuboid((x, y, z)) if positive(&[x, y, z]) => {
            Mesh::from(Cuboid::new(x, y, z))
        }
        Shape::Sphere(radius) if positive(&[radius]) => {
            Mesh::from(Sphere::new(radius))
        }
        Shape::Plane((x, z)) if positive(&[x, z]) => {
            Plane3d::default().mesh().size(x, z).build()
        }
        Shape::Cylinder((radius, height))
            if positive(&[radius, height]) =>
        {
            Mesh::from(Cylinder::new(radius, height))
        }
        Shape::Capsule((radius, length))
            if positive(&[radius, length]) =>
        {
            Mesh::from(Capsule3d::new(radius, length))
        }
        Shape::Cone((radius, height))
            if positive(&[radius, height]) =>
        {
            Mesh::from(Cone { radius, height })
        }
        Shape::Torus((tube, ring))
            if positive(&[tube, ring]) && ring > tube =>
        {
            Mesh::from(Torus::new(ring - tube, ring + tube))
        }
        _ => return refused(),
    };
    Ok(Made::of(mesh))
}

pub(crate) fn mesh(data: MeshData) -> Result<Made, String> {
    let MeshData {
        positions,
        normals,
        uvs,
        colors,
        indices,
    } = data;
    if !positions.len().is_multiple_of(3) {
        return Err("positions are three floats each".into());
    }
    let count = positions.len() / 3;
    for (name, list, width) in [
        ("normals", &normals, 3),
        ("uvs", &uvs, 2),
        ("colors", &colors, 4),
    ] {
        if !list.is_empty() && list.len() != count * width {
            return Err(format!(
                "{name} are {width} floats for each of {count} \
                 vertices"
            ));
        }
    }
    if !indices.len().is_multiple_of(3) {
        return Err("indices are three for each triangle".into());
    }
    if indices.iter().any(|&at| at as usize >= count) {
        return Err(format!("an index is past the {count} vertices"));
    }
    if ![&positions, &normals, &uvs, &colors]
        .iter()
        .all(|list| finite(list))
    {
        return Err("a value of the mesh is not finite".into());
    }
    let threes = |list: &[f32]| list.as_chunks::<3>().0.to_vec();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        threes(&positions),
    )
    .with_inserted_indices(Indices::U32(indices));
    if normals.is_empty() {
        mesh.compute_normals();
    } else {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            threes(&normals),
        );
    }
    if !uvs.is_empty() {
        let pairs = uvs.as_chunks::<2>().0.to_vec();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, pairs);
    }
    if !colors.is_empty() {
        let fours = colors.as_chunks::<4>().0.to_vec();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, fours);
    }
    Ok(Made::of(mesh))
}

/// A rotation of `x`, `y`, `z` and `w`, made whole; none when it is
/// not finite.
fn rotation(x: f32, y: f32, z: f32, w: f32) -> Option<Quat> {
    if !finite(&[x, y, z, w]) {
        return None;
    }
    let turned = Quat::from_xyzw(x, y, z, w);
    Some(if turned.length_squared() > 0.0 {
        turned.normalize()
    } else {
        Quat::IDENTITY
    })
}

pub(crate) fn transform(transform: Transform) -> Option<Placed> {
    let Transform {
        translation: (x, y, z),
        rotation: (rx, ry, rz, rw),
        scale: (sx, sy, sz),
    } = transform;
    copy(&[x, y, z, rx, ry, rz, rw, sx, sy, sz])
}

/// A copy of instances: translation, rotation and scale.
pub(crate) fn copy(floats: &[f32]) -> Option<Placed> {
    let &[x, y, z, rx, ry, rz, rw, sx, sy, sz] = floats else {
        return None;
    };
    if !finite(&[x, y, z, sx, sy, sz]) {
        return None;
    }
    Some(Placed {
        translation: (x, y, z).into(),
        rotation: rotation(rx, ry, rz, rw)?,
        scale: (sx, sy, sz).into(),
    })
}

pub(crate) fn alpha(mode: AlphaMode, cutoff: f32) -> Blend {
    match mode {
        AlphaMode::Opaque => Blend::Opaque,
        AlphaMode::Mask => Blend::Mask(cutoff),
        AlphaMode::Blend => Blend::Blend,
        AlphaMode::Premultiplied => Blend::Premultiplied,
        AlphaMode::Add => Blend::Add,
        AlphaMode::Multiply => Blend::Multiply,
    }
}

pub(crate) fn pbr(pbr: Pbr) -> Option<StandardMaterial> {
    if !finite(&[pbr.roughness, pbr.metallic, pbr.alpha_cutoff]) {
        return None;
    }
    Some(StandardMaterial {
        base_color: Color::LinearRgba(color(pbr.base_color)?),
        emissive: color(pbr.emissive)?,
        perceptual_roughness: pbr.roughness.clamp(0.0, 1.0),
        metallic: pbr.metallic.clamp(0.0, 1.0),
        alpha_mode: alpha(pbr.alpha_mode, pbr.alpha_cutoff),
        unlit: pbr.unlit,
        double_sided: pbr.double_sided,
        cull_mode: (!pbr.double_sided).then_some(Face::Back),
        fog_enabled: pbr.fog,
        ..StandardMaterial::default()
    })
}

pub(crate) fn light(light: Light) -> Option<SceneLight> {
    Some(match light {
        Light::Directional(light) => {
            if !finite(&[light.illuminance]) {
                return None;
            }
            SceneLight::Directional(DirectionalLight {
                color: Color::LinearRgba(color(light.color)?),
                illuminance: light.illuminance.max(0.0),
                shadow_maps_enabled: light.shadows,
                ..DirectionalLight::default()
            })
        }
        Light::Point(light) => {
            if !finite(&[light.intensity, light.range, light.radius])
            {
                return None;
            }
            SceneLight::Point(PointLight {
                color: Color::LinearRgba(color(light.color)?),
                intensity: light.intensity.max(0.0),
                range: light.range.max(0.0),
                radius: light.radius.max(0.0),
                shadow_maps_enabled: light.shadows,
                ..PointLight::default()
            })
        }
        Light::Spot(light) => {
            let values = [
                light.intensity,
                light.range,
                light.radius,
                light.inner_angle,
                light.outer_angle,
            ];
            if !finite(&values) {
                return None;
            }
            let outer = light
                .outer_angle
                .clamp(0.0, core::f32::consts::FRAC_PI_2);
            SceneLight::Spot(SpotLight {
                color: Color::LinearRgba(color(light.color)?),
                intensity: light.intensity.max(0.0),
                range: light.range.max(0.0),
                radius: light.radius.max(0.0),
                inner_angle: light.inner_angle.clamp(0.0, outer),
                outer_angle: outer,
                shadow_maps_enabled: light.shadows,
                ..SpotLight::default()
            })
        }
    })
}

pub(crate) fn camera(
    settings: CameraSettings,
) -> Option<CameraSetup> {
    let projection = match settings.projection {
        Projection::Perspective((fov, near, far))
            if positive(&[fov, near, far])
                && fov < core::f32::consts::PI
                && near < far =>
        {
            Lens::Perspective(PerspectiveProjection {
                fov,
                near,
                far,
                ..PerspectiveProjection::default()
            })
        }
        Projection::Orthographic((height, near, far))
            if positive(&[height])
                && finite(&[near, far])
                && near < far =>
        {
            Lens::Orthographic(OrthographicProjection {
                near,
                far,
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: height,
                },
                ..OrthographicProjection::default_3d()
            })
        }
        _ => return None,
    };
    let clear = match settings.clear {
        Some(clear) => {
            ClearColorConfig::Custom(Color::LinearRgba(color(clear)?))
        }
        None => ClearColorConfig::None,
    };
    let fog = match settings.fog {
        Some(fog) => {
            let falloff = match fog.falloff {
                FogFalloff::Linear((start, end))
                    if finite(&[start, end]) =>
                {
                    Falloff::Linear { start, end }
                }
                FogFalloff::Exponential(density)
                    if positive(&[density]) =>
                {
                    Falloff::Exponential { density }
                }
                FogFalloff::ExponentialSquared(density)
                    if positive(&[density]) =>
                {
                    Falloff::ExponentialSquared { density }
                }
                _ => return None,
            };
            Some(DistanceFog {
                color: Color::LinearRgba(color(fog.color)?),
                falloff,
                ..DistanceFog::default()
            })
        }
        None => None,
    };
    let bloom = match settings.bloom {
        Some(bloom) => {
            let values = [
                bloom.intensity,
                bloom.low_frequency_boost,
                bloom.threshold,
            ];
            if !finite(&values) {
                return None;
            }
            Some(Glow {
                intensity: bloom.intensity.max(0.0),
                low_frequency_boost: bloom
                    .low_frequency_boost
                    .max(0.0),
                prefilter: BloomPrefilter {
                    threshold: bloom.threshold.max(0.0),
                    threshold_softness: 0.0,
                },
                ..Glow::NATURAL
            })
        }
        None => None,
    };
    let (ambient, brightness) = settings.ambient;
    if !finite(&[settings.exposure, brightness]) {
        return None;
    }
    let msaa = match settings.msaa {
        1 => Msaa::Off,
        2 => Msaa::Sample2,
        4 => Msaa::Sample4,
        8 => Msaa::Sample8,
        _ => return None,
    };
    Some(CameraSetup {
        projection,
        clear,
        hdr: settings.hdr,
        tonemapping: match settings.tonemapping {
            Tonemapping::None => Mapping::None,
            Tonemapping::Simple => Mapping::Reinhard,
            Tonemapping::Luminance => Mapping::ReinhardLuminance,
            Tonemapping::Fitted => Mapping::AcesFitted,
            Tonemapping::Agx => Mapping::AgX,
            Tonemapping::Display => {
                Mapping::SomewhatBoringDisplayTransform
            }
            Tonemapping::Neutral => Mapping::TonyMcMapface,
            Tonemapping::Film => Mapping::BlenderFilmic,
        },
        exposure: Exposure {
            ev100: settings.exposure,
        },
        fog,
        bloom,
        ambient: AmbientLight {
            color: Color::LinearRgba(color(ambient)?),
            brightness: brightness.max(0.0),
            affects_lightmapped_meshes: true,
        },
        msaa,
    })
}

fn hit(met: Met) -> Hit {
    Hit {
        key: met.key,
        index: met.index,
        point: met.point.into(),
        normal: met.normal.into(),
    }
}

/// What the person did, as the plugin is told it.
pub(crate) fn input(event: SceneEvent) -> SceneInput {
    match event {
        SceneEvent::Over(met) => SceneInput::Over(hit(met)),
        SceneEvent::Out { key, index } => {
            SceneInput::Out((key, index))
        }
        SceneEvent::Click(met) => SceneInput::Click(hit(met)),
        SceneEvent::Moved { view, at, by } => {
            SceneInput::Moved(Pointer {
                view,
                x: at.x,
                y: at.y,
                dx: by.x,
                dy: by.y,
            })
        }
        SceneEvent::Pressed {
            view,
            button,
            down,
            at,
        } => SceneInput::Pressed(Press {
            view,
            button: match button {
                PointerButton::Primary => MouseButton::Primary,
                PointerButton::Secondary => MouseButton::Secondary,
                PointerButton::Middle => MouseButton::Middle,
            },
            down,
            x: at.x,
            y: at.y,
        }),
        SceneEvent::Turned { view, by } => {
            SceneInput::Turned(Wheel {
                view,
                x: by.x,
                y: by.y,
            })
        }
        SceneEvent::Resized { view, size } => {
            SceneInput::Resized((view, size.x, size.y))
        }
    }
}

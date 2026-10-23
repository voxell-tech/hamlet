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
    AmbientLight, DirectionalLight, PointLight, ShadowFilteringMethod,
    SpotLight,
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
use bevy::input::keyboard::KeyCode as Key;
use hamlet_engine_render::{
    Balloon, CameraSetup, Hit as Met, LabelLook as Look, Modifiers,
    PointerButton, PointerLabel, SceneEvent, SceneLight, WorldLabel,
};

use crate::scene::scene_bindings::exports::hamlet::engine::scene_guest::{
    Buttons, Click, Dropped, Hit, Key as Typed, KeyCode,
    Modifiers as Held, MouseButton, Pointer, Press, SceneInput, Scroll,
};
use crate::scene::scene_bindings::hamlet::engine::scene::{
    AlphaMode, Balloon as WitBalloon, CameraSettings, Decal,
    FogFalloff, Label, LabelLook, Light, MeshData, Msaa as WitMsaa,
    PointerLabel as WitPointerLabel, Projection, Rgba,
    ShadowFilteringMethod as WitShadowFiltering, Shape,
    StandardMaterial as Wit, Tonemapping, Transform,
};

/// A mesh, and the GPU memory it takes.
pub(crate) struct Made {
    pub(crate) mesh: Mesh,
    /// Bytes of its vertices and indices on the GPU.
    pub(crate) bytes: u64,
}

impl Made {
    fn of(mesh: Mesh) -> Self {
        let indices = mesh.indices().map_or(0, Indices::len);
        Self {
            bytes: (mesh.get_vertex_buffer_size() + indices * 4)
                as u64,
            mesh,
        }
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
        Shape::Plane3d((x, z)) if positive(&[x, z]) => {
            Plane3d::default().mesh().size(x, z).build()
        }
        Shape::Cylinder((radius, height))
            if positive(&[radius, height]) =>
        {
            Mesh::from(Cylinder::new(radius, height))
        }
        Shape::Capsule3d((radius, length))
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

pub(crate) fn standard_material(
    material: Wit,
) -> Option<StandardMaterial> {
    if !finite(&[
        material.perceptual_roughness,
        material.metallic,
        material.reflectance,
        material.alpha_cutoff,
    ]) {
        return None;
    }
    Some(StandardMaterial {
        base_color: Color::LinearRgba(color(material.base_color)?),
        emissive: color(material.emissive)?,
        perceptual_roughness: material
            .perceptual_roughness
            .clamp(0.0, 1.0),
        metallic: material.metallic.clamp(0.0, 1.0),
        reflectance: material.reflectance.clamp(0.0, 1.0),
        alpha_mode: alpha(material.alpha_mode, material.alpha_cutoff),
        unlit: material.unlit,
        double_sided: material.double_sided,
        cull_mode: (!material.double_sided).then_some(Face::Back),
        fog_enabled: material.fog_enabled,
        ..StandardMaterial::default()
    })
}

/// A decal's material, always blended, and how far from its square
/// it fades out.
pub(crate) fn decal(decal: Decal) -> Option<(StandardMaterial, f32)> {
    if !positive(&[decal.depth_fade_factor]) {
        return None;
    }
    let material = StandardMaterial {
        base_color: Color::LinearRgba(color(decal.base_color)?),
        emissive: color(decal.emissive)?,
        alpha_mode: Blend::Blend,
        unlit: decal.unlit,
        fog_enabled: decal.fog_enabled,
        ..StandardMaterial::default()
    };
    Some((material, decal.depth_fade_factor))
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
                shadow_maps_enabled: light.shadow_maps_enabled,
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
                shadow_maps_enabled: light.shadow_maps_enabled,
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
                shadow_maps_enabled: light.shadow_maps_enabled,
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
    let clear = match settings.clear_color {
        Some(clear) => {
            ClearColorConfig::Custom(Color::LinearRgba(color(clear)?))
        }
        None => ClearColorConfig::None,
    };
    let fog = match settings.distance_fog {
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
                bloom.prefilter.threshold,
                bloom.prefilter.threshold_softness,
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
                    threshold: bloom.prefilter.threshold.max(0.0),
                    threshold_softness: bloom
                        .prefilter
                        .threshold_softness
                        .clamp(0.0, 1.0),
                },
                ..Glow::NATURAL
            })
        }
        None => None,
    };
    let ambient = settings.ambient_light;
    if !finite(&[settings.exposure.ev100, ambient.brightness]) {
        return None;
    }
    let msaa = match settings.msaa {
        WitMsaa::Off => Msaa::Off,
        WitMsaa::Sample2 => Msaa::Sample2,
        WitMsaa::Sample4 => Msaa::Sample4,
        WitMsaa::Sample8 => Msaa::Sample8,
    };
    Some(CameraSetup {
        projection,
        clear,
        hdr: settings.hdr,
        tonemapping: match settings.tonemapping {
            Tonemapping::None => Mapping::None,
            Tonemapping::Reinhard => Mapping::Reinhard,
            Tonemapping::ReinhardLuminance => {
                Mapping::ReinhardLuminance
            }
            Tonemapping::AcesFitted => Mapping::AcesFitted,
            Tonemapping::AgX => Mapping::AgX,
            Tonemapping::SomewhatBoringDisplayTransform => {
                Mapping::SomewhatBoringDisplayTransform
            }
            Tonemapping::TonyMcMapface => Mapping::TonyMcMapface,
            Tonemapping::BlenderFilmic => Mapping::BlenderFilmic,
            Tonemapping::KhronosPbrNeutral => {
                Mapping::KhronosPbrNeutral
            }
        },
        exposure: Exposure {
            ev100: settings.exposure.ev100,
        },
        fog,
        bloom,
        ambient: AmbientLight {
            color: Color::LinearRgba(color(ambient.color)?),
            brightness: ambient.brightness.max(0.0),
            affects_lightmapped_meshes: true,
        },
        msaa,
        shadow_filtering: match settings.shadow_filtering_method {
            WitShadowFiltering::Hardware2x2 => {
                ShadowFilteringMethod::Hardware2x2
            }
            WitShadowFiltering::Gaussian => {
                ShadowFilteringMethod::Gaussian
            }
            WitShadowFiltering::Temporal => {
                ShadowFilteringMethod::Temporal
            }
        },
    })
}

/// Words over an entity, of finite sizes.
pub(crate) fn label(label: Label) -> Option<WorldLabel> {
    finite(&[label.gap, label.far]).then_some(WorldLabel {
        text: label.text,
        look: match label.look {
            LabelLook::Plain => Look::Plain,
            LabelLook::Sign => Look::Sign,
            LabelLook::Tag => Look::Tag,
        },
        gap: label.gap,
        far: label.far,
    })
}

/// A balloon over an entity, of finite sizes.
pub(crate) fn balloon(balloon: WitBalloon) -> Option<Balloon> {
    finite(&[balloon.gap, balloon.far]).then_some(Balloon {
        text: balloon.text,
        thought: balloon.thought,
        gap: balloon.gap,
        far: balloon.far,
    })
}

/// Words beside the pointer, of finite colours and sizes, their size
/// above zero.
pub(crate) fn pointer_label(
    label: WitPointerLabel,
) -> Option<PointerLabel> {
    if !positive(&[label.size]) || !finite(&[label.x, label.y]) {
        return None;
    }
    Some(PointerLabel {
        text: label.text,
        color: Color::LinearRgba(color(label.color)?),
        back: Color::LinearRgba(color(label.back)?),
        size: label.size,
        offset: bevy::math::Vec2::new(label.x, label.y),
    })
}

fn hit(met: Met) -> Hit {
    Hit {
        key: met.key,
        index: met.index,
        position: met.point.into(),
        normal: met.normal.into(),
    }
}

fn button(button: PointerButton) -> MouseButton {
    match button {
        PointerButton::Primary => MouseButton::Primary,
        PointerButton::Secondary => MouseButton::Secondary,
        PointerButton::Middle => MouseButton::Middle,
    }
}

fn modifiers(mods: Modifiers) -> Held {
    let mut held = Held::empty();
    for (on, flag) in [
        (mods.shift, Held::SHIFT),
        (mods.control, Held::CONTROL),
        (mods.alt, Held::ALT),
        (mods.meta, Held::META),
    ] {
        if on {
            held |= flag;
        }
    }
    held
}

/// What the person did, as the plugin is told it, or none for a key
/// the interface does not name. A file dropped is told by
/// [`dropped`].
pub(crate) fn input(event: SceneEvent) -> Option<SceneInput> {
    Some(match event {
        SceneEvent::Over(met) => SceneInput::Over(hit(met)),
        SceneEvent::Out { key, index } => {
            SceneInput::Out((key, index))
        }
        SceneEvent::Click {
            hit: met,
            button: which,
            mods,
        } => SceneInput::Click(Click {
            hit: hit(met),
            button: button(which),
            modifiers: modifiers(mods),
        }),
        SceneEvent::Moved {
            view,
            at,
            by,
            held,
            mods,
        } => {
            let mut buttons = Buttons::empty();
            for (down, flag) in held.into_iter().zip([
                Buttons::PRIMARY,
                Buttons::SECONDARY,
                Buttons::MIDDLE,
            ]) {
                if down {
                    buttons |= flag;
                }
            }
            SceneInput::Move(Pointer {
                view,
                x: at.x,
                y: at.y,
                dx: by.x,
                dy: by.y,
                buttons,
                modifiers: modifiers(mods),
            })
        }
        SceneEvent::Pressed {
            view,
            button: which,
            down,
            at,
            mods,
        } => SceneInput::Press(Press {
            view,
            button: button(which),
            down,
            x: at.x,
            y: at.y,
            modifiers: modifiers(mods),
        }),
        SceneEvent::Turned { view, by } => {
            SceneInput::Scroll(Scroll {
                view,
                x: by.x,
                y: by.y,
            })
        }
        SceneEvent::Resized { view, size } => {
            SceneInput::Resized((view, size.x, size.y))
        }
        SceneEvent::Key {
            view,
            key,
            down,
            mods,
        } => SceneInput::Key(Typed {
            view,
            code: code(key)?,
            down,
            modifiers: modifiers(mods),
        }),
        SceneEvent::Dropped { .. } => return None,
    })
}

/// A file dropped on `view` at `at`, handed over as `id`.
pub(crate) fn dropped(
    view: u32,
    at: bevy::math::Vec2,
    id: u32,
    path: String,
) -> SceneInput {
    SceneInput::Dropped(Dropped {
        view,
        x: at.x,
        y: at.y,
        id,
        path,
    })
}

/// The interface's name of `key`, if it has one.
fn code(key: Key) -> Option<KeyCode> {
    const LETTERS: [(Key, KeyCode); 26] = [
        (Key::KeyA, KeyCode::KeyA),
        (Key::KeyB, KeyCode::KeyB),
        (Key::KeyC, KeyCode::KeyC),
        (Key::KeyD, KeyCode::KeyD),
        (Key::KeyE, KeyCode::KeyE),
        (Key::KeyF, KeyCode::KeyF),
        (Key::KeyG, KeyCode::KeyG),
        (Key::KeyH, KeyCode::KeyH),
        (Key::KeyI, KeyCode::KeyI),
        (Key::KeyJ, KeyCode::KeyJ),
        (Key::KeyK, KeyCode::KeyK),
        (Key::KeyL, KeyCode::KeyL),
        (Key::KeyM, KeyCode::KeyM),
        (Key::KeyN, KeyCode::KeyN),
        (Key::KeyO, KeyCode::KeyO),
        (Key::KeyP, KeyCode::KeyP),
        (Key::KeyQ, KeyCode::KeyQ),
        (Key::KeyR, KeyCode::KeyR),
        (Key::KeyS, KeyCode::KeyS),
        (Key::KeyT, KeyCode::KeyT),
        (Key::KeyU, KeyCode::KeyU),
        (Key::KeyV, KeyCode::KeyV),
        (Key::KeyW, KeyCode::KeyW),
        (Key::KeyX, KeyCode::KeyX),
        (Key::KeyY, KeyCode::KeyY),
        (Key::KeyZ, KeyCode::KeyZ),
    ];
    const OTHERS: [(Key, KeyCode); 49] = [
        (Key::Digit0, KeyCode::Digit0),
        (Key::Digit1, KeyCode::Digit1),
        (Key::Digit2, KeyCode::Digit2),
        (Key::Digit3, KeyCode::Digit3),
        (Key::Digit4, KeyCode::Digit4),
        (Key::Digit5, KeyCode::Digit5),
        (Key::Digit6, KeyCode::Digit6),
        (Key::Digit7, KeyCode::Digit7),
        (Key::Digit8, KeyCode::Digit8),
        (Key::Digit9, KeyCode::Digit9),
        (Key::F1, KeyCode::F1),
        (Key::F2, KeyCode::F2),
        (Key::F3, KeyCode::F3),
        (Key::F4, KeyCode::F4),
        (Key::F5, KeyCode::F5),
        (Key::F6, KeyCode::F6),
        (Key::F7, KeyCode::F7),
        (Key::F8, KeyCode::F8),
        (Key::F9, KeyCode::F9),
        (Key::F10, KeyCode::F10),
        (Key::F11, KeyCode::F11),
        (Key::F12, KeyCode::F12),
        (Key::Escape, KeyCode::Escape),
        (Key::Enter, KeyCode::Enter),
        (Key::Space, KeyCode::Space),
        (Key::Tab, KeyCode::Tab),
        (Key::Backspace, KeyCode::Backspace),
        (Key::Delete, KeyCode::Delete),
        (Key::ArrowUp, KeyCode::ArrowUp),
        (Key::ArrowDown, KeyCode::ArrowDown),
        (Key::ArrowLeft, KeyCode::ArrowLeft),
        (Key::ArrowRight, KeyCode::ArrowRight),
        (Key::Home, KeyCode::Home),
        (Key::End, KeyCode::End),
        (Key::PageUp, KeyCode::PageUp),
        (Key::PageDown, KeyCode::PageDown),
        (Key::Comma, KeyCode::Comma),
        (Key::Period, KeyCode::Period),
        (Key::Slash, KeyCode::Slash),
        (Key::Backslash, KeyCode::Backslash),
        (Key::Semicolon, KeyCode::Semicolon),
        (Key::Quote, KeyCode::Quote),
        (Key::Backquote, KeyCode::Backquote),
        (Key::Minus, KeyCode::Minus),
        (Key::Equal, KeyCode::Equal),
        (Key::BracketLeft, KeyCode::BracketLeft),
        (Key::BracketRight, KeyCode::BracketRight),
        (Key::NumpadEnter, KeyCode::Enter),
        (Key::NumpadComma, KeyCode::Comma),
    ];
    LETTERS
        .iter()
        .chain(&OTHERS)
        .find(|(bevy, _)| *bevy == key)
        .map(|(_, code)| *code)
}

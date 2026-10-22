//! 3D scenes the engine draws, kept entity by entity: meshes,
//! materials, entities with lights or decals, instances and cameras.
//! A camera
//! draws into a [`viewport`] of a panel, or behind the dock. What the
//! person does over a view comes to
//! [`Plugin::scene`](crate::Plugin::scene), and while [`animate`] is
//! on, each frame comes to [`Plugin::tick`](crate::Plugin::tick).
//!
//! ```ignore
//! let at = Entity::make(None, CAMERA)?;
//! at.set_transform(Transform::looking_at((0.0, 4.0, 8.0), (0.0, 0.0, 0.0)));
//! let camera = Camera::make(&at, &CameraSettings::default(), Target::Viewport)?;
//! let cube = Entity::make(None, CUBE)?;
//! cube.draw(&Mesh::shape(Shape::Cuboid((1.0, 1.0, 1.0)))?, &Material::make(Pbr::colored(RED))?);
//! ```

use core::any::Any;

pub use crate::bindings::exports::hamlet::engine::scene_guest::{
    Hit, MouseButton, Pointer, Press, SceneInput, Wheel,
};
pub use crate::bindings::hamlet::engine::scene::{
    AlphaMode, Bloom, Camera, CameraSettings, Decal,
    DirectionalLight, Entity, Fog, FogFalloff, Instances, Light,
    Material, Mesh, MeshData, Pbr, PointLight, Projection, Quat,
    Rgba, Shape, SpotLight, Target, Tonemapping, Transform, Vec3,
};
/// A plugin's own GPU work in WGSL, checked by the engine before use:
/// a material's `shade` and `displace`, a decal's `shade`, a camera's
/// `post` pass, and compute's `main`, over the engine's bindings.
/// Each make fails, saying why, when the source is refused. A texture
/// also gives a decal its picture.
pub mod gpu {
    pub use crate::bindings::hamlet::engine::gpu::{
        Buffer, Compute, Shaded, Shader, ShaderKind, Texture,
        decal_material, set_params, set_pass, shaded_decal,
        shaded_material,
    };
}

use crate::ui::{AnyView, Cx, World, column};

/// Has the engine call [`Plugin::tick`](crate::Plugin::tick) each
/// frame while `on`.
pub fn animate(on: bool) {
    crate::bindings::hamlet::engine::scene::animate(on);
}

/// A frame of a panel showing the image of the camera `camera` finds
/// in the resource `R`, filling the room the layout gives it.
pub fn viewport<R: Any>(
    camera: fn(&R) -> Option<&Camera>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let key = cx.build(column(()));
        let world: &World = cx.world;
        if let (Some(camera), Some(node)) =
            (world.get::<R>().and_then(camera), world.nodes.node(key))
        {
            camera.show_in(node);
        }
        key
    })
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: (0.0, 0.0, 0.0),
            rotation: (0.0, 0.0, 0.0, 1.0),
            scale: (1.0, 1.0, 1.0),
        }
    }
}

impl Transform {
    pub fn at(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::default()
        }
    }

    /// At `eye`, its forward (-z) turned to `target`, its up toward
    /// +y.
    pub fn looking_at(eye: Vec3, target: Vec3) -> Self {
        let forward = normalize(sub(target, eye));
        let right = normalize(cross(forward, (0.0, 1.0, 0.0)));
        let up = cross(right, forward);
        let back = (-forward.0, -forward.1, -forward.2);
        Self {
            translation: eye,
            rotation: from_axes(right, up, back),
            scale: (1.0, 1.0, 1.0),
        }
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    (
        a.1 * b.2 - a.2 * b.1,
        a.2 * b.0 - a.0 * b.2,
        a.0 * b.1 - a.1 * b.0,
    )
}

fn normalize(a: Vec3) -> Vec3 {
    let length = (a.0 * a.0 + a.1 * a.1 + a.2 * a.2).sqrt();
    if length > 0.0 {
        (a.0 / length, a.1 / length, a.2 / length)
    } else {
        (0.0, 0.0, -1.0)
    }
}

/// The rotation whose x, y and z axes are `x`, `y` and `z`.
fn from_axes(x: Vec3, y: Vec3, z: Vec3) -> Quat {
    let trace = x.0 + y.1 + z.2;
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        ((y.2 - z.1) / s, (z.0 - x.2) / s, (x.1 - y.0) / s, s / 4.0)
    } else if x.0 > y.1 && x.0 > z.2 {
        let s = (1.0 + x.0 - y.1 - z.2).sqrt() * 2.0;
        (s / 4.0, (y.0 + x.1) / s, (z.0 + x.2) / s, (y.2 - z.1) / s)
    } else if y.1 > z.2 {
        let s = (1.0 + y.1 - x.0 - z.2).sqrt() * 2.0;
        ((y.0 + x.1) / s, s / 4.0, (z.1 + y.2) / s, (z.0 - x.2) / s)
    } else {
        let s = (1.0 + z.2 - x.0 - y.1).sqrt() * 2.0;
        ((z.0 + x.2) / s, (z.1 + y.2) / s, s / 4.0, (x.1 - y.0) / s)
    }
}

impl Pbr {
    /// A material of `color`, rough and not metal, lit and fogged.
    pub fn colored(color: Rgba) -> Self {
        Self {
            base_color: color,
            emissive: (0.0, 0.0, 0.0, 1.0),
            roughness: 0.5,
            metallic: 0.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            unlit: false,
            double_sided: false,
            fog: true,
        }
    }
}

impl Default for CameraSettings {
    /// A perspective of 45 degrees from 0.1 to 1000, clearing to
    /// black, in high range, neither fogged nor blooming.
    fn default() -> Self {
        Self {
            projection: Projection::Perspective((
                core::f32::consts::FRAC_PI_4,
                0.1,
                1000.0,
            )),
            clear: Some((0.0, 0.0, 0.0, 1.0)),
            hdr: true,
            tonemapping: Tonemapping::Neutral,
            exposure: 9.7,
            fog: None,
            bloom: None,
            ambient: ((1.0, 1.0, 1.0, 1.0), 80.0),
            msaa: 4,
        }
    }
}

/// Ten floats for each of `copies`, as [`Instances::set`] takes them.
pub fn copies(copies: &[Transform]) -> Vec<f32> {
    copies
        .iter()
        .flat_map(|copy| {
            let (x, y, z) = copy.translation;
            let (rx, ry, rz, rw) = copy.rotation;
            let (sx, sy, sz) = copy.scale;
            [x, y, z, rx, ry, rz, rw, sx, sy, sz]
        })
        .collect()
}

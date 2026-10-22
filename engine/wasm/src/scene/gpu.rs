//! A plugin's own GPU work, as it makes and changes it in a call:
//! each shader checked with naga before it is kept, and each thing
//! held to the budgets of GPU memory and objects and to the render
//! capabilities.

use hamlet_engine_render::{
    PARAMS, SceneChange, ShaderKind, ShaderRules, check,
};
use wasmtime::component::Resource;

use crate::instance::State;
use crate::scene::scene_bindings::hamlet::engine::gpu::{
    AlphaMode, Decal, Host, HostBuffer, HostCompute, HostShader,
    HostTexture, Shaded, ShaderKind as Kind,
};
use crate::scene::{
    CameraHandle, Held, MaterialHandle, MaterialKind, Powers,
    SceneError, SceneState, convert, trap,
};

pub struct ShaderHandle;
pub struct BufferHandle;
pub struct TextureHandle;
pub struct ComputeHandle;

/// The cutoff of a shaded material whose alpha mode masks.
const CUTOFF: f32 = 0.5;

impl SceneState {
    /// Whether one more GPU object of `bytes` fits; why not
    /// otherwise.
    fn room_gpu(&self, bytes: u64) -> Result<(), String> {
        let most = self.caps.gpu_objects;
        if self.counts.gpu_objects >= most {
            return Err(format!(
                "a plugin holds {most} GPU objects at most"
            ));
        }
        let memory = self.caps.gpu_memory;
        if self.counts.gpu_bytes.saturating_add(bytes) > memory {
            return Err(format!(
                "a plugin's GPU memory is {memory} bytes at most"
            ));
        }
        Ok(())
    }

    /// Whether the plugin may make what both kinds of its GPU work
    /// use.
    fn allowed_either(&mut self) -> Result<(), String> {
        if self.powers.has(Powers::COMPUTE) {
            return Ok(());
        }
        self.allowed(Powers::SHADER, "render.shader")
    }

    /// Numbers a GPU object of `bytes`, `held`, made by `change`.
    fn make_gpu<T: 'static>(
        &mut self,
        held: Held,
        bytes: u64,
        change: impl FnOnce(u32) -> SceneChange,
    ) -> wasmtime::Result<Resource<T>> {
        self.spend(1)?;
        self.counts.gpu_objects += 1;
        self.counts.gpu_bytes += bytes;
        let id = self.number(held);
        self.changes.push(change(id));
        Ok(Resource::new_own(id))
    }

    /// The shader of `id`, which is to be of `kind`.
    fn shader(
        &self,
        id: u32,
        kind: ShaderKind,
    ) -> wasmtime::Result<Result<(), String>> {
        match self.held(id, |held| matches!(held, Held::Shader(_)))? {
            Held::Shader(held) if held == kind => Ok(Ok(())),
            _ => Ok(Err(format!(
                "the shader is not a {kind:?} shader"
            ))),
        }
    }
}

/// `params` as a shader is given them, or why they are refused.
fn params(params: &[f32]) -> Result<(), String> {
    let most = PARAMS * 4;
    if params.len() > most {
        return Err(format!(
            "a shader is given {most} params at most"
        ));
    }
    if params.iter().any(|value| !value.is_finite()) {
        return Err("a param is not finite".into());
    }
    Ok(())
}

impl HostShader for State {
    fn compile(
        &mut self,
        kind: Kind,
        source: String,
    ) -> wasmtime::Result<Result<Resource<ShaderHandle>, String>>
    {
        self.keep_bytes(source.len())?;
        let (kind, power, name) = match kind {
            Kind::Material => (
                ShaderKind::Material,
                Powers::SHADER,
                "render.shader",
            ),
            Kind::Pass => {
                (ShaderKind::Pass, Powers::SHADER, "render.shader")
            }
            Kind::Compute => (
                ShaderKind::Compute,
                Powers::COMPUTE,
                "render.compute",
            ),
            Kind::Decal => {
                (ShaderKind::Decal, Powers::SHADER, "render.shader")
            }
        };
        let scene = &mut self.scene;
        if let Err(why) = scene
            .allowed(power, name)
            .and_then(|()| scene.room_gpu(0))
        {
            return Ok(Err(why));
        }
        let rules =
            ShaderRules::of(&scene.gpu, scene.caps.shader_bytes);
        let shader = match check(kind, &source, &rules) {
            Ok(shader) => shader,
            Err(why) => return Ok(Err(why)),
        };
        let made = scene.make_gpu(Held::Shader(kind), 0, |id| {
            SceneChange::Shader { id, shader }
        })?;
        Ok(Ok(made))
    }

    fn drop(
        &mut self,
        shader: Resource<ShaderHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(shader.rep())
    }
}

impl HostBuffer for State {
    fn make(
        &mut self,
        size: u32,
    ) -> wasmtime::Result<Result<Resource<BufferHandle>, String>>
    {
        let scene = &mut self.scene;
        let bytes = u64::from(size);
        let largest = scene.gpu.buffer_size;
        let refused = scene
            .allowed_either()
            .and_then(|()| scene.room_gpu(bytes))
            .and_then(|()| {
                (bytes <= largest).then_some(()).ok_or_else(|| {
                    format!("a buffer is {largest} bytes at most")
                })
            })
            .and_then(|()| {
                (size > 0 && size.is_multiple_of(16))
                    .then_some(())
                    .ok_or_else(|| {
                        "a buffer is a positive multiple of 16 bytes"
                            .to_owned()
                    })
            });
        if let Err(why) = refused {
            return Ok(Err(why));
        }
        let made =
            scene.make_gpu(Held::Buffer { bytes }, bytes, |id| {
                SceneChange::Buffer { id, size }
            })?;
        Ok(Ok(made))
    }

    fn write(
        &mut self,
        buffer: Resource<BufferHandle>,
        offset: u32,
        bytes: Vec<u8>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(bytes.len())?;
        let id = buffer.rep();
        let Held::Buffer { bytes: size } = self
            .scene
            .held(id, |held| matches!(held, Held::Buffer { .. }))?
        else {
            return Err(trap(SceneError::Unknown));
        };
        let end = u64::from(offset) + bytes.len() as u64;
        if !offset.is_multiple_of(4)
            || !bytes.len().is_multiple_of(4)
            || end > size
        {
            return Ok(Err(format!(
                "a write is of 4-byte steps within the buffer's \
                 {size} bytes"
            )));
        }
        self.scene.change(SceneChange::Write {
            id,
            offset,
            bytes,
        })?;
        Ok(Ok(()))
    }

    fn drop(
        &mut self,
        buffer: Resource<BufferHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(buffer.rep())
    }
}

impl State {
    /// Why `pixels` are not a `width` by `height` texture, if they
    /// are not one.
    fn picture(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), String> {
        let most = self.scene.gpu.texture_size;
        if width == 0 || height == 0 || width > most || height > most
        {
            return Err(format!(
                "a texture is 1 to {most} pixels wide and tall"
            ));
        }
        let wanted = u64::from(width) * u64::from(height) * 4;
        if pixels.len() as u64 != wanted {
            return Err(format!(
                "a {width} by {height} texture is {wanted} bytes"
            ));
        }
        Ok(())
    }
}

impl HostTexture for State {
    fn make(
        &mut self,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> wasmtime::Result<Result<Resource<TextureHandle>, String>>
    {
        self.keep_bytes(pixels.len())?;
        let bytes = pixels.len() as u64;
        let refused = self
            .picture(width, height, &pixels)
            .and_then(|()| self.scene.allowed_either())
            .and_then(|()| self.scene.room_gpu(bytes));
        if let Err(why) = refused {
            return Ok(Err(why));
        }
        let made = self.scene.make_gpu(
            Held::Texture { width, height },
            bytes,
            |id| SceneChange::Texture {
                id,
                width,
                height,
                pixels,
            },
        )?;
        Ok(Ok(made))
    }

    fn write(
        &mut self,
        texture: Resource<TextureHandle>,
        pixels: Vec<u8>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(pixels.len())?;
        let id = texture.rep();
        let Held::Texture { width, height } = self
            .scene
            .held(id, |held| matches!(held, Held::Texture { .. }))?
        else {
            return Err(trap(SceneError::Unknown));
        };
        if let Err(why) = self.picture(width, height, &pixels) {
            return Ok(Err(why));
        }
        self.scene.change(SceneChange::Texture {
            id,
            width,
            height,
            pixels,
        })?;
        Ok(Ok(()))
    }

    fn drop(
        &mut self,
        texture: Resource<TextureHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(texture.rep())
    }
}

impl SceneState {
    /// Numbers a material of `kind` drawn by `shader` with `floats`,
    /// `texture` and `data`, made by `change`, if the plugin may make
    /// it; why not otherwise.
    fn make_shaded(
        &mut self,
        kind: MaterialKind,
        shader: u32,
        floats: Vec<f32>,
        [texture, data]: [Option<u32>; 2],
        change: impl FnOnce(u32, Vec<f32>) -> SceneChange,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        if let Some(texture) = texture {
            self.held(texture, |held| {
                matches!(held, Held::Texture { .. })
            })?;
        }
        if let Some(data) = data {
            self.held(data, |held| {
                matches!(held, Held::Buffer { .. })
            })?;
        }
        let shader_kind = match kind {
            MaterialKind::ShadedDecal => ShaderKind::Decal,
            _ => ShaderKind::Material,
        };
        let of_kind = self.shader(shader, shader_kind)?;
        let refused = self
            .allowed(Powers::SHADER, "render.shader")
            .and(of_kind)
            .and_then(|()| params(&floats))
            .and_then(|()| self.room_material());
        if let Err(why) = refused {
            return Ok(Err(why));
        }
        self.spend(1)?;
        self.counts.materials += 1;
        let id = self.number(Held::Material {
            kind,
            uses: [Some(shader), texture, data],
        });
        self.changes.push(change(id, floats));
        Ok(Ok(Resource::new_own(id)))
    }
}

impl Host for State {
    fn shaded_material(
        &mut self,
        shader: Resource<ShaderHandle>,
        settings: Shaded,
        floats: Vec<f32>,
        texture: Option<Resource<TextureHandle>>,
        data: Option<Resource<BufferHandle>>,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        self.keep_bytes(size_of_val(floats.as_slice()))?;
        let shader = shader.rep();
        let texture = texture.map(|texture| texture.rep());
        let data = data.map(|data| data.rep());
        self.scene.make_shaded(
            MaterialKind::Shaded,
            shader,
            floats,
            [texture, data],
            |id, params| SceneChange::Shaded {
                id,
                shader,
                alpha: convert::alpha(settings.alpha_mode, CUTOFF),
                double_sided: settings.double_sided,
                lit: settings.lit,
                params,
                texture,
                data,
            },
        )
    }

    fn shaded_decal(
        &mut self,
        shader: Resource<ShaderHandle>,
        alpha_mode: AlphaMode,
        floats: Vec<f32>,
        texture: Option<Resource<TextureHandle>>,
        data: Option<Resource<BufferHandle>>,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        self.keep_bytes(size_of_val(floats.as_slice()))?;
        if matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Mask) {
            return Ok(Err("a decal's alpha mode is one that \
                           blends"
                .to_owned()));
        }
        let shader = shader.rep();
        let texture = texture.map(|texture| texture.rep());
        let data = data.map(|data| data.rep());
        self.scene.make_shaded(
            MaterialKind::ShadedDecal,
            shader,
            floats,
            [texture, data],
            |id, params| SceneChange::ShadedDecal {
                id,
                shader,
                alpha: convert::alpha(alpha_mode, CUTOFF),
                params,
                texture,
                data,
            },
        )
    }

    fn set_params(
        &mut self,
        material: Resource<MaterialHandle>,
        floats: Vec<f32>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(size_of_val(floats.as_slice()))?;
        let id = material.rep();
        self.scene.held(id, |held| {
            matches!(
                held,
                Held::Material {
                    kind: MaterialKind::Shaded
                        | MaterialKind::ShadedDecal,
                    ..
                }
            )
        })?;
        if let Err(why) = params(&floats) {
            return Ok(Err(why));
        }
        self.scene
            .change(SceneChange::Params { id, params: floats })?;
        Ok(Ok(()))
    }

    fn decal_material(
        &mut self,
        decal: Decal,
        texture: Resource<TextureHandle>,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        self.keep_bytes(size_of::<Decal>())?;
        self.scene.held(texture.rep(), |held| {
            matches!(held, Held::Texture { .. })
        })?;
        let made = convert::decal(decal)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.make_decal(made, Some(texture.rep()))
    }

    fn set_pass(
        &mut self,
        camera: Resource<CameraHandle>,
        shader: Option<Resource<ShaderHandle>>,
        floats: Vec<f32>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(size_of_val(floats.as_slice()))?;
        let id = camera.rep();
        let Held::Camera { background, pass } = self
            .scene
            .held(id, |held| matches!(held, Held::Camera { .. }))?
        else {
            return Err(trap(SceneError::Unknown));
        };
        let scene = &mut self.scene;
        let shader = shader.map(|shader| shader.rep());
        if let Some(shader) = shader {
            let refused = scene
                .allowed(Powers::SHADER, "render.shader")
                .and_then(|()| params(&floats))
                .and_then(|()| {
                    if pass.is_some() {
                        Ok(())
                    } else {
                        scene.room_gpu(0)
                    }
                });
            if let Err(why) = refused {
                return Ok(Err(why));
            }
            if let Err(why) =
                scene.shader(shader, ShaderKind::Pass)?
            {
                return Ok(Err(why));
            }
        }
        scene.spend(1)?;
        scene.counts.gpu_objects = scene.counts.gpu_objects
            + usize::from(shader.is_some())
            - usize::from(pass.is_some());
        scene.keep(
            id,
            Held::Camera {
                background,
                pass: shader,
            },
        );
        scene.changes.push(SceneChange::Pass {
            camera: id,
            shader,
            params: floats,
        });
        Ok(Ok(()))
    }
}

impl HostCompute for State {
    fn make(
        &mut self,
        shader: Resource<ShaderHandle>,
        data: Resource<BufferHandle>,
        extra: Option<Resource<BufferHandle>>,
    ) -> wasmtime::Result<Result<Resource<ComputeHandle>, String>>
    {
        let scene = &mut self.scene;
        let buffer =
            |held: &Held| matches!(held, Held::Buffer { .. });
        scene.held(data.rep(), buffer)?;
        if let Some(extra) = &extra {
            scene.held(extra.rep(), buffer)?;
        }
        if let Err(why) = scene
            .allowed(Powers::COMPUTE, "render.compute")
            .and_then(|()| scene.room_gpu(0))
        {
            return Ok(Err(why));
        }
        if let Err(why) =
            scene.shader(shader.rep(), ShaderKind::Compute)?
        {
            return Ok(Err(why));
        }
        let (shader, data) = (shader.rep(), data.rep());
        let extra = extra.map(|extra| extra.rep());
        let uses = [Some(shader), Some(data), extra];
        let made =
            scene.make_gpu(Held::Compute { uses }, 0, |id| {
                SceneChange::Compute {
                    id,
                    shader,
                    data,
                    extra,
                }
            })?;
        Ok(Ok(made))
    }

    fn set_params(
        &mut self,
        compute: Resource<ComputeHandle>,
        floats: Vec<f32>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(size_of_val(floats.as_slice()))?;
        let id = compute.rep();
        self.scene.held(id, Held::compute)?;
        if let Err(why) = params(&floats) {
            return Ok(Err(why));
        }
        self.scene
            .change(SceneChange::Params { id, params: floats })?;
        Ok(Ok(()))
    }

    fn dispatch(
        &mut self,
        compute: Resource<ComputeHandle>,
        x: u32,
        y: u32,
        z: u32,
        every_frame: bool,
    ) -> wasmtime::Result<Result<(), String>> {
        let id = compute.rep();
        self.scene.held(id, Held::compute)?;
        let most = self.scene.caps.workgroups;
        let groups = u64::from(x) * u64::from(y) * u64::from(z);
        if groups > u64::from(most) {
            return Ok(Err(format!(
                "{groups} workgroups, the most a frame runs is \
                 {most}"
            )));
        }
        let widest = self.scene.gpu.groups;
        if x.max(y).max(z) > widest {
            return Ok(Err(format!(
                "{x} by {y} by {z} workgroups, the most along each \
                 is {widest}"
            )));
        }
        self.scene.change(SceneChange::Dispatch {
            id,
            groups: [x, y, z],
            every_frame,
        })?;
        Ok(Ok(()))
    }

    fn drop(
        &mut self,
        compute: Resource<ComputeHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(compute.rep())
    }
}

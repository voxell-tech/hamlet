//! The pictures of plugins' image nodes: measured as a call hands
//! them over, decoded off the frame, and drawn in their image's
//! extent by its fit.

use std::io::Cursor;

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::{Rect, UVec2, Vec2};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{Node, px};
use image::{ImageFormat, ImageReader, Limits};

use crate::instance::ui_bindings::hamlet::engine::ui::Fit;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        decoded.run_if(resource_exists::<Assets<Image>>),
    );
}

/// On the holder of an image node: the picture it was given, being
/// decoded off the frame.
#[derive(Component)]
pub(crate) struct Decoding(Task<Option<Image>>);

impl Decoding {
    /// `bytes` decoded off the frame, refused past `side` pixels a
    /// side.
    pub(crate) fn start(bytes: Vec<u8>, side: u32) -> Self {
        Self(
            AsyncComputeTaskPool::get()
                .spawn(async move { decode(&bytes, side) }),
        )
    }
}

/// On the holder of an image node: its picture, decoded, and its size
/// in pixels.
#[derive(Component, Clone, PartialEq, Debug)]
pub(crate) struct Picture {
    image: Handle<Image>,
    pub(crate) size: UVec2,
}

/// On the entity drawing a plugin's picture: its size in pixels.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct ShownPicture(pub UVec2);

/// The width and height of the picture in `bytes`, read from its
/// header.
pub(crate) fn measure(bytes: &[u8]) -> Result<(u32, u32), String> {
    reader(bytes)?
        .into_dimensions()
        .map_err(|why| why.to_string())
}

/// A reader of `bytes`, a PNG, JPEG or WebP.
fn reader(
    bytes: &[u8],
) -> Result<ImageReader<Cursor<&[u8]>>, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|why| why.to_string())?;
    match reader.format() {
        Some(
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP,
        ) => Ok(reader),
        _ => Err("it is not a PNG, JPEG or WebP".into()),
    }
}

fn decode(bytes: &[u8], side: u32) -> Option<Image> {
    let mut reader = reader(bytes).ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(side);
    limits.max_image_height = Some(side);
    reader.limits(limits);
    let pixels = reader.decode().ok()?.into_rgba8();
    let (width, height) = pixels.dimensions();
    Some(Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

/// Gives each image node whose picture is decoded its picture, in
/// place of the one it had.
fn decoded(
    mut holders: Query<(Entity, &mut Decoding)>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    for (holder, mut decoding) in &mut holders {
        let Some(image) = check_ready(&mut decoding.0) else {
            continue;
        };
        let mut holder = commands.entity(holder);
        holder.try_remove::<Decoding>();
        match image {
            Some(image) => {
                let size = image.size();
                holder.try_insert(Picture {
                    image: images.add(image),
                    size,
                });
            }
            None => {
                holder.try_remove::<Picture>();
            }
        }
    }
}

/// Where a picture is drawn in its image: the image's size, the
/// picture's within it, and the part of the picture shown, in the
/// interface's pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Laid {
    outer: Vec2,
    inner: Vec2,
    rect: Option<Rect>,
}

impl Laid {
    /// A picture of `size` pixels, or none, in an image of `extent`
    /// by `fit`.
    pub(crate) fn of(
        size: Option<UVec2>,
        extent: Vec2,
        fit: Fit,
    ) -> Self {
        let size = size.map_or(Vec2::ZERO, |size| size.as_vec2());
        if size.min_element() <= 0.0 {
            return Self {
                outer: extent,
                inner: Vec2::ZERO,
                rect: None,
            };
        }
        let outer = match (extent.x > 0.0, extent.y > 0.0) {
            (true, true) => extent,
            (true, false) => {
                Vec2::new(extent.x, extent.x * size.y / size.x)
            }
            (false, true) => {
                Vec2::new(extent.y * size.x / size.y, extent.y)
            }
            (false, false) => size,
        };
        let scales = outer / size;
        match fit {
            Fit::Contain => Self {
                outer,
                inner: size * scales.min_element(),
                rect: None,
            },
            Fit::Cover => {
                let seen = outer / scales.max_element();
                let corner = (size - seen) / 2.0;
                Self {
                    outer,
                    inner: outer,
                    rect: Some(Rect::from_corners(
                        corner,
                        corner + seen,
                    )),
                }
            }
        }
    }

    /// Lays out the image `frame` and `shown`, the entity under it
    /// drawing `picture`.
    pub(crate) fn draw(
        self,
        world: &mut World,
        frame: Entity,
        shown: Entity,
        picture: Option<&Picture>,
    ) {
        if let Some(mut node) = world.get_mut::<Node>(frame) {
            node.width = px(self.outer.x);
            node.height = px(self.outer.y);
            node.flex_shrink = 0.0;
        }
        let Ok(mut entity) = world.get_entity_mut(shown) else {
            return;
        };
        if let Some(mut node) = entity.get_mut::<Node>() {
            node.width = px(self.inner.x);
            node.height = px(self.inner.y);
        }
        if let Some(mut image) = entity.get_mut::<ImageNode>() {
            image.image = picture.map_or_else(
                || ImageNode::default().image,
                |picture| picture.image.clone(),
            );
            image.rect = self.rect;
            image.image_mode = NodeImageMode::Stretch;
        }
        match picture {
            Some(picture) => {
                entity.insert(ShownPicture(picture.size));
            }
            None => {
                entity.remove::<ShownPicture>();
            }
        }
    }
}

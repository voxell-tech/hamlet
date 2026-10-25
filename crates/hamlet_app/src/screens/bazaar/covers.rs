//! The covers of the bazaar's listings: fetched from its server for
//! the page that shows one, kept while a few are, and drawn as Hamlet
//! draws a picture of a reply. And the cover chosen at the seller's
//! desk, read from a file of this computer or fetched from the web.

use std::collections::VecDeque;
use std::io::Cursor;

use bevy::app::App;
use bevy::asset::Assets;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::UVec2;
use image::{ImageFormat, ImageReader};

use crate::bazaar::errand::{self, Errand, Fetched};
use crate::bazaar::market::client::bytes;
use crate::bazaar::market::{self, Call, Market};
use crate::pictures::{Drawn, decode, image};

/// The most covers kept.
const MOST: usize = 24;

/// The most bytes a cover may be, as the bazaar's server takes it.
pub(crate) const MOST_BYTES: usize = 400 * 1024;

/// The most pixels a side of a cover may be.
pub(crate) const MOST_SIDE: u32 = 2048;

/// The pixels a cover is kept at, at most: twice the most it is
/// drawn at, for a dense screen.
const KEPT: UVec2 = UVec2::new(1280, 720);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Covers>().init_resource::<Pick>();
}

/// The covers kept, by their address on the server, which changes
/// with each new cover; the newest last.
#[derive(Resource, Default)]
pub(crate) struct Covers {
    held: VecDeque<(String, Held)>,
}

/// A cover asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Held {
    Fetching,
    Shown(Drawn),
    /// No picture came of it.
    Failed,
}

impl Covers {
    /// The cover at `url`, if it was asked for.
    pub fn get(&self, url: &str) -> Option<&Held> {
        self.held
            .iter()
            .find(|(held, _)| held == url)
            .map(|(_, held)| held)
    }

    fn put(&mut self, url: &str, held: Held) {
        self.held.retain(|(kept, _)| kept != url);
        self.held.push_back((url.to_string(), held));
        while self.held.len() > MOST {
            self.held.pop_front();
        }
    }
}

/// Asks for the cover at `url` on the bazaar's server, unless it was.
pub(crate) fn want(world: &mut World, url: &str) {
    if world.resource::<Covers>().get(url).is_some() {
        return;
    }
    world.resource_mut::<Covers>().put(url, Held::Fetching);
    let request = world.resource::<Market>().server.picture(url);
    market::send(world, Call::Picture(url.to_string()), request);
}

/// Keeps the cover at `url` that came.
pub(crate) fn came(world: &mut World, url: String, answer: Fetched) {
    let held = match bytes(answer).ok().and_then(|b| drawn(world, &b))
    {
        Some(drawn) => Held::Shown(drawn),
        None => Held::Failed,
    };
    world.resource_mut::<Covers>().put(&url, held);
}

/// `bytes` drawn as an image, if they are a picture.
fn drawn(world: &mut World, bytes: &[u8]) -> Option<Drawn> {
    let pixels = decode(bytes, KEPT)?;
    let size = pixels.size;
    let mut images = world.get_resource_mut::<Assets<Image>>()?;
    Some(Drawn {
        image: images.add(image(pixels)),
        size,
    })
}

/// The picture chosen at the desk as a cover, and where it came from.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Pick {
    /// The path of a file, or the address of a picture on the web.
    pub from: String,
    pub state: Picked,
}

/// How choosing a cover at the desk goes.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) enum Picked {
    #[default]
    Nothing,
    Fetching,
    Got(Picture),
    Failed(String),
}

/// A picture that may be a cover: its bytes, its media type and its
/// size, and how it is drawn.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Picture {
    pub bytes: Vec<u8>,
    pub media: &'static str,
    pub size: UVec2,
    pub drawn: Option<Drawn>,
}

/// Chooses the picture at `from` as the desk's cover: a file of this
/// computer, or an address on the web; nothing when it is empty.
pub(crate) fn pick(world: &mut World, from: String) {
    let from = from.trim().to_string();
    let web =
        from.starts_with("https://") || from.starts_with("http://");
    let state = if from.is_empty() {
        Picked::Nothing
    } else if web {
        if !errand::send(
            world,
            from.clone(),
            Errand::Picture(from.clone()),
        ) {
            Picked::Failed("Nothing here can fetch it.".into())
        } else {
            Picked::Fetching
        }
    } else {
        let path = from.strip_prefix("file://").unwrap_or(&from);
        match std::fs::read(path) {
            Ok(bytes) => taken(world, bytes),
            Err(why) => {
                Picked::Failed(format!("It cannot be read: {why}."))
            }
        }
    };
    *world.resource_mut::<Pick>() = Pick { from, state };
}

/// Takes in the picture fetched from `url` for the desk, if it is
/// still the one chosen.
pub(crate) fn picked(world: &mut World, url: &str, answer: Fetched) {
    if world.resource::<Pick>().from != url {
        return;
    }
    let state = match answer {
        Ok((200..=299, body)) => taken(world, body),
        Ok((status, _)) => {
            Picked::Failed(format!("Its address answered {status}."))
        }
        Err(why) => {
            Picked::Failed(format!("It could not be fetched: {why}."))
        }
    };
    world.resource_mut::<Pick>().state = state;
}

/// `bytes` as a cover, or why they cannot be one.
fn taken(world: &mut World, bytes: Vec<u8>) -> Picked {
    match check(&bytes) {
        Ok((media, size)) => {
            let drawn = drawn(world, &bytes);
            Picked::Got(Picture {
                bytes,
                media,
                size,
                drawn,
            })
        }
        Err(why) => Picked::Failed(why),
    }
}

/// The media type and size of the picture in `bytes`, or why the
/// bazaar's server would not take it as a cover.
pub(crate) fn check(
    bytes: &[u8],
) -> Result<(&'static str, UVec2), String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "It is not a picture.".to_string())?;
    let media = match reader.format() {
        Some(ImageFormat::Png) => "image/png",
        Some(ImageFormat::Jpeg) => "image/jpeg",
        Some(ImageFormat::WebP) => "image/webp",
        _ => return Err("It is not a PNG, JPEG or WebP.".into()),
    };
    if bytes.len() > MOST_BYTES {
        return Err(format!(
            "It is {} KB, over the {} KB a cover may be.",
            bytes.len().div_ceil(1024),
            MOST_BYTES / 1024
        ));
    }
    let (width, height) = reader.into_dimensions().map_err(|_| {
        "It is not a picture that can be read.".to_string()
    })?;
    if width.max(height) > MOST_SIDE {
        return Err(format!(
            "It is {width} by {height} pixels, over the {MOST_SIDE} \
             a side a cover may be."
        ));
    }
    Ok((media, UVec2::new(width, height)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::new(width, height)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn a_cover_is_a_small_enough_png_jpeg_or_webp() {
        let (media, size) = check(&png(64, 36)).unwrap();
        assert_eq!(media, "image/png");
        assert_eq!(size, UVec2::new(64, 36));
        assert!(check(&png(2049, 4)).unwrap_err().contains("2048"));
        assert!(check(b"not a picture").is_err());
    }

    #[test]
    fn the_covers_kept_are_the_newest() {
        let mut covers = Covers::default();
        for at in 0..=MOST {
            covers.put(&format!("/c/{at}"), Held::Failed);
        }
        assert!(covers.get("/c/0").is_none());
        assert_eq!(
            covers.get(&format!("/c/{MOST}")),
            Some(&Held::Failed)
        );
    }
}

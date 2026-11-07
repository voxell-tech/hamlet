//! Images a chief attaches to a line (C33): uploaded in parts, kept
//! by the server under an id, and read back by that id.

use std::collections::HashMap;

use base64::Engine;
use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_protocol::client as protocol;
use protocol::ImageId;

use crate::messages::{Reconnected, Response};
use crate::outbox::{Outbox, RequestId};

/// The most bytes of an image sent in one part.
const PART: usize = 512 * 1024;

/// The endings of the files an image is taken in (C33), in small
/// letters.
pub const ENDINGS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

/// Whether the name of `file` says it is an image that is taken.
pub fn is_image_file(file: &std::path::Path) -> bool {
    file.extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            ENDINGS.contains(&ending.to_ascii_lowercase().as_str())
        })
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Steps>()
        .add_observer(upload)
        .add_observer(download)
        .add_observer(
            |response: On<Response>, mut commands: Commands| {
                let response = response.event().clone();
                commands.queue(move |world: &mut World| {
                    answered(world, response);
                });
            },
        )
        .add_observer(lost);
}

/// A request to upload the image file whose bytes these are. `key`
/// is the caller's name for it, which the answer carries.
#[derive(Event, Clone, Debug)]
pub struct Upload {
    pub key: u64,
    pub bytes: Vec<u8>,
}

/// The server kept the image uploaded for `key` under `image`.
#[derive(Event, Clone, Debug)]
pub struct ImageKept {
    pub key: u64,
    pub image: ImageId,
}

/// The server took no image from the upload for `key`, or the
/// connection was lost on the way.
#[derive(Event, Clone, Debug)]
pub struct ImageRefused {
    pub key: u64,
    pub text: String,
}

/// A request for the bytes of the image the server keeps as `image`,
/// as `size` says.
#[derive(Event, Clone, Debug)]
pub struct Download {
    pub image: ImageId,
    pub size: protocol::ImageSize,
}

/// The bytes of an image that was asked for, with its media type.
#[derive(Event, Clone, Debug)]
pub struct ImageArrived {
    pub image: ImageId,
    pub size: protocol::ImageSize,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// An image that was asked for and could not be had.
#[derive(Event, Clone, Debug)]
pub struct ImageMissing {
    pub image: ImageId,
    pub size: protocol::ImageSize,
}

/// The step of each request the images wait on.
#[derive(Resource, Default)]
struct Steps(HashMap<RequestId, Step>);

enum Step {
    /// The upload is announced; the server has not named it yet.
    Begin {
        key: u64,
        bytes: Vec<u8>,
    },
    /// The part of the upload at `offset` is sent.
    Part {
        key: u64,
        upload: String,
        bytes: Vec<u8>,
        offset: usize,
    },
    Download(ImageId, protocol::ImageSize),
}

fn upload(
    upload: On<Upload>,
    mut steps: ResMut<Steps>,
    mut outbox: ResMut<Outbox>,
    mut commands: Commands,
) {
    let Upload { key, bytes } = upload.event().clone();
    if bytes.is_empty() {
        commands.trigger(ImageRefused {
            key,
            text: "The image has no bytes.".into(),
        });
        return;
    }
    let size = bytes.len() as u64;
    let id = outbox.send(|id| {
        protocol::ClientMessage::PutImage(protocol::PutImage {
            id,
            size,
        })
    });
    steps.0.insert(id, Step::Begin { key, bytes });
}

fn download(
    request: On<Download>,
    mut steps: ResMut<Steps>,
    mut outbox: ResMut<Outbox>,
) {
    let (image, size) = (request.image.clone(), request.size);
    let id = outbox.send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Image {
            id,
            image: image.clone(),
            size: Some(size),
        })
    });
    steps.0.insert(id, Step::Download(image, size));
}

/// Sends the part of `bytes` that starts at `offset`.
fn send_part(
    world: &mut World,
    key: u64,
    upload: String,
    bytes: Vec<u8>,
    offset: usize,
) {
    let end = (offset + PART).min(bytes.len());
    let data = base64::engine::general_purpose::STANDARD
        .encode(&bytes[offset..end]);
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::PutImagePart(
            protocol::PutImagePart {
                data,
                id,
                offset: offset as u64,
                upload: upload.clone(),
            },
        )
    });
    world.resource_mut::<Steps>().0.insert(
        id,
        Step::Part {
            key,
            upload,
            bytes,
            offset: end,
        },
    );
}

/// The words of a refusal or fault, none for what is no refusal.
fn refusal(message: &protocol::ServerMessage) -> Option<String> {
    match message {
        protocol::ServerMessage::Fault(fault) => {
            Some(fault.text.clone())
        }
        protocol::ServerMessage::Reply(reply)
            if reply.kind != protocol::ReplyKind::Notice =>
        {
            Some(reply.text.clone())
        }
        _ => None,
    }
}

fn answered(world: &mut World, response: Response) {
    let Some(step) =
        world.resource_mut::<Steps>().0.remove(&response.to)
    else {
        return;
    };
    let refused = refusal(&response.message);
    match step {
        Step::Download(image, size) => {
            let found = match (&response.message, refused) {
                (
                    protocol::ServerMessage::Read(
                        protocol::ReadResult::Image {
                            image: found,
                            ..
                        },
                    ),
                    None,
                ) => base64::engine::general_purpose::STANDARD
                    .decode(&found.data)
                    .ok()
                    .map(|bytes| (found.mime.clone(), bytes)),
                _ => None,
            };
            match found {
                Some((mime, bytes)) => {
                    world.trigger(ImageArrived {
                        image,
                        size,
                        mime,
                        bytes,
                    });
                }
                None => world.trigger(ImageMissing { image, size }),
            }
        }
        Step::Begin { key, bytes } => {
            let upload = match (&response.message, refused) {
                (_, Some(text)) => Err(text),
                (
                    protocol::ServerMessage::Reply(protocol::Reply {
                        upload: Some(upload),
                        ..
                    }),
                    None,
                ) => Ok(upload.clone()),
                _ => {
                    Err("The server did not accept the image.".into())
                }
            };
            match upload {
                Ok(upload) => send_part(world, key, upload, bytes, 0),
                Err(text) => {
                    world.trigger(ImageRefused { key, text });
                }
            }
        }
        Step::Part {
            key,
            upload,
            bytes,
            offset,
        } => {
            if let Some(text) = refused {
                world.trigger(ImageRefused { key, text });
                return;
            }
            let kept = match &response.message {
                protocol::ServerMessage::Reply(reply) => {
                    reply.image.clone()
                }
                _ => None,
            };
            match kept {
                Some(image) => {
                    world.trigger(ImageKept { key, image });
                }
                None if offset < bytes.len() => {
                    send_part(world, key, upload, bytes, offset);
                }
                None => world.trigger(ImageRefused {
                    key,
                    text: "The server kept no image.".into(),
                }),
            }
        }
    }
}

/// Ends the uploads and reads a lost connection cut off.
fn lost(
    _: On<Reconnected>,
    mut steps: ResMut<Steps>,
    mut commands: Commands,
) {
    for (_, step) in steps.0.drain() {
        match step {
            Step::Begin { key, .. } | Step::Part { key, .. } => {
                commands.trigger(ImageRefused {
                    key,
                    text: "The connection was lost.".into(),
                });
            }
            Step::Download(image, size) => {
                commands.trigger(ImageMissing { image, size });
            }
        }
    }
}

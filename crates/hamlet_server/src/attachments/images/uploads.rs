//! The images a client sends in parts, each upload an entity that
//! goes with its connection, and an image read back by its id.

use anyhow::Result;
use base64::Engine as _;
use bevy_ecs::component::Component;
use hamlet_protocol::client::{
    ImageBytes, ImageId, ImageSize, PutImage, PutImagePart,
    ReadResult, RequestId, ServerMessage,
};

use super::store::Images;
use super::{MOST_BYTES, capitalised};
use crate::app::tasks::BackgroundResult;
use crate::clients::check::{failed, fault, reply};
use crate::clients::connection::UploadOf;
use crate::core::Core;
use crate::society::rejection::Rejection;

/// The most images one connection sends at once.
const MOST_UPLOADS: usize = 32;

/// An image being sent in parts, by the connection it is
/// [`UploadOf`]; it goes when the connection goes.
#[derive(Component)]
pub(crate) struct ImageUpload {
    /// The name its parts are sent under.
    upload: String,
    size: u64,
    bytes: Vec<u8>,
}

impl Core<'_, '_> {
    /// Begins an image of `size` bytes; the reply names the upload
    /// its parts are sent under.
    pub(crate) fn put_image(
        &mut self,
        client: u64,
        put: PutImage,
    ) -> ServerMessage {
        let PutImage { id, size } = put;
        let Some(connection) = self.connections.entity(client) else {
            return reply(&id, Err(Rejection::ConnectionClosing));
        };
        let sending = self
            .uploads
            .images
            .iter()
            .filter(|(_, of, _)| of.0 == connection)
            .count();
        let rejection = if size == 0 {
            Some(Rejection::ImageEmpty)
        } else if size > MOST_BYTES as u64 {
            Some(Rejection::ImageTooBig)
        } else if sending >= MOST_UPLOADS {
            Some(Rejection::TooManyImagesSent)
        } else {
            None
        };
        if let Some(rejection) = rejection {
            return reply(&id, Err(rejection));
        }
        let upload = uuid::Uuid::new_v4().simple().to_string();
        self.commands.spawn((
            ImageUpload {
                upload: upload.clone(),
                size,
                bytes: Vec::new(),
            },
            UploadOf(connection),
        ));
        let mut answer = reply(
            &id,
            Ok::<_, Rejection>("Ready for the image.".into()),
        );
        if let ServerMessage::Reply(answer) = &mut answer {
            answer.upload = Some(upload);
        }
        answer
    }

    /// Takes a part of an image, and keeps the image once whole.
    pub(crate) fn put_image_part(
        &mut self,
        client: u64,
        part: PutImagePart,
    ) -> Result<ServerMessage> {
        let reject =
            |rejection: Rejection| reply(&part.id, Err(rejection));
        let connection = self.connections.entity(client);
        let Some((entity, _, mut sending)) =
            self.uploads.images.iter_mut().find(
                |(_, of, sending)| {
                    Some(of.0) == connection
                        && sending.upload == part.upload
                },
            )
        else {
            return Ok(reject(Rejection::NoImageUpload));
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD
            .decode(&part.data)
        else {
            return Ok(reject(Rejection::NotBase64));
        };
        let received = sending.bytes.len() as u64;
        if part.offset != received {
            return Ok(reject(Rejection::ImageContinuesAt {
                received,
            }));
        }
        if received + bytes.len() as u64 > sending.size {
            return Ok(reject(Rejection::ImageLongerThanDeclared));
        }
        sending.bytes.extend_from_slice(&bytes);
        if (sending.bytes.len() as u64) < sending.size {
            return Ok(reply(
                &part.id,
                Ok::<_, Rejection>("Received the image part.".into()),
            ));
        }
        let whole = std::mem::take(&mut sending.bytes);
        self.commands.entity(entity).despawn();
        let folder = Images::new(self.realm.layout().images());
        Ok(match folder.keep(&whole)? {
            Ok((image, _)) => {
                let mut answer = reply(
                    &part.id,
                    Ok::<_, Rejection>("Kept the image.".into()),
                );
                if let ServerMessage::Reply(answer) = &mut answer {
                    answer.image = Some(image);
                }
                answer
            }
            Err(unusable) => reject(Rejection::ImageUnusable {
                why: capitalised(&unusable.to_string()),
            }),
        })
    }

    /// Sends `client` the image `image` as `size` says, answering
    /// `id`, made on a thread of its own.
    pub(crate) fn read_image(
        &mut self,
        client: u64,
        id: &RequestId,
        image: &ImageId,
        size: ImageSize,
    ) {
        let folder = self.realm.layout().images();
        let (id, image) = (id.clone(), image.clone());
        self.later.spawn(async move {
            let wanted = image.clone();
            let found = tokio::task::spawn_blocking(move || {
                let images = Images::new(folder);
                match size {
                    ImageSize::Original => images.find(&wanted),
                    ImageSize::Thumbnail => images.thumbnail(&wanted),
                }
            })
            .await;
            let message = match found {
                Ok(Ok(Some(stored))) => {
                    ServerMessage::Read(ReadResult::Image {
                        image: ImageBytes {
                            data: base64::engine::general_purpose::STANDARD
                                .encode(&stored.bytes),
                            mime: stored.format.mime().into(),
                        },
                        to: id.clone(),
                    })
                }
                Ok(Ok(None)) => reply(
                    &id,
                    Err(Rejection::NoImage {
                        image: image.to_string(),
                    }),
                ),
                Ok(Err(error)) => fault(Some(&id), failed(error)),
                Err(panic) => fault(Some(&id), failed(panic)),
            };
            BackgroundResult::Reply { client, message }
        });
    }
}

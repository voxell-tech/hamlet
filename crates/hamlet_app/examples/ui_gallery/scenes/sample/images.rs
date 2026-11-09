//! The gallery's chat with images: one sent in a line, two attached
//! to the next, a drag over the window, and a model that sees none.

use std::io::Cursor;
use std::path::PathBuf;

use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::window::FileDragAndDrop;
use hamlet_client::chats::{Entry, Item, Transcript};
use hamlet_client::images::{Download, ImageArrived};
use hamlet_client::messages::Rewound;
use hamlet_client::realm::Account;
use hamlet_protocol::client as protocol;
use protocol::{AttachedFile, AttachedImage, FileId, ImageId};

use crate::scenes::Scene;

/// The id of the `n`th image the scene names.
fn id(n: u8) -> ImageId {
    ImageId::try_from(format!("{n:x}").repeat(64))
        .expect("an image id")
}

/// A chat whose chief sent an image with a line.
pub(super) fn conversation() -> Transcript {
    Transcript::from_items(vec![
        Item {
            id: "entry-0:0".into(),
            at: 1_791_374_400_000,
            entry: Entry::Chief {
                name: "chief".into(),
                text: "Is this the pear tree, or this one? Notes:"
                    .into(),
                images: vec![
                    AttachedImage {
                        at: Some(7),
                        id: id(1),
                    },
                    AttachedImage {
                        at: Some(30),
                        id: id(2),
                    },
                ],
                files: vec![AttachedFile {
                    at: None,
                    id: FileId::try_from("c".repeat(64))
                        .expect("a file id"),
                    name: "orchard-notes.md".into(),
                    size: 2048,
                }],
            },
        },
        Item {
            id: "entry-1:0".into(),
            at: 1_791_374_460_000,
            entry: Entry::Assistant(
                "The one on the left has pears on it.".into(),
            ),
        },
    ])
}

/// A picture of a gradient, which no server was asked for.
fn thumbnail(n: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(240, 160, |x, y| {
        image::Rgba([
            (60 + x / 2) as u8,
            (110 + u32::from(n) * 20 + y / 4) as u8,
            80,
            255,
        ])
    });
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("a picture");
    bytes.into_inner()
}

/// Gives each thumbnail asked for a picture, as the server would.
fn answer(request: On<Download>, mut commands: Commands) {
    let n = request.image.as_bytes()[0];
    let n = (n as char).to_digit(16).unwrap_or(0) as u8;
    commands.trigger(ImageArrived {
        image: request.image.clone(),
        size: request.size,
        mime: "image/png".into(),
        bytes: thumbnail(n),
    });
}

/// Attaches two images to the next line and drags an image over the
/// window. For the scene whose model sees none, the catalogue lists
/// it as one that takes none.
pub(super) fn setup(world: &mut World, scene: Scene, chat: Entity) {
    world.add_observer(answer);
    if scene == Scene::NoImages {
        let agent = world.resource::<hamlet_app::Convo>().agent;
        let model = world
            .get::<Account>(agent)
            .map(|account| account.model.clone())
            .unwrap_or_default();
        world.resource_mut::<hamlet_app::models::Shelf>().models =
            vec![protocol::Listed {
                context: None,
                default_effort: None,
                efforts: Vec::new(),
                fast: false,
                images: None,
                name: model,
                note: String::new(),
                price: None,
                priced: protocol::ListedPriced::Unknown,
                title: String::new(),
            }];
    } else {
        world.trigger(Rewound {
            chat,
            text: String::new(),
            images: vec![
                AttachedImage::last(id(3)),
                AttachedImage::last(id(4)),
            ],
            files: Vec::new(),
        });
    }
    world.write_message(FileDragAndDrop::HoveredFile {
        window: Entity::PLACEHOLDER,
        path_buf: PathBuf::from("pear.png"),
    });
}

//! Images attached to the next line (C33): dropped on the window,
//! pasted in the field or chosen in the system's dialog, uploaded at
//! once, shown as thumbnails over the field, and sent with the line.
//! A model that takes no images has its attach control greyed out,
//! and a drag over the window says it cannot be dropped.
//!
//! The system gives no pointer position with a drop. The hand holds
//! each dropped file until the pointer is seen over the chat, where
//! it is attached here.

use std::collections::HashSet;
use std::path::PathBuf;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::message::MessageReader;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::math::Vec2;
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::tasks::{AsyncComputeTaskPool, TaskPool};
use bevy::text::EditableText;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, Node,
    UiRect, px,
};
use bevy::window::FileDragAndDrop;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::images::{
    Download, ImageArrived, ImageKept, ImageMissing, ImageRefused,
    Upload, is_image_file,
};
use hamlet_client::realm::Account;
use hamlet_desktop::clipboard::{ImagesPasted, PasteImages};
use hamlet_desktop::images::{ChooseImages, ImagesChosen};
use hamlet_desktop::open;
use hamlet_protocol::client::{ImageId, ImageSize};
use hamlet_realm::hand::FileDropped;
use hamlet_ui::styles::{ICON, back_link, bare, close_button, note};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use super::field::typing;
use super::{ChatField, Draft};
use crate::convo::Convo;
use crate::models::Shelf;
use crate::pictures::{Pictures, Source, State, want};
use crate::roll::{agents_moved, either, moved};

/// The side of a thumbnail, in pixels.
const THUMB: f32 = 56.0;

/// The side of an image in a sent line, in pixels.
const SENT: f32 = 160.0;

/// The most bytes of an image file attached. The server refuses
/// larger ones, so the file is not read.
const MOST_BYTES: u64 = 20 * 1024 * 1024;

/// The images a line may carry when the model names no limit.
const MOST_PER_LINE: usize = 20;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Attached>()
        .init_resource::<Reads>()
        .init_resource::<Opening>()
        .add_message::<FileDragAndDrop>()
        .add_message::<FileDropped>()
        .add_observer(kept)
        .add_observer(refused)
        .add_observer(chosen)
        .add_observer(paste)
        .add_observer(pasted)
        .add_observer(open_original)
        .add_observer(original_arrived)
        .add_observer(original_missing)
        .add_systems(Update, (watch_drag, dropped, finish_reads));
}

/// The images the open chat's model takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Sight {
    /// It takes up to this many to a line.
    Sees(usize),
    /// Its catalogue entry says it takes none.
    Blind,
}

/// The [`Sight`] of the open chat's agent's model. A model the
/// catalogue does not list is taken to see: the server refuses what
/// it cannot.
pub(super) fn sight(world: &World) -> Sight {
    let model = world
        .get_resource::<Convo>()
        .and_then(|convo| world.get::<Account>(convo.agent))
        .map(|account| account.model.as_str())
        .unwrap_or_default();
    let listed = world.get_resource::<Shelf>().and_then(|shelf| {
        shelf.models.iter().find(|listed| listed.name == model)
    });
    match listed {
        Some(listed) => match &listed.images {
            Some(input) => Sight::Sees(
                input
                    .per_line
                    .map_or(MOST_PER_LINE, |most| most as usize),
            ),
            None => Sight::Blind,
        },
        None => Sight::Sees(MOST_PER_LINE),
    }
}

/// An image on its way to the next line.
struct Attachment {
    key: u64,
    /// The source of its thumbnail: its file, or the server's copy
    /// once that is kept. None while a pasted picture goes up.
    shown: Option<Source>,
    progress: Progress,
}

#[derive(Clone, PartialEq)]
enum Progress {
    /// Its file is read, or its bytes are on their way up.
    Sending,
    Kept(ImageId),
    Refused(String),
}

/// The images the next line carries, and a drag over the window.
#[derive(Resource, Default)]
pub struct Attached {
    list: Vec<Attachment>,
    next: u64,
    /// An image file is being dragged over the window.
    dragging: bool,
}

impl Attached {
    /// The images of the line, once every one is kept. `None` while
    /// one still goes up.
    pub(super) fn ready(&self) -> Option<Vec<ImageId>> {
        self.list
            .iter()
            .map(|attachment| match &attachment.progress {
                Progress::Kept(image) => Some(image.clone()),
                _ => None,
            })
            .collect()
    }

    /// Lets the images go, as when their line was sent.
    pub(super) fn clear(&mut self) {
        self.list.clear();
    }

    /// Puts `images`, which the server keeps, in place of what is
    /// attached, as when a conversation is rewound to a line.
    pub(super) fn give_back(&mut self, images: &[ImageId]) {
        self.list.clear();
        for image in images {
            let key = self.next;
            self.next += 1;
            self.list.push(Attachment {
                key,
                shown: Some(Source::Image(image.clone())),
                progress: Progress::Kept(image.clone()),
            });
        }
    }
}

/// What is attached: a file to read, or a picture's bytes.
pub(super) enum Incoming {
    File(PathBuf),
    Picture(Vec<u8>),
}

/// The files being read, each on a thread of the task pool, with
/// their bytes or why they were not read.
#[derive(Resource)]
struct Reads {
    tx: UnboundedSender<(u64, Result<Vec<u8>, String>)>,
    rx: UnboundedReceiver<(u64, Result<Vec<u8>, String>)>,
}

impl Default for Reads {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Reads { tx, rx }
    }
}

/// The bytes of the file `path`, unless it is over [`MOST_BYTES`].
fn read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let metadata =
        std::fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MOST_BYTES {
        return Err("The image is too large.".into());
    }
    std::fs::read(path).map_err(|error| error.to_string())
}

/// Attaches `incoming` to the next line: each is read if it is a
/// file and sent up, and shows as a thumbnail meanwhile.
pub(super) fn attach(world: &mut World, incoming: Vec<Incoming>) {
    let Sight::Sees(most) = sight(world) else {
        world.resource_mut::<Draft>().notice =
            "This model cannot see images.".into();
        return;
    };
    for item in incoming {
        if world.resource::<Attached>().list.len() >= most {
            world.resource_mut::<Draft>().notice =
                format!("A line takes at most {most} images.");
            return;
        }
        let mut attached = world.resource_mut::<Attached>();
        let key = attached.next;
        attached.next += 1;
        match item {
            Incoming::File(path) => {
                attached.list.push(Attachment {
                    key,
                    shown: Some(Source::File(path.clone())),
                    progress: Progress::Sending,
                });
                // The thumbnail is read from the file as it is.
                want(world, &Source::File(path.clone()));
                let tx = world.resource::<Reads>().tx.clone();
                AsyncComputeTaskPool::get_or_init(TaskPool::default)
                    .spawn(async move {
                        let _ = tx.send((key, read(&path)));
                    })
                    .detach();
            }
            Incoming::Picture(bytes) => {
                let progress = match bytes.len() as u64 <= MOST_BYTES
                {
                    true => Progress::Sending,
                    false => Progress::Refused(
                        "The image is too large.".into(),
                    ),
                };
                let sends = progress == Progress::Sending;
                attached.list.push(Attachment {
                    key,
                    shown: None,
                    progress,
                });
                if sends {
                    world.trigger(Upload { key, bytes });
                }
            }
        }
    }
    world.resource_mut::<Draft>().notice.clear();
}

/// Sends up the files that were read.
fn finish_reads(
    mut reads: ResMut<Reads>,
    mut attached: ResMut<Attached>,
    mut commands: Commands,
) {
    while let Ok((key, bytes)) = reads.rx.try_recv() {
        // A cross may have let it go while it was read.
        let Some(attachment) = attached
            .list
            .iter_mut()
            .find(|attachment| attachment.key == key)
        else {
            continue;
        };
        match bytes {
            Ok(bytes) => commands.trigger(Upload { key, bytes }),
            Err(text) => {
                attachment.progress = Progress::Refused(text);
            }
        }
    }
}

fn kept(kept: On<ImageKept>, mut attached: ResMut<Attached>) {
    if let Some(attachment) = attached
        .list
        .iter_mut()
        .find(|attachment| attachment.key == kept.key)
    {
        attachment.progress = Progress::Kept(kept.image.clone());
        if attachment.shown.is_none() {
            attachment.shown =
                Some(Source::Image(kept.image.clone()));
        }
    }
}

fn refused(
    refused: On<ImageRefused>,
    mut attached: ResMut<Attached>,
) {
    if let Some(attachment) = attached
        .list
        .iter_mut()
        .find(|attachment| attachment.key == refused.key)
    {
        attachment.progress = Progress::Refused(refused.text.clone());
    }
}

/// Attaches the files the dialog gave.
fn chosen(chosen: On<ImagesChosen>, mut commands: Commands) {
    let paths = chosen.paths.clone();
    commands.queue(move |world: &mut World| {
        attach(
            world,
            paths.into_iter().map(Incoming::File).collect(),
        );
    });
}

/// Asks for the images on the clipboard when Cmd+V, or Ctrl+V where
/// there is no Cmd, is pressed in the field. Words pasted are the
/// field's own.
fn paste(
    key: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    convo: Res<Convo>,
    root: Query<&Children, With<ChatField>>,
    texts: Query<&EditableText>,
    mut commands: Commands,
) {
    let held = match cfg!(target_os = "macos") {
        true => [KeyCode::SuperLeft, KeyCode::SuperRight],
        false => [KeyCode::ControlLeft, KeyCode::ControlRight],
    };
    if !key.input.state.is_pressed()
        || key.input.key_code != KeyCode::KeyV
        || !keys.any_pressed(held)
        || typing(&root, &texts) != Some(key.focused_entity)
    {
        return;
    }
    commands.trigger(PasteImages {
        asker: convo.session,
    });
}

/// Attaches the images the clipboard held.
fn pasted(
    pasted: On<ImagesPasted>,
    convo: Res<Convo>,
    mut commands: Commands,
) {
    if pasted.asker != convo.session {
        return;
    }
    let files = pasted.files.iter().cloned().map(Incoming::File);
    let pictures =
        pasted.pictures.iter().cloned().map(Incoming::Picture);
    let incoming = files.chain(pictures).collect::<Vec<_>>();
    commands.queue(move |world: &mut World| attach(world, incoming));
}

/// Follows the drag of image files over the window.
fn watch_drag(
    mut files: MessageReader<FileDragAndDrop>,
    mut attached: ResMut<Attached>,
) {
    for file in files.read() {
        match file {
            FileDragAndDrop::HoveredFile { path_buf, .. } => {
                attached.dragging = is_image_file(path_buf);
            }
            FileDragAndDrop::HoveredFileCanceled { .. }
            | FileDragAndDrop::DroppedFile { .. } => {
                attached.dragging = false;
            }
        }
    }
}

/// Attaches the image files dropped on the chat.
fn dropped(
    mut images: MessageReader<FileDropped>,
    mut commands: Commands,
) {
    let incoming = images
        .read()
        .map(|dropped| Incoming::File(dropped.path.clone()))
        .collect::<Vec<_>>();
    if !incoming.is_empty() {
        commands
            .queue(move |world: &mut World| attach(world, incoming));
    }
}

/// The image a sent line or a thumbnail shows, which a click opens in
/// full.
#[derive(Component)]
struct Original(ImageId);

/// The originals asked for, to be opened when they arrive.
#[derive(Resource, Default)]
struct Opening(HashSet<ImageId>);

/// Asks the server for the image that was clicked, in full.
fn open_original(
    click: On<PointerClick>,
    originals: Query<&Original>,
    mut opening: ResMut<Opening>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(Original(image)) = originals.get(click.entity) else {
        return;
    };
    if opening.0.insert(image.clone()) {
        commands.trigger(Download {
            image: image.clone(),
            size: ImageSize::Original,
        });
    }
}

/// The ending of a file of the media type `mime`.
fn ending(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

/// Opens an original that arrived, by the program the system keeps
/// for it. The file is written off the frame.
fn original_arrived(
    arrived: On<ImageArrived>,
    mut opening: ResMut<Opening>,
) {
    if arrived.size != ImageSize::Original
        || !opening.0.remove(&arrived.image)
    {
        return;
    }
    let path = std::env::temp_dir()
        .join("hamlet-images")
        .join(format!("{}.{}", arrived.image, ending(&arrived.mime)));
    let bytes = arrived.bytes.clone();
    AsyncComputeTaskPool::get_or_init(TaskPool::default)
        .spawn(async move {
            let written = path.parent().is_some_and(|folder| {
                std::fs::create_dir_all(folder).is_ok()
            }) && std::fs::write(&path, bytes).is_ok();
            if written {
                open::show_file(&path);
            }
        })
        .detach();
}

fn original_missing(
    missing: On<ImageMissing>,
    mut opening: ResMut<Opening>,
) {
    if missing.size == ImageSize::Original {
        opening.0.remove(&missing.image);
    }
}

/// A thumbnail of `source` no larger than `side` either way, asked
/// for once, which a plate holds the place of while it is read. One
/// of an image the server keeps opens it in full when clicked.
fn thumbnail(source: Source, side: f32) -> impl View {
    let of = source.clone();
    let state = resource::<Pictures, _>(move |all| all.state(&of));
    keyed(state, move |state| {
        let (source, state) = (source.clone(), state.clone());
        AnyView::new(move |cx: &mut Cx| {
            want(cx.world, &source);
            let within = cx.build(bare());
            let theme = cx.theme();
            let (back, edge, round) = (
                theme.palette.code,
                theme.palette.gold_6,
                theme.space.round,
            );
            let mut node = Node {
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(round)),
                ..Node::default()
            };
            let image = match &state {
                State::Ready(drawn) => {
                    let size = drawn.fitted(Vec2::splat(side));
                    (node.width, node.height) =
                        (px(size.x + 2.0), px(size.y + 2.0));
                    Some(drawn.image.clone())
                }
                _ => {
                    (node.width, node.height) =
                        (px(side * 0.75), px(side * 0.75));
                    None
                }
            };
            let opens = match &source {
                Source::Image(image) => Some(Original(image.clone())),
                _ => None,
            };
            let entity = cx
                .world
                .spawn((
                    node,
                    BackgroundColor(back),
                    BorderColor::all(edge),
                    source,
                    ChildOf(within),
                ))
                .id();
            if let Some(opens) = opens {
                cx.world.entity_mut(entity).insert(opens);
            }
            if let Some(image) = image {
                cx.world.entity_mut(entity).insert(
                    ImageNode::new(image)
                        .with_mode(NodeImageMode::Stretch),
                );
            }
            within
        })
    })
    .within(bare())
}

/// The images a sent line carries, each fetched once by its id.
pub(super) fn sent(images: &[ImageId]) -> impl View {
    row(images
        .iter()
        .map(|image| {
            thumbnail(Source::Image(image.clone()), SENT).boxed()
        })
        .collect::<Vec<_>>())
    .gap(4.0)
    .padding(UiRect::bottom(px(4.0)))
}

/// The thumbnails over the field, each with the way to let it go.
pub(super) fn strip() -> impl View {
    let seen = resource::<Attached, _>(|attached| {
        attached
            .list
            .iter()
            .map(|a| (a.key, a.shown.clone(), a.progress.clone()))
            .collect::<Vec<_>>()
    });
    keyed(seen, |seen| {
        if seen.is_empty() {
            return bare().boxed();
        }
        let items = seen
            .iter()
            .map(|(key, shown, progress)| {
                let key = *key;
                let status = match progress {
                    Progress::Sending => note("Sending").boxed(),
                    Progress::Kept(_) => bare().boxed(),
                    Progress::Refused(text) => label(text.clone())
                        .size(11.0)
                        .tone(Tone::Critical)
                        .boxed(),
                };
                let picture = match shown {
                    Some(source) => {
                        thumbnail(source.clone(), THUMB).boxed()
                    }
                    None => bare()
                        .width(px(THUMB * 0.75))
                        .height(px(THUMB * 0.75))
                        .boxed(),
                };
                column((
                    row((
                        picture,
                        close_button(move |world| {
                            world
                                .resource_mut::<Attached>()
                                .list
                                .retain(|a| a.key != key);
                        }),
                    ))
                    .gap(2.0)
                    .align(AlignItems::FlexStart),
                    status,
                ))
                .gap(2.0)
                .boxed()
            })
            .collect::<Vec<_>>();
        row(items).gap(8.0).boxed()
    })
    .within(bare())
}

/// The control that opens the dialog images are chosen in, greyed
/// out for a model that takes none.
pub(super) fn control() -> impl View {
    let sees = derived(|world: &World| sight(world)).when(either(
        either(agents_moved(), moved::<Shelf>()),
        moved::<Convo>(),
    ));
    keyed(sees, |sight| match sight {
        Sight::Sees(_) => button(glyph(Glyph::Picture, ICON))
            .padding(UiRect::all(px(4.0)))
            .tooltip(|| note("Attach images"))
            .rules(back_link)
            .tagged(Variant::Text)
            .on_activate(|world| {
                let asker = world.resource::<Convo>().session;
                world.trigger(ChooseImages { asker });
            })
            .boxed(),
        Sight::Blind => {
            column((glyph(Glyph::Picture, ICON).tone(Tone::Faint),))
                .padding(UiRect::all(px(4.0)))
                .tooltip(|| note("This model cannot see images"))
                .boxed()
        }
    })
    .within(bare())
}

/// The indicator shown while an image is dragged over the window:
/// where it can be dropped, or that it cannot.
pub(super) fn indicator() -> impl View {
    let drag = derived(|world: &World| {
        let dragging = world
            .get_resource::<Attached>()
            .is_some_and(|attached| attached.dragging);
        dragging.then(|| sight(world))
    })
    .when(either(
        either(moved::<Attached>(), moved::<Shelf>()),
        either(agents_moved(), moved::<Convo>()),
    ));
    keyed(drag, |drag| match drag {
        None => bare().boxed(),
        Some(sight) => {
            let (words, tone) = match sight {
                Sight::Sees(_) => ("Drop to attach", Tone::Accent),
                Sight::Blind => {
                    ("This model cannot see images", Tone::Critical)
                }
            };
            row((
                glyph(Glyph::Picture, ICON).tone(tone),
                label(words).size(12.0).tone(tone),
            ))
            .gap(6.0)
            .align(AlignItems::Center)
            .padding(UiRect::axes(px(8.0), px(6.0)))
            .border(1.0)
            .radius(4.0)
            .boxed()
        }
    })
    .within(bare())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bevy::math::UVec2;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::{panel_app, png, seat_hand};

    fn listed(
        images: Option<protocol::ImageInput>,
    ) -> protocol::Listed {
        protocol::Listed {
            context: None,
            default_effort: None,
            efforts: Vec::new(),
            fast: false,
            images,
            name: "lantern::wren-large".into(),
            note: String::new(),
            price: None,
            priced: protocol::ListedPriced::Unknown,
            title: String::new(),
        }
    }

    #[test]
    fn only_a_model_that_sees_takes_an_image_and_a_line_has_its_limit()
     {
        let mut app = panel_app();
        app.add_plugins(plugin)
            .init_resource::<Draft>()
            .init_resource::<Shelf>();
        let hand = seat_hand(&mut app);
        let world = app.world_mut();
        world.get_mut::<Account>(hand).unwrap().model =
            "lantern::wren-large".into();
        let folder = hamlet_testing::Scratch::new("attach");
        let pear = folder.join("pear.png");
        std::fs::write(&pear, png(UVec2::new(4, 4))).unwrap();
        let before = world.resource::<Outbox>().iter().count();

        world.resource_mut::<Shelf>().models = vec![listed(None)];
        assert_eq!(sight(world), Sight::Blind);
        attach(world, vec![Incoming::File(pear.clone())]);
        assert!(world.resource::<Attached>().list.is_empty());
        assert!(!world.resource::<Draft>().notice.is_empty());
        assert_eq!(world.resource::<Outbox>().iter().count(), before);

        let input = protocol::ImageInput {
            per_line: Some(1),
            ..Default::default()
        };
        world.resource_mut::<Shelf>().models =
            vec![listed(Some(input))];
        assert_eq!(sight(world), Sight::Sees(1));
        attach(
            world,
            vec![Incoming::File(pear.clone()), Incoming::File(pear)],
        );
        // The second image is over the line's limit.
        assert_eq!(world.resource::<Attached>().list.len(), 1);
        // The file is read off the frame, then sent up.
        let start = Instant::now();
        while !matches!(
            app.world().resource::<Outbox>().iter().last(),
            Some(protocol::ClientMessage::PutImage(_))
        ) {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "the image was not sent"
            );
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        let attached = app.world().resource::<Attached>();
        assert_eq!(attached.ready(), None);
    }

    #[test]
    fn a_rewound_line_gives_its_images_back() {
        let mut attached = Attached::default();
        let image = ImageId::try_from("a".repeat(64)).unwrap();
        attached.give_back(std::slice::from_ref(&image));
        assert_eq!(attached.ready(), Some(vec![image]));
        attached.give_back(&[]);
        assert_eq!(attached.ready(), Some(Vec::new()));
    }
}

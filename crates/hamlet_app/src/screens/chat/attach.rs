//! Images, files and folders attached to the next line (C33):
//! dropped on the window, pasted in the field or chosen in the
//! system's dialog, uploaded at once, shown over the field, and sent
//! with the line. An image is shown as a thumbnail; a file or a
//! folder as a chip with its size, which waits for a chief's word
//! when what is attached is over the cap. A model that takes no
//! images has its attach control for images greyed out, and a drag of
//! an image over the window says it cannot be dropped; files and
//! folders are taken whatever the model.
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
use bevy::ecs::schedule::IntoScheduleConfigs;
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
use hamlet_client::files::{
    FilesForget, FilesKept, FilesRefused, Gather, Gathered,
    UploadFiles,
};
use hamlet_client::images::{
    Download, ImageArrived, ImageKept, ImageMissing, ImageRefused,
    Upload,
};
use hamlet_client::realm::Account;
use hamlet_desktop::choose::{Choose, Chosen, What};
use hamlet_desktop::clipboard::{Paste, Pasted};
use hamlet_desktop::open;
use hamlet_protocol::client::{
    AttachedFile, AttachedImage, ImageId, ImageInput, ImageSize,
};
use hamlet_realm::hand::FileDropped;
use hamlet_ui::styles::{ICON, back_link, bare, close_button, note};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
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
        .add_observer(gathered)
        .add_observer(files_kept)
        .add_observer(files_refused)
        .add_observer(chosen)
        .add_observer(paste)
        .add_observer(pasted)
        .add_observer(open_original)
        .add_observer(original_arrived)
        .add_observer(original_missing)
        .add_systems(
            Update,
            (
                watch_drag,
                dropped,
                finish_reads,
                forgotten.run_if(|attached: Res<Attached>| {
                    !attached.forget.is_empty()
                }),
            ),
        );
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

/// A file or a folder on its way to the next line.
struct Parcel {
    key: u64,
    /// The name it was attached under.
    name: String,
    folder: bool,
    /// Links and the like inside it that are left out.
    skipped: usize,
    load: Load,
}

#[derive(Clone, PartialEq)]
enum Load {
    /// Its folders are walked and its files counted.
    Reading,
    /// Its files with those attached already are over the cap, so it
    /// waits for the chief to say it may be sent.
    Held {
        files: usize,
        bytes: u64,
    },
    Sending {
        files: usize,
        bytes: u64,
    },
    Kept(Vec<AttachedFile>),
    Refused(String),
}

/// The kind of thing dragged over the window.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Dragging {
    #[default]
    Nothing,
    /// An image file.
    Image,
    /// Any other file or folder.
    Other,
}

/// The images, files and folders the next line carries, and a drag
/// over the window.
#[derive(Resource, Default)]
pub struct Attached {
    list: Vec<Attachment>,
    parcels: Vec<Parcel>,
    next: u64,
    /// The parcels let go before they were kept, which the client is
    /// told to stop sending.
    forget: Vec<u64>,
    dragging: Dragging,
}

impl Attached {
    /// The images and files of the line, once every one is kept.
    /// `None` while one still goes up, waits for a word, or failed.
    pub(super) fn ready(
        &self,
    ) -> Option<(Vec<AttachedImage>, Vec<AttachedFile>)> {
        let images = self
            .list
            .iter()
            .map(|attachment| match &attachment.progress {
                Progress::Kept(image) => {
                    Some(AttachedImage::last(image.clone()))
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let mut files = Vec::new();
        for parcel in &self.parcels {
            match &parcel.load {
                Load::Kept(kept) => {
                    files.extend(kept.iter().cloned());
                }
                _ => return None,
            }
        }
        Some((images, files))
    }

    /// The files and the bytes of what is attached as files and
    /// folders, which the cap counts together.
    fn counted(&self) -> (usize, u64) {
        self.parcels.iter().fold((0, 0), |(files, bytes), parcel| {
            match &parcel.load {
                Load::Held {
                    files: more,
                    bytes: size,
                }
                | Load::Sending {
                    files: more,
                    bytes: size,
                } => (files + more, bytes + size),
                Load::Kept(kept) => (
                    files + kept.len(),
                    bytes
                        + kept
                            .iter()
                            .map(|file| file.size)
                            .sum::<u64>(),
                ),
                Load::Reading | Load::Refused(_) => (files, bytes),
            }
        })
    }

    /// Lets the parcels go, and tells the client to stop sending
    /// those not kept yet.
    fn drop_parcels(&mut self) {
        let unfinished = self
            .parcels
            .drain(..)
            .filter(|parcel| !matches!(parcel.load, Load::Kept(_)))
            .map(|parcel| parcel.key);
        self.forget.extend(unfinished);
    }

    /// Lets the images and files go.
    pub(super) fn clear(&mut self) {
        self.list.clear();
        self.drop_parcels();
    }

    /// Puts `images` and `files`, which the server keeps, in place
    /// of what is attached, as when a conversation is rewound to a
    /// line. They go after the words again, wherever they sat.
    pub(super) fn give_back(
        &mut self,
        images: &[AttachedImage],
        files: &[AttachedFile],
    ) {
        self.clear();
        for image in images {
            let key = self.next;
            self.next += 1;
            self.list.push(Attachment {
                key,
                shown: Some(Source::Image(image.id.clone())),
                progress: Progress::Kept(image.id.clone()),
            });
        }
        let files = files
            .iter()
            .map(|file| AttachedFile {
                at: None,
                ..file.clone()
            })
            .collect::<Vec<_>>();
        // One parcel for each name the files were attached under.
        for (name, kept) in by_top(&files) {
            let key = self.next;
            self.next += 1;
            self.parcels.push(Parcel {
                key,
                name,
                folder: kept
                    .iter()
                    .any(|file| file.name.contains('/')),
                skipped: 0,
                load: Load::Kept(kept),
            });
        }
    }
}

/// `files` grouped by the first part of their names: each file
/// attached, and each folder with what it holds.
fn by_top(
    files: &[AttachedFile],
) -> Vec<(String, Vec<AttachedFile>)> {
    let mut groups: Vec<(String, Vec<AttachedFile>)> = Vec::new();
    for file in files {
        let top = file.name.split('/').next().unwrap_or_default();
        match groups.iter_mut().find(|(name, _)| name == top) {
            Some((_, group)) => group.push(file.clone()),
            None => groups.push((top.to_owned(), vec![file.clone()])),
        }
    }
    groups
}

/// An attachment to take in: a file or a folder to read, or a
/// picture's bytes.
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

/// Attaches `incoming` to the next line. An image is read if it is a
/// file and sent up, and shows as a thumbnail meanwhile; any other
/// file, and a folder, is walked and counted off the frame, and sent
/// up unless that is over the cap.
pub(super) fn attach(world: &mut World, incoming: Vec<Incoming>) {
    let sight = sight(world);
    world.resource_mut::<Draft>().notice.clear();
    for item in incoming {
        let image = match &item {
            Incoming::File(path) => {
                ImageInput::is_image_file(path) && !path.is_dir()
            }
            Incoming::Picture(_) => true,
        };
        if !image {
            if let Incoming::File(path) = item {
                parcel(world, path);
            }
            continue;
        }
        let Sight::Sees(most) = sight else {
            world.resource_mut::<Draft>().notice =
                "This model cannot see images.".into();
            continue;
        };
        if world.resource::<Attached>().list.len() >= most {
            world.resource_mut::<Draft>().notice =
                format!("A line takes at most {most} images.");
            continue;
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
}

/// Attaches the file or folder at `path`, which the client counts.
fn parcel(world: &mut World, path: PathBuf) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    let folder = path.is_dir();
    let mut attached = world.resource_mut::<Attached>();
    let key = attached.next;
    attached.next += 1;
    attached.parcels.push(Parcel {
        key,
        name,
        folder,
        skipped: 0,
        load: Load::Reading,
    });
    world.trigger(Gather {
        key,
        paths: vec![path],
    });
}

/// Sends what was counted, unless it and what is attached already
/// are over the cap, when it waits for the chief's word.
fn gathered(
    gathered: On<Gathered>,
    mut attached: ResMut<Attached>,
    mut commands: Commands,
) {
    let (files, bytes) = attached.counted();
    let over = files + gathered.files > AttachedFile::CAP_FILES
        || bytes + gathered.bytes > AttachedFile::CAP_BYTES;
    // A cross may have let it go while it was counted.
    let Some(parcel) = attached
        .parcels
        .iter_mut()
        .find(|parcel| parcel.key == gathered.key)
    else {
        return;
    };
    parcel.skipped = gathered.skipped;
    let (files, bytes) = (gathered.files, gathered.bytes);
    parcel.load = if over {
        Load::Held { files, bytes }
    } else {
        commands.trigger(UploadFiles { key: gathered.key });
        Load::Sending { files, bytes }
    };
}

/// Sends a parcel that was held, as the chief said it may be.
fn send_held(world: &mut World, key: u64) {
    let mut attached = world.resource_mut::<Attached>();
    let Some(parcel) =
        attached.parcels.iter_mut().find(|parcel| parcel.key == key)
    else {
        return;
    };
    if let Load::Held { files, bytes } = parcel.load {
        parcel.load = Load::Sending { files, bytes };
        world.trigger(UploadFiles { key });
    }
}

fn files_kept(kept: On<FilesKept>, mut attached: ResMut<Attached>) {
    if let Some(parcel) = attached
        .parcels
        .iter_mut()
        .find(|parcel| parcel.key == kept.key)
    {
        parcel.load = Load::Kept(kept.files.clone());
    }
}

fn files_refused(
    refused: On<FilesRefused>,
    mut attached: ResMut<Attached>,
) {
    if let Some(parcel) = attached
        .parcels
        .iter_mut()
        .find(|parcel| parcel.key == refused.key)
    {
        parcel.load = Load::Refused(refused.text.clone());
    }
}

/// Tells the client to stop sending the parcels that were let go.
fn forgotten(mut attached: ResMut<Attached>, mut commands: Commands) {
    for key in std::mem::take(&mut attached.forget) {
        commands.trigger(FilesForget { key });
    }
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

/// Attaches the files and folders the dialog gave.
fn chosen(chosen: On<Chosen>, mut commands: Commands) {
    let paths = chosen.paths.clone();
    commands.queue(move |world: &mut World| {
        attach(
            world,
            paths.into_iter().map(Incoming::File).collect(),
        );
    });
}

/// Asks for the files and images on the clipboard when Cmd+V, or
/// Ctrl+V where there is no Cmd, is pressed in the field. Words
/// pasted are the field's own.
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
    commands.trigger(Paste {
        asker: convo.session,
    });
}

/// Attaches what the clipboard held.
fn pasted(
    pasted: On<Pasted>,
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

/// Follows the drag of files and folders over the window.
fn watch_drag(
    mut files: MessageReader<FileDragAndDrop>,
    mut attached: ResMut<Attached>,
) {
    for file in files.read() {
        match file {
            FileDragAndDrop::HoveredFile { path_buf, .. } => {
                attached.dragging =
                    match ImageInput::is_image_file(path_buf) {
                        true => Dragging::Image,
                        false => Dragging::Other,
                    };
            }
            FileDragAndDrop::HoveredFileCanceled { .. }
            | FileDragAndDrop::DroppedFile { .. } => {
                attached.dragging = Dragging::Nothing;
            }
        }
    }
}

/// Attaches the files and folders dropped on the chat.
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
pub(super) fn sent(images: &[AttachedImage]) -> impl View {
    row(images
        .iter()
        .map(|image| {
            thumbnail(Source::Image(image.id.clone()), SENT).boxed()
        })
        .collect::<Vec<_>>())
    .gap(4.0)
    .padding(UiRect::bottom(px(4.0)))
}

/// A file or folder over the field: what it is, how far it is, and
/// the way to let it go.
fn chip(
    key: u64,
    name: &str,
    folder: bool,
    skipped: usize,
    load: &Load,
) -> AnyView {
    let size = AttachedFile::size_words;
    let (words, tone) = match load {
        Load::Reading => ("Reading".to_owned(), Tone::Dim),
        Load::Sending { files, bytes } => (
            format!("Sending {} ({})", counted(*files), size(*bytes)),
            Tone::Dim,
        ),
        Load::Held { files, bytes } => (
            format!(
                "{} ({}) is over what one message takes: {} MiB or \
                 {} files.",
                counted(*files),
                size(*bytes),
                AttachedFile::CAP_BYTES >> 20,
                AttachedFile::CAP_FILES
            ),
            Tone::Critical,
        ),
        Load::Kept(kept) => (
            format!(
                "{} ({})",
                counted(kept.len()),
                size(kept.iter().map(|file| file.size).sum())
            ),
            Tone::Dim,
        ),
        Load::Refused(text) => (text.clone(), Tone::Critical),
    };
    let left_out = match skipped {
        0 => String::new(),
        n => format!(" {n} left out: not files."),
    };
    let held = matches!(load, Load::Held { .. });
    column((
        row((
            glyph(
                match folder {
                    true => Glyph::Folder,
                    false => Glyph::Notes,
                },
                ICON,
            ),
            label(name.to_owned()).size(12.0).wrap(false),
            close_button(move |world| {
                let mut attached = world.resource_mut::<Attached>();
                if let Some(at) = attached
                    .parcels
                    .iter()
                    .position(|parcel| parcel.key == key)
                {
                    let gone = attached.parcels.remove(at);
                    if !matches!(gone.load, Load::Kept(_)) {
                        attached.forget.push(gone.key);
                    }
                }
            }),
        ))
        .gap(4.0)
        .align(AlignItems::Center),
        label(format!("{words}{left_out}")).size(11.0).tone(tone),
        match held {
            true => hamlet_ui::styles::lettered("Send anyway")
                .rules(hamlet_ui::styles::button)
                .on_activate(move |world| send_held(world, key))
                .boxed(),
            false => bare().boxed(),
        },
    ))
    .gap(2.0)
    .max_width(px(240.0))
    .boxed()
}

/// `files` as words: `1 file`, `12 files`.
fn counted(files: usize) -> String {
    match files {
        1 => "1 file".to_owned(),
        n => format!("{n} files"),
    }
}

/// The files and folders a sent line carries, by name.
pub(super) fn sent_files(files: &[AttachedFile]) -> impl View {
    let words = by_top(files)
        .into_iter()
        .map(|(name, group)| {
            let size = AttachedFile::size_words(
                group.iter().map(|file| file.size).sum(),
            );
            match group.iter().any(|file| file.name.contains('/')) {
                true => format!(
                    "{name}/ ({}, {size})",
                    counted(group.len())
                ),
                false => format!("{name} ({size})"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    row((
        glyph(Glyph::Notes, ICON).tone(Tone::Dim),
        label(words).size(11.0).tone(Tone::Dim),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .padding(UiRect::bottom(px(4.0)))
}

/// The thumbnails and chips over the field, each with the way to let
/// it go.
pub(super) fn strip() -> impl View {
    let seen = resource::<Attached, _>(|attached| {
        (
            attached
                .list
                .iter()
                .map(|a| (a.key, a.shown.clone(), a.progress.clone()))
                .collect::<Vec<_>>(),
            attached
                .parcels
                .iter()
                .map(|p| {
                    (
                        p.key,
                        p.name.clone(),
                        p.folder,
                        p.skipped,
                        p.load.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    });
    keyed(seen, |(seen, parcels)| {
        if seen.is_empty() && parcels.is_empty() {
            return bare().boxed();
        }
        let mut items = seen
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
        items.extend(parcels.iter().map(
            |(key, name, folder, skipped, load)| {
                chip(*key, name, *folder, *skipped, load)
            },
        ));
        row(items).gap(8.0).tagged(Wraps).boxed()
    })
    .within(bare())
}

/// A control that opens the dialog `what` is chosen in.
fn chooser(which: Glyph, tip: &'static str, what: What) -> AnyView {
    button(glyph(which, ICON))
        .padding(UiRect::all(px(4.0)))
        .tooltip(move || note(tip))
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(move |world| {
            let asker = world.resource::<Convo>().session;
            world.trigger(Choose { asker, what });
        })
        .boxed()
}

/// The controls that open the dialogs images, files and folders are
/// chosen in. The one for images is greyed out for a model that
/// takes none.
pub(super) fn control() -> impl View {
    let sees = derived(|world: &World| sight(world)).when(either(
        either(agents_moved(), moved::<Shelf>()),
        moved::<Convo>(),
    ));
    let images = keyed(sees, |sight| match sight {
        Sight::Sees(_) => {
            chooser(Glyph::Picture, "Attach images", What::Images)
        }
        Sight::Blind => {
            column((glyph(Glyph::Picture, ICON).tone(Tone::Faint),))
                .padding(UiRect::all(px(4.0)))
                .tooltip(|| note("This model cannot see images"))
                .boxed()
        }
    })
    .within(bare());
    row((
        images,
        chooser(Glyph::Notes, "Attach files", What::Files),
        chooser(Glyph::Folder, "Attach a folder", What::Folders),
    ))
    .align(AlignItems::Center)
}

/// The indicator shown while a file or folder is dragged over the
/// window: where it can be dropped, or that it cannot.
pub(super) fn indicator() -> impl View {
    let drag = derived(|world: &World| {
        let dragging = world
            .get_resource::<Attached>()
            .map_or(Dragging::Nothing, |attached| attached.dragging);
        (dragging != Dragging::Nothing)
            .then(|| (dragging, sight(world)))
    })
    .when(either(
        either(moved::<Attached>(), moved::<Shelf>()),
        either(agents_moved(), moved::<Convo>()),
    ));
    keyed(drag, |drag| match drag {
        None => bare().boxed(),
        Some((dragging, sight)) => {
            let (words, tone) = match (dragging, sight) {
                (Dragging::Image, Sight::Blind) => {
                    ("This model cannot see images", Tone::Critical)
                }
                _ => ("Drop to attach", Tone::Accent),
            };
            let which = match dragging {
                Dragging::Image => Glyph::Picture,
                _ => Glyph::Notes,
            };
            row((
                glyph(which, ICON).tone(tone),
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

    fn listed(images: Option<ImageInput>) -> protocol::Listed {
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

        let input = ImageInput {
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
        let placed = AttachedImage {
            at: Some(3),
            id: image,
        };
        attached.give_back(std::slice::from_ref(&placed), &[]);
        assert_eq!(
            attached.ready(),
            Some((vec![AttachedImage::last(placed.id)], Vec::new()))
        );
        attached.give_back(&[], &[]);
        assert_eq!(attached.ready(), Some((Vec::new(), Vec::new())));
    }
}

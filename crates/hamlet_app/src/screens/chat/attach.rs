//! Images, files and folders attached to the next line (C33).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::app::{App, Update};
use bevy::camera::visibility::Visibility;
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::lifecycle::Remove;
use bevy::ecs::message::MessageReader;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::{Ref, World};
use bevy::input::ButtonInput;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::math::{Rect, UVec2, Vec2};
use bevy::picking::events::PointerClick;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::tasks::{AsyncComputeTaskPool, TaskPool};
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderRadius, JustifyContent, Node,
    PositionType, UiRect, Val, percent, px,
};
use bevy::window::FileDragAndDrop;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    Atom, BehaviorExt, FrameProps, Piece, RichField, Span,
    TooltipExt, button, column, label, rich, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::files::{
    FilesForget, FilesKept, FilesRejected, Gather, Gathered,
    UploadFiles,
};
use hamlet_client::images::{ImageKept, ImageRejected, Upload};
use hamlet_client::realm::Account;
use hamlet_desktop::choose::{Choose, Chosen, What};
use hamlet_desktop::clipboard::{Paste, Pasted};
use hamlet_protocol::client::{
    AttachedFile, AttachedImage, ImageId, ImageInput,
};
use hamlet_realm::hand::FileDropped;
use hamlet_ui::markdown::ATOM;
use hamlet_ui::styles::{
    ICON, Looks, Tipped, bare, close_button, note,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, Cx, View, built, with_cx};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use super::ChatField;
use crate::convo;
use crate::models::Shelf;
use crate::pictures::{Drawn, Pictures, Source, State, want};
use crate::roll::{either, moved};
use crate::screens::viewer;

/// How high Bevy lays out a line of text, in its font size.
const LINE: f32 = 1.2;

/// The side of a sent image's tile, in pixels.
const TILE: f32 = 112.0;

/// The most letters of a name a chip shows.
const NAME_LETTERS: usize = 28;

/// The most bytes of an image file attached, as the server takes.
const MOST_BYTES: u64 = 20 * 1024 * 1024;

/// The images a line may carry when the model names no limit.
const MOST_PER_LINE: usize = 20;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Attached>()
        .init_resource::<Reads>()
        .add_message::<FileDragAndDrop>()
        .add_message::<FileDropped>()
        .add_observer(kept)
        .add_observer(rejected)
        .add_observer(gathered)
        .add_observer(files_kept)
        .add_observer(files_rejected)
        .add_observer(chosen)
        .add_observer(paste)
        .add_observer(pasted)
        .add_observer(open_original)
        .add_observer(hub_gone)
        .add_systems(
            Update,
            (
                watch_drag,
                dropped,
                finish_reads,
                reveal,
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

/// The Hub what comes from outside one is attached in: the one under
/// the pointer, else the one last pressed or typed in.
fn receiver(world: &World) -> Option<Entity> {
    crate::hub::under_pointer(world)
        .or_else(|| crate::hub::active(world))
}

/// The draft attachments go with: a Hub's, for one of its chats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Owner {
    pub hub: Entity,
    pub chat: Entity,
}

impl Owner {
    /// The draft of the chat `hub` shows.
    pub(super) fn of(world: &World, hub: Entity) -> Self {
        Owner {
            hub,
            chat: convo::chat_of(world, hub),
        }
    }
}

/// The [`Sight`] of the model of `hub`'s chat; an unlisted one sees.
pub(super) fn sight(world: &World, hub: Option<Entity>) -> Sight {
    let model = hub
        .and_then(|hub| convo::of(world, hub))
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
    /// The draft it is attached to.
    owner: Owner,
    name: String,
    /// The bytes of its file, once they are read.
    size: Option<u64>,
    /// Its picture's source, none while a pasted picture goes up.
    shown: Option<Source>,
    progress: Progress,
    /// Whether its chip has been seen in the field.
    placed: bool,
}

#[derive(Clone, PartialEq)]
enum Progress {
    /// Its file is read, or its bytes are on their way up.
    Sending,
    Kept(ImageId),
    Rejected(String),
}

/// A file or a folder on its way to the next line.
struct Parcel {
    key: u64,
    /// The draft it is attached to.
    owner: Owner,
    name: String,
    folder: bool,
    /// Links and the like inside it that are left out.
    skipped: usize,
    load: Load,
    /// Whether its chip has been seen in the field.
    placed: bool,
}

#[derive(Clone, PartialEq)]
enum Load {
    /// Its folders are walked and its files counted.
    Reading,
    /// Over the cap, it waits for the chief's word.
    Held {
        files: usize,
        bytes: u64,
    },
    Sending {
        files: usize,
        bytes: u64,
    },
    Kept(Vec<AttachedFile>),
    Rejected(String),
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

/// What the Hubs' drafts carry, and a drag over the window.
#[derive(Resource, Default)]
pub struct Attached {
    list: Vec<Attachment>,
    parcels: Vec<Parcel>,
    next: u64,
    /// The parcels let go before they were kept.
    forget: Vec<u64>,
    dragging: Dragging,
}

/// The byte of the line's text each attachment sits at, by its key.
pub(crate) type Places = HashMap<u64, u64>;

impl Attached {
    fn images_of(
        &self,
        owner: Owner,
    ) -> impl Iterator<Item = &Attachment> {
        self.list.iter().filter(move |image| image.owner == owner)
    }

    fn parcels_of(
        &self,
        owner: Owner,
    ) -> impl Iterator<Item = &Parcel> {
        self.parcels
            .iter()
            .filter(move |parcel| parcel.owner == owner)
    }

    /// The Hub whose draft the attachment `key` is in.
    fn hub_of(&self, key: u64) -> Option<Entity> {
        let image = self.list.iter().find(|image| image.key == key);
        let owner = match image {
            Some(image) => image.owner,
            None => {
                self.parcels
                    .iter()
                    .find(|parcel| parcel.key == key)?
                    .owner
            }
        };
        Some(owner.hub)
    }

    /// `owner`'s images and files at `places`, once all are kept.
    pub(super) fn ready(
        &self,
        owner: Owner,
        places: &Places,
    ) -> Option<(Vec<AttachedImage>, Vec<AttachedFile>)> {
        let images = self
            .images_of(owner)
            .map(|attachment| match &attachment.progress {
                Progress::Kept(image) => Some(AttachedImage {
                    at: places.get(&attachment.key).copied(),
                    id: image.clone(),
                }),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let mut files = Vec::new();
        for parcel in self.parcels_of(owner) {
            match &parcel.load {
                Load::Kept(kept) => {
                    files.extend(kept.iter().map(|file| {
                        AttachedFile {
                            at: places.get(&parcel.key).copied(),
                            ..file.clone()
                        }
                    }));
                }
                _ => return None,
            }
        }
        Some((images, files))
    }

    /// The files and bytes attached to `owner`'s draft.
    fn counted(&self, owner: Owner) -> (usize, u64) {
        self.parcels_of(owner).fold(
            (0, 0),
            |(files, bytes), parcel| match &parcel.load {
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
                Load::Reading | Load::Rejected(_) => (files, bytes),
            },
        )
    }

    /// Lets the images and files of `owner`'s draft go.
    pub(super) fn clear(&mut self, owner: Owner) {
        self.release(|of| of == owner);
    }

    /// Lets the images and files of the drafts `mine` picks go.
    fn release(&mut self, mine: impl Fn(Owner) -> bool) {
        self.list.retain(|image| !mine(image.owner));
        let mut unfinished = Vec::new();
        self.parcels.retain(|parcel| {
            let mine = mine(parcel.owner);
            if mine && !matches!(parcel.load, Load::Kept(_)) {
                unfinished.push(parcel.key);
            }
            !mine
        });
        self.forget.extend(unfinished);
    }

    /// Attaches `images` and `files` to `owner`'s draft again and
    /// gives `words` with their chips.
    pub(super) fn give_back(
        &mut self,
        owner: Owner,
        images: &[AttachedImage],
        files: &[AttachedFile],
        words: &str,
        shift: usize,
    ) -> Vec<Piece> {
        self.clear(owner);
        let place = |at: Option<u64>| {
            at.and_then(|at| usize::try_from(at).ok())
                .and_then(|at| at.checked_sub(shift))
                .filter(|at| {
                    *at <= words.len() && words.is_char_boundary(*at)
                })
                .unwrap_or(words.len())
        };
        let mut chips = Vec::new();
        for image in images {
            let key = self.next;
            self.next += 1;
            self.list.push(Attachment {
                key,
                owner,
                name: "Image".into(),
                size: None,
                shown: Some(Source::Image(image.id.clone())),
                progress: Progress::Kept(image.id.clone()),
                placed: false,
            });
            chips.push((place(image.at), key));
        }
        // One parcel for each name the files were attached under.
        for (name, kept) in by_top(files) {
            let key = self.next;
            self.next += 1;
            chips.push((place(kept[0].at), key));
            self.parcels.push(Parcel {
                key,
                owner,
                name,
                folder: kept
                    .iter()
                    .any(|file| file.name.contains('/')),
                skipped: 0,
                load: Load::Kept(kept),
                placed: false,
            });
        }
        chips.sort_by_key(|(at, _)| *at);
        let mut pieces = Vec::new();
        let mut from = 0;
        for (at, key) in chips {
            if at > from {
                pieces.push(Piece::Text(words[from..at].into()));
                from = at;
            }
            pieces.push(Piece::Atom(chip_atom(
                key,
                &self.name_of(key),
            )));
        }
        if from < words.len() {
            pieces.push(Piece::Text(words[from..].into()));
        }
        pieces
    }

    fn name_of(&self, key: u64) -> String {
        self.face(key).map(|face| face.name).unwrap_or_default()
    }

    /// The [`Face`] of the chip of the attachment `key`.
    fn face(&self, key: u64) -> Option<Face> {
        let size = AttachedFile::size_words;
        if let Some(image) =
            self.list.iter().find(|image| image.key == key)
        {
            let (size, trouble) = match &image.progress {
                Progress::Sending => ("Sending".to_owned(), None),
                Progress::Kept(_) => {
                    (image.size.map(size).unwrap_or_default(), None)
                }
                Progress::Rejected(text) => {
                    (String::new(), Some(text.clone()))
                }
            };
            return Some(Face {
                glyph: Glyph::Picture,
                picture: image.shown.clone(),
                name: image.name.clone(),
                size,
                trouble,
                image: true,
            });
        }
        let parcel =
            self.parcels.iter().find(|parcel| parcel.key == key)?;
        let (size, trouble) = match &parcel.load {
            Load::Reading => ("Reading".to_owned(), None),
            Load::Sending { .. } => ("Sending".to_owned(), None),
            Load::Held { files, bytes } => (
                held_words(*files, *bytes),
                Some(over(*files, *bytes)),
            ),
            Load::Kept(kept) => (
                parcel_words(
                    parcel.folder,
                    kept.len(),
                    kept.iter().map(|file| file.size).sum(),
                ),
                None,
            ),
            Load::Rejected(text) => {
                (String::new(), Some(text.clone()))
            }
        };
        let trouble = match (trouble, parcel.skipped) {
            (trouble, 0) => trouble,
            (trouble, n) => Some(format!(
                "{} {n} left out: not files.",
                trouble.unwrap_or_default()
            )),
        };
        Some(Face {
            glyph: match parcel.folder {
                true => Glyph::Folder,
                false => Glyph::Notes,
            },
            picture: None,
            name: parcel.name.clone(),
            size,
            trouble,
            image: false,
        })
    }

    /// Notes the chips of `owner`'s field and lets go of those taken
    /// out.
    fn follow(&mut self, owner: Owner, seen: &HashSet<u64>) {
        let mut gone = Vec::new();
        self.list.retain_mut(|image| {
            if image.owner != owner {
                return true;
            }
            let here = seen.contains(&image.key);
            image.placed |= here;
            here || !image.placed
        });
        self.parcels.retain_mut(|parcel| {
            if parcel.owner != owner {
                return true;
            }
            let here = seen.contains(&parcel.key);
            parcel.placed |= here;
            let keep = here || !parcel.placed;
            if !keep && !matches!(parcel.load, Load::Kept(_)) {
                gone.push(parcel.key);
            }
            keep
        });
        self.forget.extend(gone);
    }

    /// Whether the chips `owner`'s field holds differ from those
    /// noted.
    fn is_out_of_step(
        &self,
        owner: Owner,
        seen: &HashSet<u64>,
    ) -> bool {
        let noted = self
            .images_of(owner)
            .map(|image| (image.key, image.placed))
            .chain(
                self.parcels_of(owner)
                    .map(|parcel| (parcel.key, parcel.placed)),
            );
        noted
            .into_iter()
            .any(|(key, placed)| placed != seen.contains(&key))
    }
}

/// Keeps what is attached in `hub` in step with the chips among
/// `pieces`.
pub(super) fn reconcile(
    world: &mut World,
    hub: Entity,
    pieces: &[Piece],
) {
    let seen = pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Atom(atom) => atom.key().parse::<u64>().ok(),
            Piece::Text(_) => None,
        })
        .collect::<HashSet<_>>();
    let owner = Owner::of(world, hub);
    if world.resource::<Attached>().is_out_of_step(owner, &seen) {
        world.resource_mut::<Attached>().follow(owner, &seen);
    }
}

/// Lets go of what the drafts of a Hub that is gone carry.
fn hub_gone(
    gone: On<Remove<crate::hub::Hub>>,
    mut attached: ResMut<Attached>,
) {
    let hub = gone.entity;
    attached.release(|owner| owner.hub == hub);
}

/// Takes the attachment `key` and its chip off the line.
fn remove(world: &mut World, key: u64) {
    let mut fields =
        world.query_filtered::<&mut RichField, With<ChatField>>();
    for mut field in fields.iter_mut(world) {
        field.remove_atom(key.to_string());
    }
    let mut attached = world.resource_mut::<Attached>();
    attached.list.retain(|image| image.key != key);
    if let Some(at) =
        attached.parcels.iter().position(|parcel| parcel.key == key)
    {
        let gone = attached.parcels.remove(at);
        if !matches!(gone.load, Load::Kept(_)) {
            attached.forget.push(gone.key);
        }
    }
}

/// The chip of the attachment `key` as an atom that copies as `name`.
fn chip_atom(key: u64, name: &str) -> Atom {
    Atom::new::<HamletTheme>(key.to_string(), name, move || {
        composer_chip(key)
    })
}

/// Puts the chip of the attachment `key` at the caret of `hub`'s
/// field.
fn put_chip(world: &mut World, hub: Entity, key: u64) {
    let name = world.resource::<Attached>().name_of(key);
    super::field::with_field(world, hub, |mut field| {
        field.insert_atom(chip_atom(key, &name));
    });
}

/// `files` grouped by the first part of their names.
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

/// An attachment to take in.
pub(super) enum Incoming {
    File(PathBuf),
    Picture(Vec<u8>),
}

/// The files being read on the task pool.
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

/// Attaches `incoming` to the next line in `hub`.
pub(super) fn attach(
    world: &mut World,
    hub: Entity,
    incoming: Vec<Incoming>,
) {
    let sight = sight(world, Some(hub));
    let owner = Owner::of(world, hub);
    super::tell(world, Some(hub), "");
    for item in incoming {
        let image = match &item {
            Incoming::File(path) => {
                ImageInput::is_image_file(path) && !path.is_dir()
            }
            Incoming::Picture(_) => true,
        };
        if !image {
            if let Incoming::File(path) = item {
                parcel(world, hub, path);
            }
            continue;
        }
        let Sight::Sees(most) = sight else {
            super::tell(
                world,
                Some(hub),
                "This model cannot see images.",
            );
            continue;
        };
        if world.resource::<Attached>().images_of(owner).count()
            >= most
        {
            super::tell(
                world,
                Some(hub),
                &format!("A line takes at most {most} images."),
            );
            continue;
        }
        let mut attached = world.resource_mut::<Attached>();
        let key = attached.next;
        attached.next += 1;
        match item {
            Incoming::File(path) => {
                attached.list.push(Attachment {
                    key,
                    owner,
                    name: file_name(&path),
                    size: None,
                    shown: Some(Source::File(path.clone())),
                    progress: Progress::Sending,
                    placed: false,
                });
                put_chip(world, hub, key);
                // The small picture is read from the file as it is.
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
                    false => Progress::Rejected(
                        "The image is too large.".into(),
                    ),
                };
                let sends = progress == Progress::Sending;
                attached.list.push(Attachment {
                    key,
                    owner,
                    name: "Pasted image".into(),
                    size: Some(bytes.len() as u64),
                    shown: None,
                    progress,
                    placed: false,
                });
                put_chip(world, hub, key);
                if sends {
                    world.trigger(Upload { key, bytes });
                }
            }
        }
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// Attaches the file or folder at `path` in `hub`, which the client
/// counts.
fn parcel(world: &mut World, hub: Entity, path: PathBuf) {
    let name = file_name(&path);
    let folder = path.is_dir();
    let owner = Owner::of(world, hub);
    let mut attached = world.resource_mut::<Attached>();
    let key = attached.next;
    attached.next += 1;
    attached.parcels.push(Parcel {
        key,
        owner,
        name,
        folder,
        skipped: 0,
        load: Load::Reading,
        placed: false,
    });
    put_chip(world, hub, key);
    world.trigger(Gather {
        key,
        paths: vec![path],
    });
}

/// Sends what was counted, unless the total is over the cap.
fn gathered(
    gathered: On<Gathered>,
    mut attached: ResMut<Attached>,
    mut commands: Commands,
) {
    // A cross may have let it go while it was counted.
    let Some(owner) = attached
        .parcels
        .iter()
        .find(|parcel| parcel.key == gathered.key)
        .map(|parcel| parcel.owner)
    else {
        return;
    };
    let (files, bytes) = attached.counted(owner);
    let over = files + gathered.files > AttachedFile::CAP_FILES
        || bytes + gathered.bytes > AttachedFile::CAP_BYTES;
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

fn files_rejected(
    rejected: On<FilesRejected>,
    mut attached: ResMut<Attached>,
) {
    if let Some(parcel) = attached
        .parcels
        .iter_mut()
        .find(|parcel| parcel.key == rejected.key)
    {
        parcel.load = Load::Rejected(rejected.text.clone());
    }
}

/// Tells the client to stop sending the parcels that were let go.
fn forgotten(mut attached: ResMut<Attached>, mut commands: Commands) {
    for key in std::mem::take(&mut attached.forget) {
        commands.trigger(FilesForget { key });
    }
}

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
            Ok(bytes) => {
                attachment.size = Some(bytes.len() as u64);
                commands.trigger(Upload { key, bytes });
            }
            Err(text) => {
                attachment.progress = Progress::Rejected(text);
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

fn rejected(
    rejected: On<ImageRejected>,
    mut attached: ResMut<Attached>,
) {
    if let Some(attachment) = attached
        .list
        .iter_mut()
        .find(|attachment| attachment.key == rejected.key)
    {
        attachment.progress =
            Progress::Rejected(rejected.text.clone());
    }
}

/// Where a Hub asks for files: the dialog or the clipboard, which
/// each answer on their own.
#[derive(Clone, Copy)]
enum Channel {
    Dialog,
    Clipboard,
}

/// The chat a Hub asked the dialog for files in, and the one it
/// asked the clipboard in, so neither's answer lands in the other's.
#[derive(Component, Default)]
struct Asking {
    dialog: Option<Entity>,
    clipboard: Option<Entity>,
}

impl Asking {
    fn of(&mut self, channel: Channel) -> &mut Option<Entity> {
        match channel {
            Channel::Dialog => &mut self.dialog,
            Channel::Clipboard => &mut self.clipboard,
        }
    }
}

/// Notes that `hub` asks `channel` for files for the chat it shows.
fn ask(world: &mut World, hub: Entity, channel: Channel) {
    let chat = convo::chat_of(world, hub);
    let mut hub = world.entity_mut(hub);
    let mut asking = hub.entry::<Asking>().or_default().into_mut();
    *asking.of(channel) = Some(chat);
}

/// The Hub `asker` when it still shows the chat it asked `channel`
/// in, which the answer is taken for.
fn asking(
    world: &mut World,
    asker: Entity,
    channel: Channel,
) -> Option<Entity> {
    let chat = world.get_mut::<Asking>(asker)?.of(channel).take()?;
    (convo::chat_of(world, asker) == chat).then_some(asker)
}

/// Attaches the files and folders the dialog gave the chat.
fn chosen(chosen: On<Chosen>, mut commands: Commands) {
    let (asker, paths) = (chosen.asker, chosen.paths.clone());
    commands.queue(move |world: &mut World| {
        if let Some(hub) = asking(world, asker, Channel::Dialog) {
            let incoming = paths.into_iter().map(Incoming::File);
            attach(world, hub, incoming.collect());
        }
    });
}

/// Asks for the clipboard's files and images on the paste key.
fn paste(
    key: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    fields: Query<&ChatField>,
    mut commands: Commands,
) {
    let held = match cfg!(target_os = "macos") {
        true => [KeyCode::SuperLeft, KeyCode::SuperRight],
        false => [KeyCode::ControlLeft, KeyCode::ControlRight],
    };
    if !key.input.state.is_pressed()
        || key.input.key_code != KeyCode::KeyV
        || !keys.any_pressed(held)
    {
        return;
    }
    let Ok(&ChatField(hub)) = fields.get(key.focused_entity) else {
        return;
    };
    commands.queue(move |world: &mut World| {
        ask(world, hub, Channel::Clipboard);
        world.trigger(Paste { asker: hub });
    });
}

/// Attaches what the clipboard held.
fn pasted(pasted: On<Pasted>, mut commands: Commands) {
    let files = pasted.files.iter().cloned().map(Incoming::File);
    let pictures =
        pasted.pictures.iter().cloned().map(Incoming::Picture);
    let incoming = files.chain(pictures).collect::<Vec<_>>();
    let asker = pasted.asker;
    commands.queue(move |world: &mut World| {
        if let Some(hub) = asking(world, asker, Channel::Clipboard) {
            attach(world, hub, incoming);
        }
    });
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
        commands.queue(move |world: &mut World| {
            if let Some(hub) = receiver(world) {
                attach(world, hub, incoming);
            }
        });
    }
}

/// The image a chip or a tile shows.
#[derive(Component)]
struct Original(ImageId);

/// Opens the image that was clicked in the viewer.
fn open_original(
    click: On<PointerClick>,
    originals: Query<&Original>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(Original(image)) = originals.get(click.entity) else {
        return;
    };
    let image = image.clone();
    commands
        .queue(move |world: &mut World| viewer::open(world, image));
}

/// The picture of `source` cut by `cut`, on a plate while it is read.
fn picture(
    source: Source,
    cut: impl Fn(&Drawn) -> (Vec2, Option<Rect>) + Send + Sync + 'static,
) -> impl View {
    let of = source.clone();
    let state = resource::<Pictures, _>(move |all| all.state(&of));
    let cut = Arc::new(cut);
    keyed(state, move |state| {
        let (source, state, cut) =
            (source.clone(), state.clone(), cut.clone());
        AnyView::new(move |cx: &mut Cx| {
            want(cx.world, &source);
            let within = cx.build(bare());
            let theme = cx.theme();
            let (back, round) =
                (theme.palette.code, theme.space.round_small);
            let mut node = Node {
                width: percent(100.0),
                height: percent(100.0),
                border_radius: BorderRadius::all(px(round)),
                ..Node::default()
            };
            match &state {
                State::Ready(drawn) => {
                    let (size, rect) = cut(drawn);
                    (node.width, node.height) =
                        (px(size.x), px(size.y));
                    let image = ImageNode {
                        rect,
                        ..ImageNode::new(drawn.image.clone())
                            .with_mode(NodeImageMode::Stretch)
                    };
                    cx.world.spawn((node, image, ChildOf(within)));
                }
                _ => {
                    cx.world.spawn((
                        node,
                        BackgroundColor(back),
                        ChildOf(within),
                    ));
                }
            }
            within
        })
    })
    .within(
        column(())
            .width(percent(100.0))
            .height(percent(100.0))
            .justify(JustifyContent::Center)
            .align(AlignItems::Center),
    )
}

/// A small picture of `source` fitted in a square of `side`.
fn thumbnail(source: Source, side: f32) -> impl View {
    let fitted = picture(source, move |drawn| {
        (drawn.fitted(Vec2::splat(side)), None)
    });
    column((fitted,))
        .width(px(side))
        .height(px(side))
        .shrink(0.0)
}

/// The middle square of a picture of `size` pixels.
fn square(size: UVec2) -> Rect {
    let side = size.min_element() as f32;
    let corner = (size.as_vec2() - side) / 2.0;
    Rect::from_corners(corner, corner + side)
}

/// On a sent image's tile.
#[derive(Component)]
struct Tile;

/// The corner of a tile that holds the button opening its image.
#[derive(Component)]
struct Zoom;

/// A sent image's square tile, which opens it in the viewer.
fn tile(image: ImageId) -> AnyView {
    let opened = image.clone();
    let zoom = button(glyph(Glyph::ZoomIn, ICON))
        .padding(UiRect::all(px(4.0)))
        .tip("View image")
        .variant(Variant::Default)
        .on_activate(move |world| {
            viewer::open(world, opened.clone());
        });
    let corner = row((zoom,))
        .position(PositionType::Absolute)
        .inset(UiRect::new(Val::Auto, px(4.0), px(4.0), Val::Auto))
        .tagged(Zoom);
    let shown = picture(Source::Image(image.clone()), |drawn| {
        (Vec2::splat(TILE), Some(square(drawn.size)))
    });
    built(move |cx| {
        let node = cx.build(
            column((shown,))
                .width(px(TILE))
                .height(px(TILE))
                .shrink(0.0)
                .tagged(Tile),
        );
        let corner = cx.build(corner);
        cx.world
            .entity_mut(corner)
            .insert((Visibility::Hidden, ChildOf(node)));
        cx.world
            .entity_mut(node)
            .insert((Original(image), Hovered::default()));
        node
    })
    .boxed()
}

/// The tiles of a sent line's images, in a row that wraps.
pub(super) fn tiles(images: &[AttachedImage]) -> impl View {
    row(images
        .iter()
        .map(|image| tile(image.id.clone()))
        .collect::<Vec<_>>())
    .gap(6.0)
    .tagged(Wraps)
}

/// Shows the button over each tile while it is hovered or focused.
fn reveal(
    focus: Res<InputFocus>,
    tiles: Query<(Ref<Hovered>, &Children), With<Tile>>,
    parents: Query<&ChildOf>,
    mut zooms: Query<&mut Visibility, With<Zoom>>,
) {
    let focused = focus.get();
    for (hovered, children) in &tiles {
        if !hovered.is_changed() && !focus.is_changed() {
            continue;
        }
        for &corner in children {
            let Ok(mut shown) = zooms.get_mut(corner) else {
                continue;
            };
            let held = focused.is_some_and(|input| {
                parents.iter_ancestors(input).any(|up| up == corner)
            });
            shown.set_if_neq(match hovered.get() || held {
                true => Visibility::Inherited,
                false => Visibility::Hidden,
            });
        }
    }
}

/// The parts of a chip of one attachment.
#[derive(Clone, PartialEq)]
struct Face {
    glyph: Glyph,
    /// The small picture of an image, once it has one.
    picture: Option<Source>,
    name: String,
    /// Its size, or how far it has got.
    size: String,
    /// Why it is rejected or held back, which its tooltip says.
    trouble: Option<String>,
    image: bool,
}

fn counted(files: usize) -> String {
    match files {
        1 => "1 file".to_owned(),
        n => format!("{n} files"),
    }
}

/// The size of `files` files of `bytes` bytes, as a chip says it.
fn parcel_words(folder: bool, files: usize, bytes: u64) -> String {
    let size = AttachedFile::size_words(bytes);
    match folder {
        true => format!("{}, {size}", counted(files)),
        false => size,
    }
}

fn held_words(files: usize, bytes: u64) -> String {
    format!("{}, {}", counted(files), AttachedFile::size_words(bytes))
}

/// Why `files` files of `bytes` bytes wait for a word.
fn over(files: usize, bytes: u64) -> String {
    format!(
        "{} ({}) is over what one message takes: {} MiB or {} files.",
        counted(files),
        AttachedFile::size_words(bytes),
        AttachedFile::CAP_BYTES >> 20,
        AttachedFile::CAP_FILES
    )
}

/// `name`, cut in the middle when it is long.
fn shown_name(name: &str) -> String {
    let letters = name.chars().count();
    if letters <= NAME_LETTERS {
        return name.to_owned();
    }
    let head = NAME_LETTERS / 2;
    let tail = NAME_LETTERS - head - 1;
    format!(
        "{}...{}",
        name.chars().take(head).collect::<String>(),
        name.chars().skip(letters - tail).collect::<String>()
    )
}

/// A chip as high as a line of text.
fn chip(
    face: Face,
    disabled: Option<&'static str>,
    cross: Option<u64>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let palette = &theme.palette;
        let (fill, round, words) =
            (palette.code, theme.space.round, theme.text.body);
        let padding = theme.space.chip_padding_x;
        let edge = match &face.trouble {
            Some(_) => palette.danger_border,
            None => palette.gold_5,
        };
        let tone = match (&face.trouble, disabled) {
            (Some(_), _) => Tone::Critical,
            (None, Some(_)) => Tone::Faint,
            (None, None) => Tone::Body,
        };
        let mark = match &face.picture {
            Some(source) => thumbnail(source.clone(), ICON).boxed(),
            None => glyph(face.glyph, ICON).tone(tone).boxed(),
        };
        let mut parts = vec![
            mark,
            label(shown_name(&face.name))
                .size(words)
                .wrap(false)
                .tone(tone)
                .boxed(),
        ];
        if !face.size.is_empty() {
            parts.push(
                label(face.size.clone())
                    .size(words)
                    .wrap(false)
                    .tone(Tone::Faint)
                    .boxed(),
            );
        }
        if let Some(key) = cross {
            parts.push(
                close_button(move |world| remove(world, key)).boxed(),
            );
        }
        let chip = row(parts)
            .gap(5.0)
            .align(AlignItems::Center)
            .height(px(words * LINE))
            .padding(UiRect::horizontal(px(padding)))
            .margin(UiRect::horizontal(px(2.0)))
            .fill(fill)
            .border(1.0)
            .border_color(edge)
            .radius(round)
            .opacity(if disabled.is_some() { 0.6 } else { 1.0 });
        let tip = face
            .trouble
            .clone()
            .or_else(|| disabled.map(str::to_owned));
        let opens = match (&face.picture, face.trouble.is_some()) {
            (Some(Source::Image(image)), false) => {
                Some(Original(image.clone()))
            }
            _ => None,
        };
        let node = match tip {
            Some(tip) => {
                cx.build(chip.tooltip(move || note(tip.clone())))
            }
            None => cx.build(chip),
        };
        if let Some(opens) = opens {
            cx.world.entity_mut(node).insert(opens);
        }
        node
    })
}

/// The chip of the attachment `key` in the composer.
fn composer_chip(key: u64) -> AnyView {
    let seen = derived(move |world: &World| {
        world.resource::<Attached>().face(key).map(|face| {
            let hub = world.resource::<Attached>().hub_of(key);
            (face, sight(world, hub))
        })
    })
    .when(every_frame());
    keyed(seen, move |seen| match seen {
        None => bare().boxed(),
        Some((face, sight)) => {
            let blind = face.image && *sight == Sight::Blind;
            chip(
                face.clone(),
                blind.then_some("This model cannot see images"),
                Some(key),
            )
        }
    })
    .within(bare())
    .boxed()
}

/// Each chip of a sent line with its byte in the words.
pub(super) fn sent_chips(
    images: &[AttachedImage],
    files: &[AttachedFile],
) -> Vec<(Option<u64>, Atom)> {
    let mut chips = Vec::new();
    for (n, image) in images.iter().enumerate() {
        let face = Face {
            glyph: Glyph::Picture,
            picture: Some(Source::Image(image.id.clone())),
            name: "Image".into(),
            size: String::new(),
            trouble: None,
            image: true,
        };
        let key = format!("image {n}");
        chips.push((image.at, sent_atom(key, face)));
    }
    for (n, (name, group)) in by_top(files).into_iter().enumerate() {
        let folder = group.iter().any(|file| file.name.contains('/'));
        let face = Face {
            glyph: match folder {
                true => Glyph::Folder,
                false => Glyph::Notes,
            },
            picture: None,
            name,
            size: parcel_words(
                folder,
                group.len(),
                group.iter().map(|file| file.size).sum(),
            ),
            trouble: None,
            image: false,
        };
        chips.push((
            group[0].at,
            sent_atom(format!("file {n}"), face),
        ));
    }
    chips
}

/// The chip of a sent line's attachment as an atom of its words.
fn sent_atom(key: String, face: Face) -> Atom {
    Atom::new::<HamletTheme>(key, "", move || {
        chip(face.clone(), None, None)
    })
}

/// `words` with an [`ATOM`] at each chip, and the chips in that
/// order.
pub(super) fn marked(
    words: &str,
    chips: Vec<(Option<u64>, Atom)>,
) -> (String, Vec<Atom>) {
    let mut chips = chips
        .into_iter()
        .map(|(at, atom)| {
            let at = at
                .and_then(|at| usize::try_from(at).ok())
                .filter(|at| {
                    *at <= words.len() && words.is_char_boundary(*at)
                })
                .unwrap_or(words.len());
            (at, atom)
        })
        .collect::<Vec<_>>();
    chips.sort_by_key(|(at, _)| *at);
    // So that an [`ATOM`] stands for a chip alone.
    let clean = |words: &str| words.replace(ATOM, "\n");
    let mut marked = String::new();
    let mut atoms = Vec::new();
    let mut from = 0;
    for (at, atom) in chips {
        marked.push_str(&clean(&words[from..at]));
        marked.push(ATOM);
        atoms.push(atom);
        from = at;
    }
    marked.push_str(&clean(&words[from..]));
    (marked, atoms)
}

/// The chips with no place in a line's words, in a row of their own.
pub(super) fn after_words(
    chips: Vec<(Option<u64>, Atom)>,
) -> impl View {
    row(chips
        .into_iter()
        .map(|(_, atom)| rich(vec![Span::atom(atom)]).atoms().boxed())
        .collect::<Vec<_>>())
    .gap(4.0)
    .tagged(Wraps)
}

/// A row over `hub`'s field for each file held back by the cap.
pub(super) fn holds(hub: Entity) -> impl View {
    let held = derived(move |world: &World| {
        let owner = Owner::of(world, hub);
        world
            .resource::<Attached>()
            .parcels_of(owner)
            .filter_map(|parcel| match parcel.load {
                Load::Held { files, bytes } => {
                    Some((parcel.key, over(files, bytes)))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    })
    .when(either(moved::<Attached>(), every_frame()));
    keyed(held, |held| {
        let rows = held
            .iter()
            .map(|(key, words)| {
                let key = *key;
                row((
                    label(words.clone())
                        .size(11.0)
                        .tone(Tone::Critical),
                    hamlet_ui::styles::lettered("Send Anyway")
                        .rules(hamlet_ui::styles::button)
                        .on_activate(move |world| {
                            send_held(world, key);
                        }),
                ))
                .gap(8.0)
                .align(AlignItems::Center)
                .tagged(Wraps)
                .boxed()
            })
            .collect::<Vec<_>>();
        column(rows).gap(4.0).boxed()
    })
    .within(bare())
}

/// Opens the dialog `what` is chosen in, for `hub`'s chat.
pub(crate) fn choose(world: &mut World, hub: Entity, what: What) {
    crate::hub::activate(world, hub);
    ask(world, hub, Channel::Dialog);
    world.trigger(Choose { asker: hub, what });
}

pub(crate) fn sees_images(world: &World, hub: Entity) -> bool {
    matches!(sight(world, Some(hub)), Sight::Sees(_))
}

/// A control that opens the dialog `what` is chosen in for `hub`.
fn chooser(
    hub: Entity,
    which: Glyph,
    tip: &'static str,
    what: What,
) -> AnyView {
    super::tool_button(which, tip, move |world| {
        choose(world, hub, what);
    })
}

/// The buttons of `hub`'s composer that open the attach dialogs.
pub(crate) fn control(hub: Entity) -> impl View {
    let sees = derived(move |world: &World| sight(world, Some(hub)))
        .when(every_frame());
    let images = keyed(sees, move |sight| match sight {
        Sight::Sees(_) => chooser(
            hub,
            Glyph::Picture,
            "Attach images",
            What::Images,
        ),
        Sight::Blind => with_cx(|cx| {
            let side = cx.theme().space.row;
            column((glyph(Glyph::Picture, ICON).tone(Tone::Faint),))
                .width(px(side))
                .height(px(side))
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .tooltip(|| note("This model cannot see images"))
        })
        .boxed(),
    })
    .within(bare());
    with_cx(move |cx| {
        row((
            images,
            chooser(hub, Glyph::Notes, "Attach files", What::Files),
            chooser(
                hub,
                Glyph::Folder,
                "Attach a folder",
                What::Folders,
            ),
        ))
        .gap(super::TOOL_GAP * cx.theme().space.gap)
        .align(AlignItems::Center)
    })
}

/// The indicator shown in `hub` while files are dragged over the
/// window, if they would drop there.
pub(super) fn indicator(hub: Entity) -> impl View {
    let drag = derived(move |world: &World| {
        let dragging = world
            .get_resource::<Attached>()
            .map_or(Dragging::Nothing, |attached| attached.dragging);
        (dragging != Dragging::Nothing
            && receiver(world) == Some(hub))
        .then(|| (dragging, sight(world, Some(hub))))
    })
    .when(every_frame());
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
    use super::*;

    const OWNER: Owner = Owner {
        hub: Entity::PLACEHOLDER,
        chat: Entity::PLACEHOLDER,
    };

    #[test]
    fn a_rewound_line_gives_its_chips_back_at_their_places() {
        let mut attached = Attached::default();
        let image = ImageId::try_from("a".repeat(64)).unwrap();
        // The line had ten bytes before its words.
        let placed = AttachedImage {
            at: Some(13),
            id: image,
        };
        let pieces = attached.give_back(
            OWNER,
            std::slice::from_ref(&placed),
            &[],
            "abcdef",
            10,
        );
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], Piece::Text("abc".into()));
        assert_eq!(pieces[2], Piece::Text("def".into()));
        let Piece::Atom(chip) = &pieces[1] else {
            panic!("a chip between the words");
        };
        // It copies as the name the image has.
        assert_eq!(chip.text(), "Image");
        let key = chip.key().parse::<u64>().unwrap();
        assert_eq!(
            attached.ready(OWNER, &Places::from([(key, 13)])),
            Some((vec![placed], Vec::new()))
        );
        attached.give_back(OWNER, &[], &[], "", 0);
        assert_eq!(
            attached.ready(OWNER, &Places::new()),
            Some((Vec::new(), Vec::new()))
        );
    }

    #[test]
    fn a_chip_taken_out_of_the_field_lets_its_image_go_once_seen() {
        let mut attached = Attached::default();
        let image = ImageId::try_from("b".repeat(64)).unwrap();
        attached.give_back(
            OWNER,
            &[AttachedImage::last(image)],
            &[],
            "",
            0,
        );
        let key = attached.list[0].key;
        // Until its chip is seen in the field, it is kept.
        let nothing = HashSet::new();
        assert!(!attached.is_out_of_step(OWNER, &nothing));
        let seen = HashSet::from([key]);
        assert!(attached.is_out_of_step(OWNER, &seen));
        attached.follow(OWNER, &seen);
        assert!(attached.is_out_of_step(OWNER, &nothing));
        attached.follow(OWNER, &nothing);
        assert!(attached.list.is_empty());
    }
}

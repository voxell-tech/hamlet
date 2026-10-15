//! The pictures a reply shows (docs/chat.md): where each comes from,
//! and its pixels, read once off the frame and kept while they are
//! shown.

mod fetch;
mod load;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::component::Component;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::{UVec2, Vec2};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::tasks::{AsyncComputeTaskPool, TaskPool};
use hamlet_client::Tokio;
use hamlet_client::messages::JobFinished;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

pub(crate) use self::fetch::host;
pub(crate) use self::load::fit;
use self::load::{Bounds, Pixels};
use crate::chat::ChatSettings;
use crate::links::{Anchor, seek};
use crate::markdown::Target;

/// The endings of the files drawn as pictures, in small letters.
const PICTURED: [&str; 6] =
    ["bmp", "gif", "jpeg", "jpg", "png", "webp"];

/// The pixels kept to one of the chat's, so that a picture is sharp
/// on a dense screen.
const DENSITY: f32 = 2.0;

/// The most pictures remembered, shown or not.
const REMEMBERED: usize = 512;

/// The time a file was last written and its length.
type Stamp = (Option<SystemTime>, u64);

/// A picture that was read: its place, the file as it then was, and
/// its pixels if it had any.
type Read = (Source, Option<Stamp>, Option<Pixels>);

/// Reads the pictures of the chat off the frame and keeps them.
pub fn plugin(app: &mut App) {
    app.init_resource::<Pictures>()
        .init_resource::<ChatSettings>()
        .add_observer(show)
        .add_observer(unshow)
        .add_observer(look_again)
        .add_systems(Update, take.run_if(arrived));
}

/// The place a picture comes from.
#[derive(Component, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Source {
    /// A file, with every link on the way to it followed.
    File(PathBuf),
    /// A web address.
    Web(String),
}

impl Source {
    /// The place a click on the picture leads.
    pub(crate) fn anchor(&self) -> Anchor {
        match self {
            Source::File(file) => Anchor::File(file.clone()),
            Source::Web(address) => Anchor::Web(address.clone()),
        }
    }
}

/// A picture that can be drawn.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Drawn {
    pub image: Handle<Image>,
    /// Its size in pixels, as it is kept.
    pub size: UVec2,
}

impl Drawn {
    /// The size it is drawn at where it may be `most` at most.
    pub(crate) fn fitted(&self, most: Vec2) -> Vec2 {
        fit(self.size, most.max(Vec2::ONE).as_uvec2()).as_vec2()
    }
}

/// How far a picture has come.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum State {
    /// Nothing has asked for it.
    Unasked,
    Loading,
    Ready(Drawn),
    /// No picture could be made of it.
    Failed,
}

struct Kept {
    /// The file as it was when it was read. None for the web.
    stamp: Option<Stamp>,
    state: State,
    /// The turn of [`Pictures::clock`] it was last asked for on.
    used: u64,
}

impl Kept {
    fn bytes(&self) -> u64 {
        match &self.state {
            State::Ready(drawn) => {
                u64::from(drawn.size.x) * u64::from(drawn.size.y) * 4
            }
            _ => 0,
        }
    }
}

/// The pictures of the chat, each read once and shared by every row
/// that shows it.
#[derive(Resource)]
pub struct Pictures {
    kept: HashMap<Source, Kept>,
    /// The rows that show each.
    shown: HashMap<Source, u32>,
    clock: u64,
    loading: usize,
    tx: UnboundedSender<Read>,
    rx: UnboundedReceiver<Read>,
}

impl Default for Pictures {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Pictures {
            kept: HashMap::new(),
            shown: HashMap::new(),
            clock: 0,
            loading: 0,
            tx,
            rx,
        }
    }
}

/// The file at `path` as it is now, if it is one.
fn stamp(path: &Path) -> Option<Stamp> {
    let file = std::fs::metadata(path).ok()?;
    file.is_file().then(|| (file.modified().ok(), file.len()))
}

impl Pictures {
    pub(crate) fn state(&self, source: &Source) -> State {
        self.kept
            .get(source)
            .map_or(State::Unasked, |kept| kept.state.clone())
    }

    /// True while a picture is being read.
    pub fn waiting(&self) -> bool {
        self.loading > 0
    }

    fn put(
        &mut self,
        source: Source,
        stamp: Option<Stamp>,
        state: State,
    ) {
        self.clock += 1;
        let used = self.clock;
        let now = state == State::Loading;
        let kept = Kept { stamp, state, used };
        let was = self
            .kept
            .insert(source, kept)
            .is_some_and(|was| was.state == State::Loading);
        match (was, now) {
            (false, true) => self.loading += 1,
            (true, false) => {
                self.loading = self.loading.saturating_sub(1);
            }
            _ => {}
        }
    }

    /// Reads the file `path` on another thread.
    fn read(&mut self, path: &Path, stamp: Stamp, bounds: Bounds) {
        let source = Source::File(path.to_path_buf());
        self.put(source.clone(), Some(stamp), State::Loading);
        let (tx, path) = (self.tx.clone(), path.to_path_buf());
        AsyncComputeTaskPool::get_or_init(TaskPool::default)
            .spawn(async move {
                let pixels = load::read(&path, bounds);
                let _ = tx.send((source, Some(stamp), pixels));
            })
            .detach();
    }

    /// Asks for the picture in the file `path`, which is read if it
    /// never was or has changed since.
    fn ask(&mut self, path: &Path, bounds: Bounds) {
        let source = Source::File(path.to_path_buf());
        let Some(stamp) = stamp(path) else {
            return self.put(source, None, State::Failed);
        };
        match self.kept.get_mut(&source) {
            Some(kept) if kept.stamp == Some(stamp) => {
                self.clock += 1;
                kept.used = self.clock;
            }
            _ if stamp.1 > bounds.bytes => {
                self.put(source, Some(stamp), State::Failed);
            }
            _ => self.read(path, stamp, bounds),
        }
    }

    /// Reads again each file that is shown and has changed.
    fn look_again(&mut self, bounds: Bounds) {
        let changed = self
            .shown
            .keys()
            .filter_map(|source| match source {
                Source::File(path) => Some((source, path)),
                Source::Web(_) => None,
            })
            .filter_map(|(source, path)| {
                let now = stamp(path)?;
                let kept = self.kept.get(source)?;
                (kept.stamp != Some(now) && now.1 <= bounds.bytes)
                    .then(|| (path.clone(), now))
            })
            .collect::<Vec<_>>();
        for (path, now) in changed {
            self.read(&path, now, bounds);
        }
    }

    /// Keeps what was read of `source`, unless its file has been
    /// asked for again as it is since.
    fn settle(
        &mut self,
        source: Source,
        stamp: Option<Stamp>,
        state: State,
    ) {
        let waits = self.kept.get(&source).is_some_and(|kept| {
            kept.stamp == stamp && kept.state == State::Loading
        });
        if waits {
            self.put(source, stamp, state);
        }
    }

    /// Forgets the pictures no row shows, the one asked for longest
    /// ago first, until they take no more than `budget` bytes and
    /// no more than [`REMEMBERED`] are kept in all.
    fn trim(&mut self, budget: u64) {
        let mut idle = self
            .kept
            .iter()
            .filter(|(source, kept)| {
                !self.shown.contains_key(*source)
                    && kept.state != State::Loading
            })
            .map(|(source, kept)| {
                (kept.used, kept.bytes(), source.clone())
            })
            .collect::<Vec<_>>();
        let mut bytes =
            idle.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
        let mut count = self.kept.len();
        if bytes <= budget && count <= REMEMBERED {
            return;
        }
        idle.sort_by_key(|(used, ..)| *used);
        for (_, size, source) in idle {
            if bytes <= budget && count <= REMEMBERED {
                break;
            }
            self.kept.remove(&source);
            bytes -= size;
            count -= 1;
        }
    }
}

/// The bounds a picture is read within, by the chat's tuning.
fn bounds(settings: &ChatSettings) -> Bounds {
    let most =
        Vec2::new(settings.picture_width, settings.picture_height);
    Bounds {
        bytes: settings.picture_bytes,
        pixels: (most * DENSITY).max(Vec2::ONE).as_uvec2(),
    }
}

fn settings(world: &World) -> ChatSettings {
    world
        .get_resource::<ChatSettings>()
        .cloned()
        .unwrap_or_default()
}

/// True when the name of `file` says it is a picture that can be
/// read.
fn pictured(file: &Path) -> bool {
    file.extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            PICTURED.contains(&ending.to_ascii_lowercase().as_str())
        })
}

/// The place the picture written as `target` comes from, its file
/// asked for. Nothing for what is no picture: a file outside the
/// folders of the chat, one of another kind, an address that is not
/// the web's. A picture of the web is only named here: nothing is
/// fetched until [`fetch()`].
pub(crate) fn find(
    world: &mut World,
    target: &Target,
) -> Option<Source> {
    match target {
        Target::Web(address) => {
            host(address).map(|_| Source::Web(address.clone()))
        }
        Target::Path(path) => {
            let file =
                seek(world, path).filter(|file| pictured(file))?;
            let bounds = bounds(&settings(world));
            world
                .get_resource_or_init::<Pictures>()
                .ask(&file, bounds);
            Some(Source::File(file))
        }
    }
}

/// Fetches the picture at `address`, which a chief asked to see.
pub(crate) fn fetch(world: &mut World, address: &str) {
    let source = Source::Web(address.to_string());
    let bounds = bounds(&settings(world));
    let tokio = world.get_resource::<Tokio>().map(|t| t.0.clone());
    let mut pictures = world.get_resource_or_init::<Pictures>();
    if pictures.state(&source) == State::Loading {
        return;
    }
    let Some(tokio) = tokio else {
        return pictures.put(source, None, State::Failed);
    };
    pictures.put(source.clone(), None, State::Loading);
    let (tx, address) = (pictures.tx.clone(), address.to_string());
    tokio.spawn(async move {
        let pixels = fetch::get(address, bounds).await;
        let _ = tx.send((source, None, pixels));
    });
}

fn image(pixels: Pixels) -> Image {
    Image::new(
        Extent3d {
            width: pixels.size.x,
            height: pixels.size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn arrived(pictures: Res<Pictures>) -> bool {
    !pictures.rx.is_empty()
}

/// Keeps the pictures that were read as images.
fn take(
    mut pictures: ResMut<Pictures>,
    mut images: ResMut<Assets<Image>>,
    settings: Res<ChatSettings>,
) {
    while let Ok((source, stamp, pixels)) = pictures.rx.try_recv() {
        let state = match pixels {
            Some(pixels) => State::Ready(Drawn {
                size: pixels.size,
                image: images.add(image(pixels)),
            }),
            None => State::Failed,
        };
        pictures.settle(source, stamp, state);
    }
    pictures.trim(settings.picture_memory);
}

fn show(
    added: On<Add, Source>,
    sources: Query<&Source>,
    mut pictures: ResMut<Pictures>,
) {
    if let Ok(source) = sources.get(added.entity) {
        *pictures.shown.entry(source.clone()).or_default() += 1;
    }
}

fn unshow(
    gone: On<Remove, Source>,
    sources: Query<&Source>,
    settings: Res<ChatSettings>,
    mut pictures: ResMut<Pictures>,
) {
    let Ok(source) = sources.get(gone.entity) else {
        return;
    };
    let rows = pictures.shown.get(source).copied().unwrap_or(0);
    if rows > 1 {
        pictures.shown.insert(source.clone(), rows - 1);
        return;
    }
    pictures.shown.remove(source);
    pictures.clock += 1;
    let now = pictures.clock;
    if let Some(kept) = pictures.kept.get_mut(source) {
        kept.used = now;
    }
    pictures.trim(settings.picture_memory);
}

/// Looks at the files shown again when a turn has ended, which is
/// when an agent has drawn some anew.
fn look_again(
    _: On<JobFinished>,
    settings: Res<ChatSettings>,
    mut pictures: ResMut<Pictures>,
) {
    if !pictures.shown.is_empty() {
        pictures.look_again(bounds(&settings));
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::UVec2;

    use super::load::tests::png;
    use super::*;
    use crate::links::Roots;

    fn ready(side: u32) -> State {
        State::Ready(Drawn {
            image: Handle::default(),
            size: UVec2::splat(side),
        })
    }

    #[test]
    fn a_picture_is_a_file_of_the_chat_or_an_address_never_fetched() {
        let all = hamlet_testing::Scratch::new("pictures-find");
        let folder = all.join("project");
        std::fs::create_dir_all(&folder).unwrap();
        let small = png(UVec2::new(4, 4));
        std::fs::write(folder.join("pear.PNG"), &small).unwrap();
        std::fs::write(folder.join("plan.svg"), "<svg/>").unwrap();
        std::fs::write(all.join("secret.png"), &small).unwrap();
        let large = folder.join("large.png");
        std::fs::write(&large, &small).unwrap();

        let mut world = World::new();
        world.insert_resource(Roots::new([folder.clone()]));
        world.insert_resource(ChatSettings {
            picture_bytes: small.len() as u64,
            ..ChatSettings::default()
        });
        let mut find = |written: &str| {
            let target = match written.contains(':') {
                true => Target::Web(written.to_string()),
                false => Target::Path(written.to_string()),
            };
            find(&mut world, &target)
        };
        let pear = Source::File(
            folder.canonicalize().unwrap().join("pear.PNG"),
        );
        assert_eq!(find("pear.PNG"), Some(pear.clone()));
        assert_eq!(find("plan.svg"), None);
        assert_eq!(find("gone.png"), None);
        assert_eq!(find("../secret.png"), None);
        assert_eq!(find("mailto:chief@hamlet.gg"), None);
        let address = "https://orchard.example/pear.png?key=secret";
        let web = Source::Web(address.to_string());
        assert_eq!(find(address), Some(web.clone()));

        std::fs::write(&large, [small.as_slice(), &[0]].concat())
            .unwrap();
        let large = find("large.png").unwrap();
        let pictures = world.resource::<Pictures>();
        assert_eq!(pictures.state(&pear), State::Loading);
        assert_eq!(pictures.state(&large), State::Failed);
        assert_eq!(pictures.state(&web), State::Unasked);
        assert_eq!(pictures.loading, 1);
    }

    #[test]
    fn pictures_no_row_shows_are_forgotten_oldest_first() {
        let mut pictures = Pictures::default();
        let named = |name: &str| Source::Web(name.to_string());
        for name in ["old", "shown", "new"] {
            pictures.put(named(name), None, ready(10));
        }
        pictures.shown.insert(named("shown"), 1);
        pictures.trim(400);
        assert_eq!(pictures.state(&named("old")), State::Unasked);
        assert_eq!(pictures.state(&named("shown")), ready(10));
        assert_eq!(pictures.state(&named("new")), ready(10));

        // A file read before it changed is not kept as the new one.
        let file = Source::File("pear.png".into());
        let (was, now) = (Some((None, 1)), Some((None, 2)));
        pictures.put(file.clone(), now, State::Loading);
        pictures.settle(file.clone(), was, ready(1));
        assert_eq!(pictures.state(&file), State::Loading);
        pictures.settle(file.clone(), now, ready(2));
        assert_eq!(pictures.state(&file), ready(2));
        assert!(!pictures.waiting());
    }

    #[test]
    fn an_absolute_local_picture_loads_without_a_house_or_work_folder()
     {
        use std::time::{Duration, Instant};

        use bevy::ui::widget::ImageNode;
        use bevy_fynix::tokens::Tone;

        use crate::theme::HamletTheme;
        let scratch =
            hamlet_testing::Scratch::new("absolute-picture");
        let file = scratch.join("pear.png");
        std::fs::write(&file, png(UVec2::new(4, 4))).unwrap();
        let mut app = crate::me::tests::panel_app();
        app.init_resource::<Assets<Image>>().add_plugins(plugin);
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            crate::markdown::fixed(
                &format!("![pear]({})", file.display()),
                Tone::Body,
            ),
        );
        let start = Instant::now();
        loop {
            app.update();
            let image = app
                .world_mut()
                .query::<&ImageNode>()
                .iter(app.world())
                .find_map(|node| {
                    app.world()
                        .resource::<Assets<Image>>()
                        .get(&node.image)
                });
            if let Some(image) = image {
                assert_eq!(image.size(), UVec2::new(4, 4));
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "picture did not load"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

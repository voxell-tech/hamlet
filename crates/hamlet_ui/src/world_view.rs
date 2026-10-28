//! The world in the window: the 3D scene drawn into a dock panel, the
//! sky's hour.

use bevy::app::{App, PostUpdate, PreUpdate, Update};
use bevy::asset::{Assets, RenderAssetUsages};
use bevy::camera::{
    Camera, ImageRenderTarget, NormalizedRenderTarget, RenderTarget,
};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::{Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{
    Commands, Local, Query, Res, ResMut, Single,
};
use bevy::ecs::world::World;
use bevy::image::{Image, ToExtents};
use bevy::math::UVec2;
use bevy::picking::pointer::PointerLocation;
use bevy::render::render_resource::{
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::state::condition::in_state;
use bevy::state::state::{OnEnter, OnExit};
use bevy::time::Time;
use bevy::ui::widget::ViewportNode;
use bevy::ui::{Display, Node, Overflow, UiSystems, px};
use bevy::window::{PrimaryWindow, Window};
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_fynix::ViewExt;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, frame};
use chrono::Timelike;
use hamlet_client::realm::Busy;
use hamlet_client::waits::Waiting;
use hamlet_realm::bubble::Bubble;
use hamlet_realm::hand::Readout;
use hamlet_realm::needs::NeedTag;
use hamlet_realm::pick::{NameTag, Sign, WorldSurface};
use hamlet_realm::sky::SkySettings;
use hamlet_realm::{
    Hour, North, RealmSystems, Sky, Stage, WorldCamera,
};

use crate::dock::PanelAppExt;
use crate::icons::Glyph;
use crate::styles::{Fills, ICON};
use crate::window::WorldShown;
use crate::{View, built};

type WorldCameras<'w, 's> = Query<
    'w,
    's,
    &'static mut Camera,
    (With<WorldCamera>, Without<Window>),
>;

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "world",
        DockWindowKind::new("REALM", || {
            column((view(),))
                .fills_width()
                .min_height(px(0.0))
                .gap(0.0)
                .boxed()
        })
        .icon(Glyph::Globe.image(ICON)),
    )
    .init_resource::<TimeOfDay>()
    .init_resource::<NorthTarget>()
    .init_resource::<Gliding>()
    .insert_resource(hamlet_realm::pick::NoNameTag)
    .add_systems(
        Update,
        (
            (glide_sky, drive).chain().in_set(RealmSystems::Steer),
            pace.run_if(in_state(WorldShown)),
            adopt_tags,
            follow_scale,
        ),
    )
    .add_systems(
        PreUpdate,
        match_pointer_target
            .after(bevy::picking::pointer::PointerInput::receive)
            .before(bevy::picking::backend::ray::RayMap::repopulate),
    )
    .add_systems(PostUpdate, cap_picture.after(UiSystems::PostLayout))
    .add_systems(OnEnter(WorldShown), draw)
    .add_systems(OnExit(WorldShown), rest);
}

/// The camera that draws the world into the panel.
#[derive(Resource)]
pub(crate) struct WorldCam(Entity);

/// Marks the panel node the world is drawn in.
#[derive(Component)]
pub(crate) struct WorldPanel;

/// The sky following the clock, or held at day, at night or at an
/// hour.
#[derive(
    Resource,
    Clone,
    Copy,
    PartialEq,
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TimeOfDay {
    #[default]
    Clock,
    Day,
    Night,
    /// An hour of the day, 0 to 24.
    At(f32),
}

impl TimeOfDay {
    /// The hour it holds the sky at; none while it follows the clock.
    pub(crate) fn held(self, set: &SkySettings) -> Option<f32> {
        match self {
            Self::Clock => None,
            Self::Day => Some(set.held(false)),
            Self::Night => Some(set.held(true)),
            Self::At(hour) => Some(hour.rem_euclid(24.0)),
        }
    }

    /// The hour the sky goes to: the one held, or the clock's.
    pub(crate) fn target(self, set: &SkySettings) -> f32 {
        self.held(set).unwrap_or_else(wall_hour)
    }
}

fn wall_hour() -> f32 {
    let now = chrono::Local::now();
    now.hour() as f32 + now.minute() as f32 / 60.0
}

/// Degrees north is to be at; [`North`] glides to it.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub(crate) struct NorthTarget(pub(crate) f32);

/// True while the sky glides to the hour or the north chosen.
#[derive(Resource, Default)]
pub(crate) struct Gliding(bool);

/// The share of what is left that the sky closes each second, as the
/// rate of its fall.
const GLIDE: f32 = 4.0;

/// The way from `from` to `to` round a circle of `whole`, the short
/// way, clockwise positive.
fn way(from: f32, to: f32, whole: f32) -> f32 {
    (to - from + whole / 2.0).rem_euclid(whole) - whole / 2.0
}

/// Moves the sky's hour and north toward the ones chosen, the short
/// way round. The first frame puts them there at once.
pub(crate) fn glide_sky(
    time: Res<Time>,
    (chosen, set, sky): (Res<TimeOfDay>, Res<SkySettings>, Res<Sky>),
    target: Res<NorthTarget>,
    mut north: ResMut<North>,
    mut hour: ResMut<Hour>,
    mut gliding: ResMut<Gliding>,
    mut began: Local<bool>,
) {
    let k = if *began {
        1.0 - (-GLIDE * time.delta_secs()).exp()
    } else {
        1.0
    };
    *began = true;
    let to = chosen.target(&set);
    let from = hour.0.unwrap_or(sky.hour);
    let left = way(from, to, 24.0);
    let turning = left.abs() > 1e-3;
    let at = if turning {
        (from + left * k).rem_euclid(24.0)
    } else {
        to
    };
    // Once it has caught the clock up, the sky follows it itself.
    let held = (turning || *chosen != TimeOfDay::Clock).then_some(at);
    if hour.0 != held {
        hour.0 = held;
    }
    let left = way(north.0, target.0, 360.0);
    let swinging = left.abs() > 1e-2;
    let at = if swinging {
        (north.0 + left * k).rem_euclid(360.0)
    } else {
        target.0
    };
    if north.0 != at {
        north.0 = at;
    }
    if gliding.0 != (turning || swinging) {
        gliding.0 = turning || swinging;
    }
}

/// Points the world's camera at a texture a panel can show.
pub(crate) fn attach(world: &mut World) {
    let Some(camera) = world
        .query_filtered::<Entity, With<WorldCamera>>()
        .iter(world)
        .next()
    else {
        return;
    };
    // A UI pixel is `scale` texture pixels, so the camera's viewport
    // is measured in the same units as the panel and labels
    // placed over the scene line up.
    let scale = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or(1.0, Window::scale_factor);
    let mut image = Image::new_uninit(
        Default::default(),
        TextureDimension::D2,
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::all(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    let mut entity = world.entity_mut(camera);
    entity.insert(RenderTarget::Image(ImageRenderTarget {
        handle,
        scale_factor: scale,
    }));
    if let Some(mut cam) = entity.get_mut::<Camera>() {
        // Before the window's own camera, which draws the panels.
        cam.order = -1;
    }
    world.insert_resource(WorldCam(camera));
}

/// Keeps the world's picture at the window's pixel scale, which
/// changes when the window moves to a display of another density.
pub(crate) fn follow_scale(
    window: Single<&Window, With<PrimaryWindow>>,
    mut target: Single<&mut RenderTarget, With<WorldCamera>>,
) {
    let scale = window.scale_factor();
    if let RenderTarget::Image(image) = &**target
        && image.scale_factor != scale
    {
        let handle = image.handle.clone();
        **target = RenderTarget::Image(ImageRenderTarget {
            handle,
            scale_factor: scale,
        });
    }
}

/// Keeps the world's picture no larger than the window. Bevy sizes it
/// from the panel's layout, and a minimized window on Windows lays
/// the panel out at a size far past what the GPU allows, which fails
/// the bloom texture and every pass after it.
pub(crate) fn cap_picture(
    window: Single<&Window, With<PrimaryWindow>>,
    target: Single<&RenderTarget, With<WorldCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(handle) = target.as_image() else {
        return;
    };
    let Some(image) = images.get(handle) else {
        return;
    };
    let most =
        UVec2::new(window.physical_width(), window.physical_height())
            .max(UVec2::ONE);
    if image.size().cmpgt(most).any() {
        let size = image.size().min(most);
        if let Some(mut image) = images.get_mut(handle) {
            image.resize(size.to_extents());
        }
    }
}

/// Gives the pointer forwarded into the world panel the camera's own
/// target. Bevy forwards it as if the picture's scale were one, which
/// names a target no camera has, and nothing in the world could be
/// hit.
pub(crate) fn match_pointer_target(
    window: Query<Entity, With<PrimaryWindow>>,
    cameras: Query<&RenderTarget, With<WorldCamera>>,
    mut pointers: Query<&mut PointerLocation, With<ViewportNode>>,
) {
    let Some(ours) = cameras
        .iter()
        .next()
        .and_then(|target| target.normalize(window.single().ok()))
    else {
        return;
    };
    let NormalizedRenderTarget::Image(picture) = &ours else {
        return;
    };
    for mut pointer in &mut pointers {
        let stale = pointer.location.as_ref().is_some_and(|at| {
            matches!(
                &at.target,
                NormalizedRenderTarget::Image(theirs)
                    if theirs.handle == picture.handle && theirs != picture
            )
        });
        if stale && let Some(at) = &mut pointer.location {
            at.target = ours.clone();
        }
    }
}

/// The panel: an ordinary frame whose node shows the world camera's
/// picture.
pub fn view() -> impl View {
    built(|cx| {
        let camera = cx.world.get_resource::<WorldCam>().map(|c| c.0);
        // The tags, signs and balloons hang off this node at the
        // camera's viewport coordinates, so one that runs past the
        // panel is cut here.
        let panel = frame()
            .fills_width()
            .min_height(px(0.0))
            .overflow(Overflow::clip())
            .tagged(WorldPanel)
            .tagged(WorldSurface);
        match camera {
            Some(camera) => {
                cx.build(panel.seeded(ViewportNode::new(camera)))
            }
            None => cx.build(panel),
        }
    })
}

/// Hangs every label placed in the camera's viewport off the panel
/// that shows it.
pub(crate) fn adopt_tags(
    mut commands: Commands,
    panel: Query<Entity, With<WorldPanel>>,
    tags: Query<
        (Entity, Option<&ChildOf>),
        Or<(
            With<NameTag>,
            With<Sign>,
            With<NeedTag>,
            With<Bubble>,
            With<Readout>,
        )>,
    >,
    mut away: Local<Option<Entity>>,
) {
    // With no panel to hang off, they are put away where nothing is
    // drawn.
    let panel = panel.iter().next().unwrap_or_else(|| {
        *away.get_or_insert_with(|| {
            commands
                .spawn(Node {
                    display: Display::None,
                    ..Node::default()
                })
                .id()
        })
    });
    for (tag, parent) in &tags {
        if parent.map(ChildOf::parent) != Some(panel) {
            commands.entity(tag).try_insert(ChildOf(panel));
        }
    }
}

/// The sky's night state, at the hour the sky is at, not the one it
/// glides to, so the two never disagree.
pub(crate) fn drive(
    hour: Res<Hour>,
    sky_set: Res<SkySettings>,
    mut stage: ResMut<Stage>,
) {
    let night = sky_set.is_night(hour.0.unwrap_or_else(wall_hour));
    if stage.night != night {
        stage.night = night;
    }
}

/// Draws every frame while something moves, and at the window's slow
/// tick otherwise.
pub(crate) fn pace(
    busy: Query<(), With<Busy>>,
    waiting: Query<(), With<Waiting>>,
    gliding: Res<Gliding>,
    mut winit: ResMut<WinitSettings>,
) {
    let moving = !busy.is_empty() || !waiting.is_empty() || gliding.0;
    let mode = if moving {
        UpdateMode::Continuous
    } else {
        crate::window::winit_settings().focused_mode
    };
    if winit.focused_mode != mode {
        winit.focused_mode = mode;
    }
}

/// Draws the world while it is in sight.
pub fn draw(mut cameras: WorldCameras) {
    for mut camera in &mut cameras {
        camera.is_active = true;
    }
}

/// Draws nothing while the world is out of sight.
pub fn rest(mut cameras: WorldCameras) {
    for mut camera in &mut cameras {
        camera.is_active = false;
    }
}

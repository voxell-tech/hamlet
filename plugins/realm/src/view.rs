//! The world in the window: the 3D scene drawn into a dock panel, the
//! sky's hour, the mayor's state on the stage and the chief's mascot
//! beside whoever the chat is with.

mod chief;

use bevy::app::{App, PostUpdate, PreUpdate, Update};
use bevy::asset::{Assets, RenderAssetUsages};
use bevy::camera::{
    Camera, ImageRenderTarget, NormalizedRenderTarget, RenderTarget,
};
use bevy::ecs::change_detection::DetectChangesMut;
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
use bevy::state::state::{OnEnter, OnExit};
use bevy::ui::widget::ViewportNode;
use bevy::ui::{Display, Node, Overflow, UiSystems, px};
use bevy::window::{PrimaryWindow, Window};
use bevy_fynix::ViewExt;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, frame};
use chrono::Timelike;
use hamlet_agents_api::agents::Busy;
use hamlet_agents_api::convo::Me;
use hamlet_agents_api::oversight::Waiting;
use hamlet_agents_api::session::{Entry, Status, Transcript};
use hamlet_realm::bubble::Bubble;
use hamlet_realm::hand::Readout;
use hamlet_realm::keeper::KState;
use hamlet_realm::needs::NeedTag;
use hamlet_realm::pick::{NameTag, Orbit, Sign, WorldSurface};
use hamlet_realm::sky::SkySettings;
use hamlet_realm::{Hour, Stage, WorldCamera, WorldSystems};
use hamlet_realm_api::sky::TimeOfDay;
use hamlet_sdk::frame::{Animating, Mounting, WorldShown};
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::looks::Fills;
use hamlet_sdk::menu::Menu;
use hamlet_sdk::plugin::{AppPluginExt, HamletPlugin, Manifest};
use hamlet_sdk::theme::Look;
use hamlet_sdk::{AnyView, Cx};

/// The section of the lines a menu always has.
const MAIN: u32 = 0;

type WorldCameras<'w, 's> = Query<
    'w,
    's,
    &'static mut Camera,
    (With<WorldCamera>, Without<Window>),
>;

/// Adds the World View plugin. Add it after the realm's plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins(chief::plugin)
        .init_resource::<TimeOfDay>()
        .init_resource::<Animating>()
        .insert_resource(hamlet_realm::pick::NoNameTag)
        .add_systems(
            Update,
            (
                drive.before(WorldSystems),
                pace,
                adopt_tags,
                follow_scale,
                hold_sky.before(WorldSystems),
            ),
        )
        .add_systems(
            PreUpdate,
            match_pointer_target
                .after(bevy::picking::pointer::PointerInput::receive)
                .before(
                    bevy::picking::backend::ray::RayMap::repopulate,
                ),
        )
        .add_systems(
            PostUpdate,
            cap_picture.after(UiSystems::PostLayout),
        )
        // The world panel shows the world's camera, which the world
        // has spawned by now.
        .add_systems(OnEnter(Look::Ready), attach.before(Mounting))
        .add_systems(OnEnter(WorldShown), draw)
        .add_systems(OnExit(WorldShown), rest)
        .add_hamlet_plugin::<WorldViewPlugin>();
}

/// The plugin that shows the world in a panel, with the VIEW lines of
/// its camera and its sky.
pub struct WorldViewPlugin;

impl HamletPlugin for WorldViewPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("world-view").named("World View")
    }

    fn entry(entry: &mut hamlet_sdk::plugin::Entry) {
        entry.panel("world", "REALM", Glyph::Globe, || {
            column((view(),))
                .fills_width()
                .min_height(px(0.0))
                .gap(0.0)
                .boxed()
        });
        entry.command("reset-camera", "Reset Camera", |commands| {
            commands.insert_resource(Orbit::default());
        });
        entry.command(
            "sky-clock",
            "Sky Follows the Clock",
            |commands| {
                commands.insert_resource(TimeOfDay::Clock);
            },
        );
        entry.command("sky-day", "Sky at Day", |commands| {
            commands.insert_resource(TimeOfDay::Day);
        });
        entry.command("sky-night", "Sky at Night", |commands| {
            commands.insert_resource(TimeOfDay::Night);
        });
        for (label, command, glyph) in [
            ("Reset Camera", "reset-camera", Glyph::Aim),
            ("Sky Follows the Clock", "sky-clock", Glyph::Clock),
            ("Sky at Day", "sky-day", Glyph::Sun),
            ("Sky at Night", "sky-night", Glyph::Moon),
        ] {
            entry.menu_item(
                Menu::View,
                MAIN,
                label,
                command,
                Some(glyph),
            );
        }
    }
}

/// The camera that draws the world into the panel.
#[derive(Resource)]
pub(crate) struct WorldCam(Entity);

/// Marks the panel node the world is drawn in.
#[derive(Component)]
pub(crate) struct WorldPanel;

/// True when the sky `sky` asks for is a night sky.
fn night(sky: TimeOfDay, set: &SkySettings) -> bool {
    match sky {
        TimeOfDay::Day => false,
        TimeOfDay::Night => true,
        TimeOfDay::Clock => {
            let now = chrono::Local::now();
            set.night(now.hour() as f32 + now.minute() as f32 / 60.0)
        }
    }
}

/// Holds the sky at the middle of the day or of the night, or lets it
/// follow the clock.
pub(crate) fn hold_sky(
    sky: Res<TimeOfDay>,
    set: Res<SkySettings>,
    mut hour: ResMut<Hour>,
) {
    let held = match *sky {
        TimeOfDay::Clock => None,
        TimeOfDay::Day => Some(set.held(false)),
        TimeOfDay::Night => Some(set.held(true)),
    };
    if hour.0 != held {
        hour.0 = held;
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
        .map_or(1.0, |w| w.scale_factor());
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
pub fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
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
#[allow(clippy::type_complexity)]
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

/// Sets the stage from the session this window talks to.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive(
    me: Res<Me>,
    sky: Res<TimeOfDay>,
    sky_set: Res<SkySettings>,
    sessions: Query<(&Status, &Transcript)>,
    mut stage: ResMut<Stage>,
) {
    let (working, failed) = match sessions.get(me.mayor) {
        Ok((status, transcript)) => (
            status.working(),
            matches!(transcript.last(), Some(Entry::Error(_))),
        ),
        Err(_) => (false, false),
    };
    let kstate = match (working, failed) {
        (true, _) => KState::Working,
        (false, true) => KState::Fault,
        (false, false) => KState::Rest,
    };
    let next = Stage {
        night: night(*sky, &sky_set),
        kstate,
        ..stage.clone()
    };
    if *stage != next {
        *stage = next;
    }
}

/// Asks for every frame while something in the world moves.
fn pace(
    stage: Res<Stage>,
    busy: Query<(), With<Busy>>,
    waiting: Query<(), With<Waiting>>,
    mut animating: ResMut<Animating>,
) {
    let moving =
        stage.animating() || !busy.is_empty() || !waiting.is_empty();
    animating.set_if_neq(Animating(moving));
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

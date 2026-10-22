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
use hamlet_realm::{Hour, Stage, WorldCamera, WorldSystems};

use crate::dock::PanelAppExt;
use crate::icons::Glyph;
use crate::looks::{Fills, ICON};
use crate::window::WorldShown;
use crate::{AnyView, Cx};

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
    .insert_resource(hamlet_realm::pick::NoNameTag)
    .add_systems(
        Update,
        (
            drive.before(WorldSystems),
            pace.run_if(in_state(WorldShown)),
            adopt_tags,
            follow_scale,
            hold_sky.before(WorldSystems),
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

/// The sky following the clock, or held at day or night.
#[derive(
    Resource,
    Clone,
    Copy,
    PartialEq,
    Eq,
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
}

impl TimeOfDay {
    fn night(self, set: &SkySettings) -> bool {
        match self {
            Self::Day => false,
            Self::Night => true,
            Self::Clock => {
                let now = chrono::Local::now();
                set.night(
                    now.hour() as f32 + now.minute() as f32 / 60.0,
                )
            }
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

/// The sky's night state.
pub(crate) fn drive(
    sky: Res<TimeOfDay>,
    sky_set: Res<SkySettings>,
    mut stage: ResMut<Stage>,
) {
    let night = sky.night(&sky_set);
    if stage.night != night {
        stage.night = night;
    }
}

/// Draws every frame while something moves, and at the window's slow
/// tick otherwise.
pub fn pace(
    busy: Query<(), With<Busy>>,
    waiting: Query<(), With<Waiting>>,
    mut winit: ResMut<WinitSettings>,
) {
    let moving = !busy.is_empty() || !waiting.is_empty();
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

#[cfg(test)]
mod tests {
    use hamlet_protocol::client as protocol;

    use super::*;

    #[test]
    fn drawing_pace_follows_received_work_and_waits_without_a_stage()
    {
        let mut app = App::new();
        app.add_plugins(hamlet_client::ClientPlugin { server: None });
        app.insert_resource(crate::window::winit_settings());
        app.add_systems(Update, pace);
        let idle = crate::window::winit_settings().focused_mode;
        for (busy, waiting, expected) in [
            (false, false, idle),
            (true, false, UpdateMode::Continuous),
            (false, true, UpdateMode::Continuous),
            (false, false, idle),
        ] {
            let mut state = hamlet_testing::realm::state();
            state.agents[0].busy = busy;
            if waiting {
                state.waits.push(hamlet_testing::realm::wait("pace"));
            }
            hamlet_client::receive(
                app.world_mut(),
                protocol::ServerMessage::State(state),
            );
            app.update();
            assert_eq!(
                app.world().resource::<WinitSettings>().focused_mode,
                expected
            );
        }
    }
}

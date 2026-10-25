//! The viewer: one [`Subject`] drawn apart from the world, by a
//! camera of its own that orbits it, into a picture a panel shows and
//! turns by hand.

mod dummy;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use bevy::window::RequestRedraw;

pub use self::dummy::Dummy;
use crate::WorldSystems;
use crate::diorama::{daylight, lens, picture};
use crate::sky::Looks;

/// The render layer the viewer draws on.
const LAYER: usize = 10;

/// Where a subject stands: under the world, apart from the
/// dioramas.
const AWAY: Vec3 = Vec3::new(0.0, -9000.0, 600.0);

/// The pixels of the picture until a panel shows it, and the most of
/// a side.
const FIRST: UVec2 = UVec2::new(640, 480);
const MOST_SIDE: u32 = 2048;

pub fn plugin(app: &mut App) {
    app.init_resource::<Viewer>()
        .init_resource::<ViewerSettings>()
        .add_systems(
            Update,
            (keep, lay, aim)
                .chain()
                .run_if(|viewer: Res<Viewer>| viewer.is_busy())
                .before(WorldSystems),
        );
}

/// Something the viewer draws: it raises itself about the origin.
pub trait Subject: Send + Sync + 'static {
    /// Raises the subject and returns its root; None while it cannot
    /// be yet, and it is asked again the next update.
    fn raise(&mut self, world: &mut World) -> Option<Entity>;

    /// Where the camera looks at it from at first.
    fn framing(&self) -> Framing;

    /// Runs each update while it stands and is in sight.
    fn tend(&mut self, _world: &mut World, _root: Entity) {}

    /// Runs once it is let go of, after its root is despawned.
    fn release(&mut self, _world: &mut World) {}
}

/// The point a camera looks at, about the subject's origin, and its
/// distance from it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Framing {
    pub middle: Vec3,
    pub distance: f32,
}

/// How the viewer is turned and drawn.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewerSettings {
    /// The camera's field of view.
    pub fov: f32,
    /// The camera's turn about the subject from +Z, and its height
    /// as an angle, at first.
    pub yaw: f32,
    pub pitch: f32,
    /// The lowest and highest the camera is raised, as an angle.
    pub lowest: f32,
    pub highest: f32,
    /// The nearest and farthest the camera comes, in shares of the
    /// subject's distance.
    pub nearest: f32,
    pub farthest: f32,
    /// Radians the camera turns for each pixel dragged, and the
    /// share of its distance it comes nearer for each pixel of
    /// the wheel.
    pub drag: f32,
    pub wheel: f32,
    /// Seconds left alone before it turns by itself, and the radians
    /// it turns a second then.
    pub idle: f32,
    pub spin: f32,
}

/// The subject shown, how the camera is turned about it, and the
/// picture it is drawn into.
#[derive(Resource, Default)]
pub struct Viewer {
    /// The pixels of the picture shown, while a panel shows it.
    pub sight: Option<UVec2>,
    shown: Option<Shown>,
    /// Those shown before, to let go of.
    gone: Vec<Shown>,
    orbit: Option<Orbit>,
    asked: Asked,
    image: Option<Handle<Image>>,
    eye: Option<Entity>,
    /// The camera draws.
    drawing: bool,
}

/// A subject shown and its root, once raised.
struct Shown {
    key: String,
    subject: Box<dyn Subject>,
    root: Option<Entity>,
}

/// The camera about the subject.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    /// The share of the subject's distance.
    zoom: f32,
    /// Seconds since it was turned by hand.
    idle: f32,
}

/// What the panel asked of the camera since the last update.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Asked {
    drag: Vec2,
    wheel: f32,
    reset: bool,
}

impl Viewer {
    /// Shows `subject` in place of what was shown, its camera put
    /// back. A subject of the `key` shown already is dropped.
    pub fn show(
        &mut self,
        key: impl Into<String>,
        subject: impl Subject,
    ) {
        let key = key.into();
        if self.key() == Some(key.as_str()) {
            return;
        }
        self.gone.extend(self.shown.take());
        self.shown = Some(Shown {
            key,
            subject: Box::new(subject),
            root: None,
        });
        self.reset();
    }

    /// Shows nothing.
    pub fn clear(&mut self) {
        self.gone.extend(self.shown.take());
    }

    /// The key of the subject shown.
    pub fn key(&self) -> Option<&str> {
        self.shown.as_ref().map(|shown| shown.key.as_str())
    }

    /// Whether the subject shown stands.
    pub fn is_raised(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.root.is_some())
    }

    /// Turns the camera as for a drag of `pixels`.
    pub fn drag(&mut self, pixels: Vec2) {
        self.asked.drag += pixels;
    }

    /// Brings the camera nearer as for `pixels` of the wheel, farther
    /// when negative.
    pub fn wheel(&mut self, pixels: f32) {
        self.asked.wheel += pixels;
    }

    /// Puts the camera back where it was at first.
    pub fn reset(&mut self) {
        self.asked = Asked {
            reset: true,
            ..default()
        };
    }

    /// Whether there is anything to do: a subject to let go of or to
    /// raise, or a picture in sight or still drawn.
    fn is_busy(&self) -> bool {
        !self.gone.is_empty()
            || self.sight.is_some()
            || self.drawing
            || self
                .shown
                .as_ref()
                .is_some_and(|shown| shown.root.is_none())
    }
}

impl Orbit {
    fn new(set: &ViewerSettings) -> Self {
        Orbit {
            yaw: set.yaw,
            pitch: set.pitch,
            zoom: 1.0,
            idle: 0.0,
        }
    }

    /// Moved by what was `asked` and by `seconds` passing.
    fn step(
        mut self,
        asked: Asked,
        seconds: f32,
        set: &ViewerSettings,
    ) -> Self {
        if asked.reset {
            self = Orbit::new(set);
        }
        if asked.drag != Vec2::ZERO || asked.wheel != 0.0 {
            self.idle = 0.0;
        } else {
            self.idle += seconds;
        }
        // Dragged right, the subject turns right: the camera left.
        self.yaw -= asked.drag.x * set.drag;
        self.pitch = (self.pitch + asked.drag.y * set.drag)
            .clamp(set.lowest, set.highest);
        self.zoom = (self.zoom * (-asked.wheel * set.wheel).exp())
            .clamp(set.nearest, set.farthest);
        if self.idle >= set.idle {
            self.yaw += set.spin * seconds;
        }
        self.yaw %= std::f32::consts::TAU;
        self
    }

    /// The camera's place about a subject framed by `framing`.
    fn sight(&self, framing: Framing) -> Transform {
        let middle = AWAY + framing.middle;
        let from = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        Transform::from_translation(
            middle + from * framing.distance * self.zoom,
        )
        .looking_at(middle, Vec3::Y)
    }
}

/// The picture the viewer draws into. None where no realm has a
/// viewer.
pub fn image(world: &mut World) -> Option<Handle<Image>> {
    if let Some(image) = &world.get_resource::<Viewer>()?.image {
        return Some(image.clone());
    }
    let image =
        world.resource_mut::<Assets<Image>>().add(picture(FIRST));
    world.resource_mut::<Viewer>().image = Some(image.clone());
    Some(image)
}

/// The root a subject's own root stands under, at [`AWAY`].
#[derive(Component)]
struct Stand;

/// The camera of the viewer.
#[derive(Component)]
struct Eye;

/// Lets go of the subjects shown before, raises the one shown once it
/// can be, and tends it while it stands.
fn keep(world: &mut World) {
    let mut viewer = world.resource_mut::<Viewer>();
    let gone = std::mem::take(&mut viewer.gone);
    let shown = viewer.shown.take();
    for mut one in gone {
        if let Some(root) = one.root
            && let Ok(root) = world.get_entity_mut(root)
        {
            root.despawn();
        }
        one.subject.release(world);
    }
    let Some(mut shown) = shown else {
        return;
    };
    let seen = world.resource::<Viewer>().sight.is_some();
    match shown.root {
        Some(root) if seen => shown.subject.tend(world, root),
        Some(_) => {}
        None => {
            shown.root = shown.subject.raise(world).map(|raised| {
                let stand = world
                    .spawn((
                        Stand,
                        Transform::from_translation(AWAY),
                        Visibility::Visible,
                    ))
                    .id();
                world.entity_mut(raised).insert(ChildOf(stand));
                stand
            });
        }
    }
    let mut viewer = world.resource_mut::<Viewer>();
    // Nothing else is shown between the take and here: the world is
    // this system's alone.
    viewer.shown = Some(shown);
}

/// Puts every part of the subject on the viewer's layer, those it
/// grows as it loads and moves among them.
fn lay(
    mut commands: Commands,
    stands: Query<Entity, With<Stand>>,
    children: Query<&Children>,
    laid: Query<(), With<RenderLayers>>,
) {
    let layer = RenderLayers::layer(LAYER);
    for stand in &stands {
        for part in std::iter::once(stand)
            .chain(children.iter_descendants(stand))
        {
            if !laid.contains(part) {
                commands.entity(part).try_insert(layer.clone());
            }
        }
    }
}

/// Turns the camera as the panel asked, or by itself when left
/// alone, and draws while the picture is shown.
fn aim(
    mut commands: Commands,
    mut viewer: ResMut<Viewer>,
    (set, looks, time): (Res<ViewerSettings>, Res<Looks>, Res<Time>),
    mut images: ResMut<Assets<Image>>,
    mut eyes: Query<
        (
            &mut Camera,
            &mut RenderTarget,
            &mut Projection,
            &mut Transform,
        ),
        With<Eye>,
    >,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let viewer = &mut *viewer;
    // Until the subject stands, the camera draws the empty stage, so
    // the picture of the one shown before is not seen.
    let framing =
        viewer.shown.as_ref().map(|shown| match shown.root {
            Some(_) => shown.subject.framing(),
            None => Framing {
                middle: Vec3::ZERO,
                distance: 1.0,
            },
        });
    let wanted = viewer.sight.zip(framing).zip(viewer.image.clone());
    viewer.drawing = wanted.is_some();
    let Some(((size, framing), image)) = wanted else {
        for (mut camera, ..) in &mut eyes {
            if camera.is_active {
                camera.is_active = false;
            }
        }
        return;
    };
    redraw.write(RequestRedraw);
    if viewer.eye.is_none() {
        let layer = RenderLayers::layer(LAYER);
        daylight(&mut commands, &looks.day, &layer, 60.0);
        viewer.eye = Some(
            commands
                .spawn((
                    Eye,
                    lens(&looks.day, set.fov, (0.05, 200.0)),
                    Camera {
                        order: -2,
                        is_active: false,
                        clear_color: ClearColorConfig::Custom(
                            Color::NONE,
                        ),
                        ..default()
                    },
                    layer,
                ))
                .id(),
        );
        return;
    }
    let asked = std::mem::take(&mut viewer.asked);
    let orbit = viewer.orbit.unwrap_or_else(|| Orbit::new(&set));
    let orbit = orbit.step(asked, time.delta_secs(), &set);
    viewer.orbit = Some(orbit);
    let Ok((mut camera, mut target, mut projection, mut at)) =
        eyes.single_mut()
    else {
        return;
    };
    let size = size.clamp(UVec2::ONE, UVec2::splat(MOST_SIDE));
    let resized =
        images.get(&image).is_some_and(|drawn| drawn.size() != size);
    if resized && let Some(mut drawn) = images.get_mut(&image) {
        drawn.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    let aimed = matches!(
        &*target,
        RenderTarget::Image(at) if at.handle == image
    );
    if !aimed {
        *target = RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: 1.0,
        });
    }
    if !aimed || resized {
        // Bevy measures a camera's picture again only when its
        // projection or its target's asset changes.
        projection.set_changed();
    }
    if !camera.is_active {
        camera.is_active = true;
    }
    at.set_if_neq(orbit.sight(framing));
}

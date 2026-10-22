//! The pointer: the chief's taper, whose flame is the point, and the
//! chief's glove for what is held. Each mode is a picture drawn in
//! code, set on the window as a custom cursor.

mod art;

use bevy::app::{App, PostUpdate, PreUpdate};
use bevy::asset::{Assets, Handle};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::{
    resource_changed, resource_exists,
};
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::image::Image;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::time::{Real, Time};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button;
use bevy::window::{
    CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow,
    SystemCursorIcon,
};
use bevy_fynix::{EntityCursor, OverrideCursor};

use crate::pointer::art::Inks;
use crate::theme::EngineTheme;

/// The share of each pixel's alpha a faint pointer keeps, so what it
/// holds is seen through it.
const FAINT: f32 = 0.4;

/// Pictures a second of a pointer that moves.
const FRAME_RATE: f32 = 10.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PreUpdate,
        draw.run_if(resource_exists::<Assets<Image>>)
            .run_if(resource_changed::<EngineTheme>),
    )
    .add_systems(
        PostUpdate,
        show.run_if(resource_exists::<Pointers>),
    );
}

/// The line a resizing pointer lies along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeAxis {
    /// West to east.
    Across,
    /// North to south.
    Upright,
    /// North-west to south-east.
    Falling,
    /// South-west to north-east.
    Rising,
}

/// What the pointer says can be done where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerMode {
    Point,
    /// Over what can be pressed.
    Press,
    /// Over a thing of a scene that can be selected.
    Pick,
    /// Over what can be taken hold of.
    Grab,
    /// Holding something.
    Carry,
    /// Sliding the ground of a view.
    Slide,
    /// Turning a view.
    Turn,
    /// Over text that can be typed in.
    Write,
    Resize(ResizeAxis),
    /// What is asked is not allowed here.
    Refuse,
    /// About to be busy.
    Wait,
}

/// On a UI node: the pointer shown while the mouse is over it or
/// what it holds, unless what it holds asks for another.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodePointer {
    pub mode: PointerMode,
    /// Drawn faint, so what is under it is seen through it.
    pub faint: bool,
}

impl NodePointer {
    /// The pointer `mode`, drawn whole.
    pub fn new(mode: PointerMode) -> Self {
        Self { mode, faint: false }
    }
}

impl PointerMode {
    /// Every mode, each axis of a resize its own.
    fn all() -> impl Iterator<Item = Self> {
        [
            Self::Point,
            Self::Press,
            Self::Pick,
            Self::Grab,
            Self::Carry,
            Self::Slide,
            Self::Turn,
            Self::Write,
        ]
        .into_iter()
        .chain(
            [
                ResizeAxis::Across,
                ResizeAxis::Upright,
                ResizeAxis::Falling,
                ResizeAxis::Rising,
            ]
            .map(Self::Resize),
        )
        .chain([Self::Refuse, Self::Wait])
    }

    /// The mode a cursor of the system stands for, as fynix's widgets
    /// ask for one. None for the plain arrow and what means nothing
    /// here.
    fn of_system(icon: SystemCursorIcon) -> Option<Self> {
        use SystemCursorIcon as S;
        Some(match icon {
            S::Pointer => Self::Press,
            S::Cell | S::Crosshair => Self::Pick,
            S::Grab => Self::Grab,
            S::Grabbing => Self::Carry,
            S::Move | S::AllScroll => Self::Slide,
            S::Text | S::VerticalText => Self::Write,
            S::EResize | S::WResize | S::EwResize | S::ColResize => {
                Self::Resize(ResizeAxis::Across)
            }
            S::NResize | S::SResize | S::NsResize | S::RowResize => {
                Self::Resize(ResizeAxis::Upright)
            }
            S::NwResize | S::SeResize | S::NwseResize => {
                Self::Resize(ResizeAxis::Falling)
            }
            S::NeResize | S::SwResize | S::NeswResize => {
                Self::Resize(ResizeAxis::Rising)
            }
            S::NoDrop | S::NotAllowed => Self::Refuse,
            S::Wait | S::Progress => Self::Wait,
            _ => return None,
        })
    }
}

/// The pictures of every mode, whole and faint.
#[derive(Resource)]
struct Pointers(Vec<(PointerMode, [Vec<Handle<Image>>; 2])>);

impl Pointers {
    /// The picture of `mode` at `seconds`, the faint one if `faint`.
    fn at(
        &self,
        mode: PointerMode,
        seconds: f32,
        faint: bool,
    ) -> Option<&Handle<Image>> {
        let (_, both) = self.0.iter().find(|(of, _)| *of == mode)?;
        let faces = &both[usize::from(faint)];
        faces.get((seconds * FRAME_RATE) as usize % faces.len())
    }
}

/// Leaves [`FAINT`] of every pixel's alpha.
fn fade(image: &mut Image) {
    for alpha in image.data.iter_mut().flatten().skip(3).step_by(4) {
        *alpha = (*alpha as f32 * FAINT) as u8;
    }
}

/// Draws every pointer in the theme's colours.
fn draw(
    theme: Res<EngineTheme>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    let inks = Inks::from(&theme.colors);
    let pointers = PointerMode::all()
        .map(|mode| {
            let (mut whole, mut faint) = (Vec::new(), Vec::new());
            for frame in 0..mode.frames() {
                let image = mode.face(frame, &inks);
                let mut faded = image.clone();
                fade(&mut faded);
                whole.push(images.add(image));
                faint.push(images.add(faded));
            }
            (mode, [whole, faint])
        })
        .collect::<Vec<_>>();
    commands.insert_resource(Pointers(pointers));
}

/// The pointer the hovered node or the nearest node above it asks
/// for: its own [`NodePointer`], the mode of a system cursor fynix
/// gave it, or a button's.
fn asked(
    hovered: Entity,
    parents: &Query<&ChildOf>,
    nodes: &Query<(
        Option<&NodePointer>,
        Option<&EntityCursor>,
        Has<Button>,
        Has<InteractionDisabled>,
    )>,
) -> Option<NodePointer> {
    core::iter::once(hovered)
        .chain(parents.iter_ancestors(hovered))
        .find_map(|entity| {
            let (own, system, button, disabled) =
                nodes.get(entity).ok()?;
            own.copied()
                .or_else(|| {
                    system
                        .and_then(|cursor| {
                            PointerMode::of_system(cursor.0)
                        })
                        .map(NodePointer::new)
                })
                .or_else(|| {
                    button.then(|| {
                        NodePointer::new(if disabled {
                            PointerMode::Refuse
                        } else {
                            PointerMode::Press
                        })
                    })
                })
        })
}

/// Shows the pointer of the mode that holds: a drag's, then what is
/// under the mouse asks for, then [`PointerMode::Point`].
fn show(
    pointers: Res<Pointers>,
    time: Res<Time<Real>>,
    dragged: Option<Res<OverrideCursor>>,
    hover_map: Option<Res<HoverMap>>,
    parents: Query<&ChildOf>,
    nodes: Query<(
        Option<&NodePointer>,
        Option<&EntityCursor>,
        Has<Button>,
        Has<InteractionDisabled>,
    )>,
    windows: Query<
        (Entity, Option<&CursorIcon>),
        With<PrimaryWindow>,
    >,
    mut commands: Commands,
) {
    let held = dragged
        .and_then(|dragged| dragged.0)
        .and_then(PointerMode::of_system)
        .map(NodePointer::new);
    let hovered = || {
        let topmost = hover_map
            .as_deref()?
            .get(&PointerId::Mouse)?
            .iter()
            .min_by(|(_, a), (_, b)| a.depth.total_cmp(&b.depth))
            .map(|(entity, _)| *entity)?;
        asked(topmost, &parents, &nodes)
    };
    let pointer = held
        .or_else(hovered)
        .unwrap_or(NodePointer::new(PointerMode::Point));
    let Some(image) =
        pointers.at(pointer.mode, time.elapsed_secs(), pointer.faint)
    else {
        return;
    };
    let wanted =
        CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle: image.clone(),
            hotspot: pointer.mode.hotspot(),
            ..CustomCursorImage::default()
        }));
    for (window, shown) in &windows {
        // Fynix sets a cursor of the system over its widgets each
        // frame it wants one; it is replaced here.
        if shown != Some(&wanted) {
            commands.entity(window).try_insert(wanted.clone());
        }
    }
}

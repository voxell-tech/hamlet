//! An asset's pictures, taken evenly about it, shown one at a time as
//! if it stood on a turntable: it turns slowly by itself until it is
//! touched, then by a drag across it or by its arrows.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::picking::events::{Drag, DragStart, Pointer};
use bevy::time::Time;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, Display,
    Node, UiRect, percent, px,
};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_model::wardrobe::Asset;

use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{CONTROL, ICON, bare, brass, note, raised};
use crate::pictures::{Pictures, Source, State};
use crate::{AnyView, Cx};

/// The seconds a turn takes by itself.
const TURN: f32 = 8.0;

/// The pixels a drag goes across it for one picture.
const STEP: f32 = 14.0;

/// The widest it is drawn.
const WIDTH: f32 = 420.0;

pub(super) fn plugin(app: &mut App) {
    app.add_observer(grab).add_observer(drag).add_systems(
        Update,
        (turn, develop.run_if(resource_changed::<Pictures>)),
    );
}

/// The first picture of an asset in a card, drawn once it has come.
#[derive(Component)]
pub(super) struct Still;

/// Where a turntable is turned to, in pictures.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Turn {
    at: f32,
    /// It was dragged or stepped, and turns by itself no more.
    touched: bool,
    /// Where it was when the drag began.
    grabbed: f32,
}

impl Turn {
    /// Turns it by itself for `seconds`, while it is untouched.
    fn tick(&mut self, seconds: f32, frames: u32) {
        if !self.touched && frames > 0 {
            self.at = (self.at + seconds * frames as f32 / TURN)
                .rem_euclid(frames as f32);
        }
    }

    fn grab(&mut self) {
        self.touched = true;
        self.grabbed = self.at;
    }

    /// Turns it to where a drag `across` pixels from its start leads.
    fn drag(&mut self, across: f32) {
        self.at = self.grabbed + across / STEP;
    }

    /// Turns it by `by` pictures from the one shown.
    fn step(&mut self, by: f32) {
        self.touched = true;
        self.at = self.at.round() + by;
    }

    /// The picture shown of `frames`, which wraps round both ways.
    fn frame(&self, frames: u32) -> usize {
        (self.at.round() as i64).rem_euclid(i64::from(frames.max(1)))
            as usize
    }
}

/// A turntable: the pictures of every turn of its asset, and where it
/// is turned to.
#[derive(Component)]
struct Spinner {
    frames: Vec<Source>,
    turn: Turn,
}

/// The turntable of `asset`, with every one of its pictures asked
/// for.
pub(super) fn turntable(asset: &Asset) -> AnyView {
    let paths = (0..asset.frames)
        .map(|frame| asset.frame(frame))
        .collect::<Vec<_>>();
    let picture = AnyView::new(move |cx: &mut Cx| {
        let frames = paths
            .iter()
            .map(|path| super::ask_picture(cx.world, path))
            .collect::<Vec<_>>();
        let theme = cx.theme();
        let (ground, edge, round) = (
            theme.palette.well,
            theme.palette.brass_6,
            theme.space.round,
        );
        let within = cx.build(bare().width(percent(100.0)));
        let shown = cx
            .world
            .spawn((
                Node {
                    width: percent(100.0),
                    max_width: px(WIDTH),
                    aspect_ratio: Some(1.0),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(round)),
                    ..Node::default()
                },
                BackgroundColor(ground),
                BorderColor::all(edge),
                ChildOf(within),
            ))
            .id();
        // Each picture held while the turntable is shown, so that
        // none is forgotten while it turns.
        for source in &frames {
            cx.world.spawn((
                Node {
                    display: Display::None,
                    ..Node::default()
                },
                source.clone(),
                ChildOf(shown),
            ));
        }
        cx.world.entity_mut(shown).insert(Spinner {
            frames,
            turn: Turn::default(),
        });
        within
    });
    column((
        picture,
        row((
            arrow(Glyph::Before, -1.0),
            note("Drag across it to turn it."),
            arrow(Glyph::Into, 1.0),
        ))
        .gap(10.0)
        .align(AlignItems::Center),
    ))
    .gap(8.0)
    .boxed()
}

/// A button that turns every turntable shown `by` pictures.
fn arrow(which: Glyph, by: f32) -> AnyView {
    button(raised(glyph(which, ICON)))
        .padding(UiRect::axes(px(10.0), px(0.0)))
        .min_height(px(CONTROL))
        .tooltip(move || {
            label(match by < 0.0 {
                true => "Turn it back",
                false => "Turn it on",
            })
            .size(12.0)
        })
        .rules(brass)
        .tagged(Mood::Plain)
        .on_activate(move |world: &mut World| {
            let mut spinners = world.query::<&mut Spinner>();
            for mut spinner in spinners.iter_mut(world) {
                spinner.turn.step(by);
            }
        })
        .boxed()
}

fn grab(
    start: On<Pointer<DragStart>>,
    mut spinners: Query<&mut Spinner>,
) {
    if let Ok(mut spinner) = spinners.get_mut(start.entity) {
        spinner.turn.grab();
    }
}

fn drag(moved: On<Pointer<Drag>>, mut spinners: Query<&mut Spinner>) {
    if let Ok(mut spinner) = spinners.get_mut(moved.entity) {
        spinner.turn.drag(moved.distance.x);
    }
}

/// Turns each turntable that is untouched, and shows the picture it
/// is turned to once that has come.
fn turn(
    time: Res<Time>,
    pictures: Option<Res<Pictures>>,
    mut spinners: Query<(
        Entity,
        &mut Spinner,
        Option<&mut ImageNode>,
    )>,
    mut commands: Commands,
) {
    let Some(pictures) = pictures else {
        return;
    };
    for (entity, mut spinner, image) in &mut spinners {
        let frames = spinner.frames.len() as u32;
        spinner.turn.tick(time.delta_secs(), frames);
        let Some(source) =
            spinner.frames.get(spinner.turn.frame(frames))
        else {
            continue;
        };
        // The last picture stays until the next has come.
        let State::Ready(drawn) = pictures.state(source) else {
            continue;
        };
        match image {
            Some(mut image) if image.image != drawn.image => {
                image.image = drawn.image;
            }
            Some(_) => {}
            None => {
                commands.entity(entity).insert(
                    ImageNode::new(drawn.image)
                        .with_mode(NodeImageMode::Stretch),
                );
            }
        }
    }
}

/// Draws the first picture of each card once it has come.
fn develop(
    pictures: Res<Pictures>,
    stills: Query<(Entity, &Source), (With<Still>, With<Node>)>,
    images: Query<(), With<ImageNode>>,
    mut commands: Commands,
) {
    for (entity, source) in &stills {
        if images.contains(entity) {
            continue;
        }
        if let State::Ready(drawn) = pictures.state(source) {
            commands.entity(entity).insert(
                ImageNode::new(drawn.image)
                    .with_mode(NodeImageMode::Stretch),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_turns_it_round_both_ways() {
        let mut turn = Turn::default();
        turn.grab();
        turn.drag(-STEP);
        assert_eq!(turn.frame(24), 23);
        turn.drag(STEP * 25.0);
        assert_eq!(turn.frame(24), 1);
        turn.drag(-STEP * 49.0);
        assert_eq!(turn.frame(24), 23);
        // A drag is from where it was grabbed, not from the last.
        turn.grab();
        turn.drag(STEP * 2.0);
        assert_eq!(turn.frame(24), 1);
    }

    #[test]
    fn it_turns_by_itself_until_it_is_touched() {
        let mut turn = Turn::default();
        turn.tick(TURN / 4.0, 24);
        assert_eq!(turn.frame(24), 6);
        turn.tick(TURN, 24);
        assert_eq!(turn.frame(24), 6);
        turn.step(-1.0);
        turn.tick(TURN / 4.0, 24);
        assert_eq!(turn.frame(24), 5);
    }
}

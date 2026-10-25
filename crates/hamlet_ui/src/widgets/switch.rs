//! The switch: a track with a knob that slides from the start while
//! off to the end while on.

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy::ui::{AlignItems, Checked, Node, px};
use bevy::ui_widgets::{Checkbox as CheckboxBehavior, ValueChange};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{FrameProps, frame, label, row};
use bevy_fynix::{EntityCursor, Prop, component, style};

use super::fade::ease;
use crate::theme::{HamletTheme, Themed};
use crate::{AnyView, Cx, View};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, slide);
}

/// A handler run with the world and the state the switch was asked
/// to take.
type Change = Box<dyn Fn(&mut World, bool) + Send + Sync>;

/// On a switch's root: what runs when it is activated.
#[derive(Component)]
struct ChangeHandler(Change);

/// On a switch's knob: its switch, how far the knob slides when the
/// switch is on, and how far it has.
#[derive(Component)]
struct Knob {
    switch: Entity,
    travel: f32,
    at: Option<f32>,
}

/// A switch showing `on`. It never flips itself: a press hands the
/// state asked for to `change`, and a bound `on` follows what that
/// writes.
pub fn switch(
    on: impl Into<Prop<bool>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> impl View {
    switch_of(on.into(), None, change)
}

/// A [`switch`] with `text` beside it, pressed as one with it.
pub fn labelled_switch(
    text: impl Into<Prop<String>>,
    on: impl Into<Prop<bool>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> impl View {
    switch_of(on.into(), Some(text.into()), change)
}

fn switch_of(
    on: Prop<bool>,
    text: Option<Prop<String>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> impl View {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (space, palette) = (&theme.space, &theme.palette);
        let (width, height, inset, border, gap) = (
            space.switch_width,
            space.switch_height,
            space.switch_inset,
            space.border,
            space.gap,
        );
        let knob = height - 2.0 * (border + inset);
        let travel = width - height;
        let tracks = (palette.brass_4, palette.grey_4);
        let frames = (palette.brass_3, palette.brass_5);
        let knobs = (palette.brass_1, palette.grey_1);
        cx.scope(|cx| {
            style::<HamletTheme>()
                .tone(Tone::Faint)
                .hovered(|s| s.tone(Tone::Body))
                .pressed(|s| s.tone(Tone::Dim))
                .transition(Motion::Interact)
                .apply(cx);
            let root =
                cx.build(row(()).gap(gap).align(AlignItems::Center));
            cx.world
                .entity_mut(root)
                .insert((
                    CheckboxBehavior,
                    EntityCursor(SystemCursorIcon::Pointer),
                    ChangeHandler(Box::new(change)),
                ))
                .observe(changed);
            cx.effect(root, on, |world, node, &on| {
                let Ok(mut entity) = world.get_entity_mut(node)
                else {
                    return;
                };
                if !on {
                    entity.remove::<Checked>();
                } else if !entity.contains::<Checked>() {
                    entity.insert(Checked);
                }
            });
            let by_state = move |(on, off): (Color, Color)| {
                component::<Checked, _>(root, move |checked| {
                    if checked.is_some() { on } else { off }
                })
            };
            cx.under(root, |cx| {
                let track = cx.build(
                    frame()
                        .width(px(width))
                        .height(px(height))
                        .shrink(0.0)
                        .radius(height / 2.0)
                        .border(border)
                        .border_color(by_state(frames))
                        .fill(by_state(tracks))
                        .padding(bevy::ui::UiRect::all(px(inset)))
                        .align(AlignItems::Center),
                );
                cx.under(track, |cx| {
                    let slider = cx.build(
                        frame()
                            .width(px(knob))
                            .height(px(knob))
                            .radius(knob / 2.0)
                            .fill(by_state(knobs)),
                    );
                    cx.world.entity_mut(slider).insert(Knob {
                        switch: root,
                        travel,
                        at: None,
                    });
                });
                if let Some(text) = text {
                    cx.build(label(text));
                }
            });
            root
        })
    })
}

fn changed(change: On<ValueChange<bool>>, mut commands: Commands) {
    let node = change.event_target();
    let value = change.value;
    commands.queue(move |world: &mut World| {
        let Some(ChangeHandler(handler)) = world
            .get_entity_mut(node)
            .ok()
            .and_then(|mut entity| entity.take::<ChangeHandler>())
        else {
            return;
        };
        handler(world, value);
        if let Ok(mut entity) = world.get_entity_mut(node) {
            entity.insert(ChangeHandler(handler));
        }
    });
}

/// Slides each knob toward the end its switch is at, and sets one
/// there at once when it comes.
fn slide(
    time: Res<Time>,
    theme: Themed,
    on: Query<(), With<Checked>>,
    mut knobs: Query<(&mut Knob, &mut Node)>,
) {
    let rate = theme.motion.interact.recip();
    let dt = time.delta_secs();
    for (mut knob, mut node) in &mut knobs {
        let to = if on.contains(knob.switch) {
            knob.travel
        } else {
            0.0
        };
        let at = knob.at.map_or(to, |at| ease(at, to, rate, dt));
        if knob.at != Some(at) {
            knob.at = Some(at);
            node.left = px(at);
        }
    }
}

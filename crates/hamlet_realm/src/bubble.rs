//! The balloon over a robot with its agent's words or thought.

mod words;

use bevy::prelude::*;
use bevy::ui::UiSystems;
use hamlet_assets::tint;
use hamlet_client::realm::{Activity, Busy, Doing, Saying};

use self::words::{deed, gist, tail};
use super::generator::Robot;
use super::hamlet::Housed;
use super::label::{self, Lens};
use super::sound::{Cue, Sound};
use crate::pick::PickSettings;

/// Draws the balloons of every agent that is [`Saying`] something.
pub fn plugin(app: &mut App) {
    app.init_resource::<BubbleSettings>()
        .add_systems(
            PostUpdate,
            (bubbles, dots, pop_cues)
                .chain()
                .before(UiSystems::Layout),
        )
        .add_systems(Update, unpin);
}

/// The bubbles over robots.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BubbleSettings {
    /// The widest a balloon grows, the size of its letters and the
    /// rounding of its corners, in pixels.
    pub width: f32,
    pub size: f32,
    pub round: f32,
    /// The most letters it shows, counted from the end of what was
    /// said, and the most of the gist of a thought.
    pub most: usize,
    pub most_thought: usize,
    /// Height it floats over a robot's head.
    pub lift: f32,
    /// Camera distance beyond which balloons are not shown.
    pub far: f32,
    /// Seconds a balloon stays after its agent's last word, when the
    /// agent has finished and while it is still at work.
    pub linger: f32,
    pub linger_busy: f32,
    /// Seconds its words stay before a thought with no words yet
    /// takes their place.
    pub hold: f32,
    /// Seconds it takes to pop in, and to shrink away.
    pub arrive: f32,
    pub leave: f32,
    /// The paper, the ink and the frame of speech, and of a thought.
    /// They are the panels' own card and lettering.
    #[serde(with = "tint")]
    pub paper: Color,
    #[serde(with = "tint")]
    pub ink: Color,
    #[serde(with = "tint")]
    pub edge: Color,
    #[serde(with = "tint")]
    pub paper_thought: Color,
    #[serde(with = "tint")]
    pub ink_thought: Color,
    #[serde(with = "tint")]
    pub edge_thought: Color,
    /// The shadow a balloon casts.
    #[serde(with = "tint")]
    pub shade: Color,
}

/// The bubble of an agent: a paper balloon over its robot.
#[derive(Component)]
pub struct Bubble {
    of: Entity,
    /// Seconds since its agent last said anything, and since it
    /// appeared.
    quiet: f32,
    age: f32,
    balloon: Entity,
    words: Entity,
    dots: Entity,
    /// The point of a speech balloon, and the small rounds that
    /// trail from a thought.
    point: Entity,
    trail: Entity,
    /// The words of what its agent says now and the deed they were
    /// made with. Nothing until they are made.
    said: Option<(String, Option<Doing>)>,
    /// The words it shows, whether they are a thought, and the
    /// seconds since they last changed.
    shown: String,
    thought: bool,
    steady: f32,
}

/// On an agent whose balloon is held open for this many seconds
/// more, however far the camera is and whether or not it has just
/// said anything.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(crate) struct Pinned(pub f32);

/// One of the dots of a thought that has no words yet.
#[derive(Component)]
struct Dot(f32);

/// Lets each pin run out.
pub(super) fn unpin(
    mut commands: Commands,
    time: Res<Time>,
    mut pinned: Query<(Entity, &mut Pinned)>,
) {
    for (agent, mut pin) in &mut pinned {
        pin.0 -= time.delta_secs();
        if pin.0 <= 0.0 {
            commands.entity(agent).try_remove::<Pinned>();
        }
    }
}

/// A small pop where a balloon appears: one for speech, a softer
/// one for a thought.
fn pop_cues(
    new: Query<&Bubble, Added<Bubble>>,
    agents: Query<(&Housed, &Saying)>,
    robots: Query<(&Robot, &GlobalTransform)>,
    mut cues: MessageWriter<Cue>,
) {
    for bubble in &new {
        let Ok((housed, saying)) = agents.get(bubble.of) else {
            continue;
        };
        let Some((_, place)) =
            robots.iter().find(|(robot, _)| robot.agent == housed.0)
        else {
            continue;
        };
        cues.write(Cue {
            sound: if saying.thought {
                Sound::Think
            } else {
                Sound::Speak
            },
            at: place.translation(),
            turn: bubble.of.index_u32(),
        });
    }
}

type Speaker<'a> = (
    Entity,
    &'a Housed,
    Ref<'a, Saying>,
    Has<Busy>,
    Option<&'a Pinned>,
    Option<&'a Activity>,
);

/// A round of `side` pixels.
fn round(side: f32, fill: Color) -> impl Bundle {
    (
        Node {
            width: px(side),
            height: px(side),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(fill),
        Pickable::IGNORE,
    )
}

/// A balloon with nothing in it yet, for `agent`.
fn spawn(
    commands: &mut Commands,
    agent: Entity,
    set: &BubbleSettings,
) {
    let words = commands
        .spawn((
            Text::new(""),
            TextFont::from_font_size(set.size),
            TextColor(set.ink),
            Pickable::IGNORE,
        ))
        .id();
    let dots = commands
        .spawn((
            Node {
                column_gap: px(5),
                padding: UiRect::axes(px(2), px(4)),
                ..default()
            },
            Pickable::IGNORE,
            children![
                (Dot(0.0), round(6.0, set.ink_thought)),
                (Dot(1.0), round(6.0, set.ink_thought)),
                (Dot(2.0), round(6.0, set.ink_thought)),
            ],
        ))
        .id();
    let balloon = commands
        .spawn((
            Node {
                max_width: px(set.width),
                padding: UiRect::axes(px(10), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(set.round)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(set.paper),
            BorderColor::all(set.edge),
            BoxShadow::new(set.shade, px(0), px(3), px(0), px(10)),
            Pickable::IGNORE,
        ))
        .add_children(&[words, dots])
        .id();
    // A square on its corner, half under the balloon.
    let point = commands
        .spawn((
            Node {
                width: px(12),
                height: px(12),
                margin: UiRect::top(px(-7)),
                // The two sides that show under the balloon.
                border: UiRect {
                    right: px(1),
                    bottom: px(1),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(set.paper),
            BorderColor::all(set.edge),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            Pickable::IGNORE,
        ))
        .id();
    let trail = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(3),
                margin: UiRect::top(px(3)),
                ..default()
            },
            Pickable::IGNORE,
            children![
                round(8.0, set.paper_thought),
                round(5.0, set.paper_thought),
            ],
        ))
        .id();
    commands
        .spawn((
            Bubble {
                of: agent,
                quiet: 0.0,
                age: 0.0,
                balloon,
                words,
                dots,
                point,
                trail,
                said: None,
                shown: String::new(),
                thought: false,
                steady: 0.0,
            },
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            UiTransform::from_scale(Vec2::ZERO),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .add_children(&[balloon, point, trail]);
}

fn show(node: &mut Node, shown: bool) {
    let want = if shown { Display::Flex } else { Display::None };
    if node.display != want {
        node.display = want;
    }
}

/// The size of a balloon `t` of the way into popping out: past its
/// size a little, then back.
fn pop(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + 2.2 * t * t * t + 1.2 * t * t
}

/// Keeps a balloon over every robot whose agent is saying or
/// thinking something, and lets it go a while after the agent falls
/// silent.
#[allow(clippy::too_many_arguments)]
fn bubbles(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<BubbleSettings>,
    pick: Res<PickSettings>,
    lens: Lens,
    agents: Query<Speaker>,
    robots: Query<(
        Entity,
        &Robot,
        &GlobalTransform,
        &InheritedVisibility,
    )>,
    mut shown: Query<(
        Entity,
        &mut Bubble,
        &mut Node,
        &mut Visibility,
        &mut UiTransform,
        &ComputedNode,
    )>,
    mut parts: Query<&mut Node, Without<Bubble>>,
    mut fills: Query<(&mut BackgroundColor, &mut BorderColor)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let (camera, view) = lens.camera();
    let eye = view.translation();
    let dt = time.delta_secs();
    for (agent, _, saying, _, pinned, _) in &agents {
        if (saying.is_changed() || pinned.is_some())
            && !shown.iter().any(|(_, b, ..)| b.of == agent)
        {
            spawn(&mut commands, agent, &set);
        }
    }
    for (
        id,
        mut bubble,
        mut node,
        mut visibility,
        mut grown,
        computed,
    ) in &mut shown
    {
        let Ok((_, housed, saying, busy, pinned, activity)) =
            agents.get(bubble.of)
        else {
            commands.entity(id).try_despawn();
            continue;
        };
        let pinned = pinned.is_some();
        bubble.quiet = if saying.is_changed() || pinned {
            0.0
        } else {
            bubble.quiet + dt
        };
        bubble.age += dt;
        // One that is still at work keeps its balloon, and one that
        // has finished keeps its last words for a while.
        let linger = if busy { set.linger_busy } else { set.linger };
        if bubble.quiet > linger + set.leave {
            commands.entity(id).try_despawn();
            continue;
        }
        // Speech is shown as it is said. A thought shows only its
        // subject, or the agent's doing meanwhile. The words are made
        // again only when one of those changed.
        let doing = activity.filter(|_| busy).map(|a| a.doing);
        let kept = bubble.said.as_ref().is_some_and(|(_, with)| {
            *with == doing
                && !saying.is_changed()
                && !set.is_changed()
        });
        if !kept {
            let words = if saying.thought {
                gist(&saying.text, set.most_thought)
                    .or_else(|| deed(doing?).map(String::from))
                    .unwrap_or_default()
            } else {
                tail(&saying.text, set.most)
            };
            bubble.said = Some((words, doing));
        }
        let robot = robots
            .iter()
            .find(|(_, robot, ..)| robot.agent == housed.0)
            .filter(|(.., seen)| seen.get());
        let point = robot.and_then(|(robot, _, was, _)| {
            let top = lens.stands(robot, was).translation()
                + Vec3::Y * (pick.robot_box.y + set.lift);
            (pinned || eye.distance(top) < set.far)
                .then(|| camera.world_to_viewport(&view, top).ok())
                .flatten()
        });
        let Some(point) = point else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let bubble = &mut *bubble;
        let made = bubble
            .said
            .as_ref()
            .map_or("", |(words, _)| words.as_str());
        // Words stay their while when what follows has none yet,
        // and give way at once to other words.
        bubble.steady += dt;
        let held = made.is_empty()
            && !bubble.shown.is_empty()
            && bubble.steady < set.hold;
        if !held {
            if bubble.shown != made {
                bubble.shown = made.to_string();
                bubble.steady = 0.0;
            }
            bubble.thought = saying.thought;
        }
        let words = bubble.shown.as_str();
        let thought = bubble.thought;
        let wordless = words.is_empty();
        let (paper, ink, edge) = if thought {
            (set.paper_thought, set.ink_thought, set.edge_thought)
        } else {
            (set.paper, set.ink, set.edge)
        };
        if let Ok((mut text, mut colour)) =
            texts.get_mut(bubble.words)
        {
            if text.0 != words {
                text.0 = words.to_string();
            }
            if colour.0 != ink {
                colour.0 = ink;
            }
        }
        for part in [bubble.balloon, bubble.point] {
            if let Ok((mut fill, mut frame)) = fills.get_mut(part) {
                if fill.0 != paper {
                    fill.0 = paper;
                }
                frame.set_if_neq(BorderColor::all(edge));
            }
        }
        let parts_shown = [
            (bubble.words, !wordless),
            (bubble.dots, wordless),
            (bubble.point, !thought),
            (bubble.trail, thought),
        ];
        for (part, on) in parts_shown {
            if let Ok(mut part) = parts.get_mut(part) {
                show(&mut part, on);
            }
        }
        // It pops in, and shrinks away once its time is up.
        let leaving =
            ((bubble.quiet - linger) / set.leave).clamp(0.0, 1.0);
        let size = pop(bubble.age / set.arrive) * (1.0 - leaving);
        let size = Vec2::splat(size.max(0.0));
        if grown.scale != size {
            grown.scale = size;
        }
        // The robot's place, even past an edge: the panel cuts the
        // balloon there.
        label::put(&mut node, label::above(computed, point, 2.0));
        visibility.set_if_neq(Visibility::Visible);
    }
}

/// The dots of a wordless thought swell one after another.
fn dots(
    time: Res<Time>,
    set: Res<BubbleSettings>,
    mut dots: Query<(&Dot, &mut BackgroundColor)>,
) {
    let t = time.elapsed_secs() * 4.5;
    for (dot, mut fill) in &mut dots {
        let lit = (0.5 + 0.5 * (t - dot.0 * 0.9).sin()).powi(2);
        let want = set.ink_thought.with_alpha(0.25 + 0.75 * lit);
        if fill.0 != want {
            fill.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use hamlet_client::ClientPlugin;
    use hamlet_client::realm::mayor;
    use hamlet_protocol as protocol;
    use hamlet_testing::realm;

    use super::{Bubble, plugin};
    use crate::generator::society::AgentId;
    use crate::generator::testing::{rehearse, stand_in};
    use crate::hamlet::Housed;
    use crate::hamlet::tests::{
        camera, receive_change, receive_state,
    };
    use crate::pick::PickSettings;
    use crate::sound::Cue;

    #[test]
    fn a_received_saying_has_speech_and_short_deed_words() {
        let mut app = App::new();
        rehearse(&mut app);
        app.add_plugins((ClientPlugin { server: None }, plugin))
            .init_resource::<PickSettings>()
            .add_message::<Cue>();
        let mut state = realm::state();
        let founder = &mut state.agents[0];
        founder.busy = true;
        founder.activity.doing = protocol::Doing::Run;
        founder.saying.thought = true;
        founder.saying.text = "Checking the orchard".into();
        let mut founder = founder.clone();
        receive_state(&mut app, state);
        let agent = mayor(app.world_mut()).unwrap();
        app.world_mut().entity_mut(agent).insert(Housed(AgentId(1)));
        let robot = stand_in(app.world_mut(), AgentId(1), 1);
        app.world_mut()
            .entity_mut(robot)
            .insert(InheritedVisibility::VISIBLE);
        let porch =
            app.world().get::<Transform>(robot).unwrap().translation;
        camera(
            &mut app,
            porch + Vec3::new(0.0, 6.0, 12.0),
            porch + Vec3::Y * 2.0,
        );
        app.update();
        app.update();
        let world = app.world_mut();
        assert!(
            world
                .query::<&Text>()
                .iter(world)
                .any(|text| text.0 == "Running a command")
        );
        assert_eq!(
            *world
                .query_filtered::<&Visibility, With<Bubble>>()
                .single(world)
                .unwrap(),
            Visibility::Visible
        );
        founder.saying.thought = false;
        founder.saying.text = "The orchard is ready.".into();
        receive_change(
            &mut app,
            protocol::Update::Agent { agent: founder },
        );
        app.update();
        let world = app.world_mut();
        assert!(
            world
                .query::<&Text>()
                .iter(world)
                .any(|text| text.0 == "The orchard is ready.")
        );
    }
}

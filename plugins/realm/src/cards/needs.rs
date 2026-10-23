//! Paper tags at the houses of agents that wait for the chief: what
//! waits, and a check and a cross that answer it.

use glam::Vec3;
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Choice, Need,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    ButtonProps, ButtonStyle, Cx, FrameProps, Size, Space, Tone,
    UiExt, ViewExt, column, icon_button, label, open, pressable, row,
};
use hamlet_engine_bevy::{Reply, bevy_ecs};

use crate::build::frame::Frame;
use crate::build::upload::Draw;
use crate::cards::WordsExt;
use crate::realm::Things;
use crate::society::AgentId;
use crate::view::pick::Thing;
use crate::world::land::Land;

/// The engine's panel of everything that waits for the chief.
const PETITIONS: &str = "engine/petitions";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Waits>().add_systems(
        Update,
        (follow_waits, hang_tags, perch_tags)
            .chain()
            .run_if(resource_exists::<Land>),
    );
}

/// What waits for the chief, oldest first.
#[derive(Resource, Default)]
struct Waits(Vec<Need>);

/// The tag of the oldest thing that waits at one agent.
#[derive(Component, Clone, PartialEq)]
struct NeedTag {
    agent: AgentId,
    /// The thing that waits.
    need: u64,
    text: String,
    answers: Vec<Choice>,
    /// Number of others that wait at the same agent.
    more: usize,
}

/// What a tag hangs from, over the agent's house.
#[derive(Component)]
struct Perch(Entity);

impl NeedTag {
    /// The answer a check (`agree`) or a cross gives among the
    /// answers the need takes, when one of them is plain enough for
    /// a single click.
    fn answer(&self, agree: bool) -> Option<Choice> {
        let plain: &[Choice] = if agree {
            &[Choice::Allow, Choice::GoAhead]
        } else {
            &[Choice::Deny, Choice::Withdraw]
        };
        self.answers
            .iter()
            .find(|choice| plain.contains(choice))
            .or(self.answers.first().filter(|choice| {
                agree && **choice == Choice::Dismiss
            }))
            .copied()
    }
}

/// Keeps what waits for the chief as the agents service tells it.
fn follow_waits(
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    mut waits: ResMut<Waits>,
) {
    let replied = replies.read().filter_map(|replied| match &replied
        .reply
    {
        Ok(Reply::Agents(AgentsReply::Needs(needs))) => Some(needs),
        _ => None,
    });
    let told = events.read().filter_map(|event| match &event.event {
        AgentsEvent::Needs(needs) => Some(needs),
        _ => None,
    });
    if let Some(needs) = replied.chain(told).last() {
        waits.0.clone_from(needs);
    }
}

/// Keeps one tag for each agent with something waiting.
fn hang_tags(
    waits: Res<Waits>,
    things: Things,
    tags: Query<(Entity, &NeedTag, &Perch)>,
    mut draw: Draw,
) {
    let mut wanted = Vec::<NeedTag>::new();
    for need in &waits.0 {
        let Some(agent) = things.bridge.find(&need.by) else {
            continue;
        };
        match wanted.iter_mut().find(|tag| tag.agent == agent) {
            Some(tag) => tag.more += 1,
            None => wanted.push(NeedTag {
                agent,
                need: need.id,
                text: need.line.short(things.tuning.tags.line),
                answers: need.answers.clone(),
                more: 0,
            }),
        }
    }
    for (card, tag, perch) in &tags {
        if !wanted.contains(tag) {
            draw.despawn(perch.0);
            draw.despawn(card);
        }
    }
    for tag in wanted {
        if tags.iter().any(|(_, held, _)| *held == tag) {
            continue;
        }
        let perch = draw.spawn(None, 0);
        draw.commands.spawn((
            Card::new(CardLook::Tag, things.tuning.tags.width, view),
            CardPlace::Over {
                entity: perch,
                gap: things.tuning.tags.lift,
            },
            Perch(perch),
            tag,
        ));
    }
}

/// Holds each tag over the house of its agent.
fn perch_tags(
    things: Things,
    tags: Query<(&NeedTag, &Perch)>,
    places: Query<&Transform>,
    mut draw: Draw,
) {
    for (tag, perch) in &tags {
        let Some(at) = things.place(Thing::House(tag.agent)) else {
            continue;
        };
        let top = things.land.on_ground(at)
            + Vec3::Y * things.tuning.pick.house.1;
        let held = places
            .get(perch.0)
            .is_ok_and(|place| place.translation == top);
        if !held {
            draw.place(perch.0, Frame::at(top));
        }
    }
}

/// The tag on the card `card`.
fn view(card: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(tag) = cx.world.get::<NeedTag>(card).cloned() else {
            return cx.build(column(()));
        };
        let text = match tag.more {
            0 => tag.text.clone(),
            n => format!("{}\n(and {n} more)", tag.text),
        };
        let mut parts = vec![
            pressable((column((
                label("NEEDS YOU")
                    .size(Size::Small)
                    .tone(Tone::Accent)
                    .bold(true),
                label(text),
            ))
            .space(Space::None),))
            .style(ButtonStyle::Quiet)
            .on_click(|_| open(PETITIONS))
            .boxed(),
        ];
        for (agree, glyph, style) in [
            (true, "check", ButtonStyle::Primary),
            (false, "cross", ButtonStyle::Critical),
        ] {
            let Some(choice) = tag.answer(agree) else {
                continue;
            };
            let need = tag.need;
            parts.push(
                icon_button(glyph, "")
                    .style(style)
                    .on_click(move |world| {
                        world.agents(&AgentsRequest::Answer {
                            need,
                            choice,
                            text: String::new(),
                        });
                    })
                    .boxed(),
            );
        }
        cx.build(row(parts).space(Space::Half))
    })
}

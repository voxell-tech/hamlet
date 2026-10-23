//! The menu of each kind of thing in the world, and of the bare
//! ground.

use hamlet_engine_bevy::api::agents::{
    AgentsRequest, Gift, RosterAgent, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{MenuItem, menu_item};

use crate::ask::AskWorldExt;
use crate::ask::deed::Deed;
use crate::bridge::Bridge;
use crate::realm::HOME;
use crate::society::{AgentId, Place};
use crate::view::pick::{Picking, Thing};

/// The group the skills an agent could be given are under.
const GIVE: &str = "Give a skill...";

/// What the menus are made from.
pub struct Menus<'a> {
    bridge: &'a Bridge,
    picking: &'a Picking,
    /// The skills the agent asked about could be given.
    gifts: &'a [Gift],
}

/// An item that runs `act`.
fn item(
    text: impl Into<String>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> MenuItem {
    menu_item(text, act)
}

/// An item that asks before it does `deed`.
fn asking(
    text: impl Into<String>,
    deed: Deed,
    working: Vec<String>,
) -> MenuItem {
    item(text, move |world| {
        world.question(deed.clone(), working.clone());
    })
}

/// The agent a thing stands for: its robot or its house.
fn agent_of(thing: Thing) -> Option<AgentId> {
    match thing {
        Thing::Robot(agent) | Thing::House(agent) => Some(agent),
        _ => None,
    }
}

impl<'a> Menus<'a> {
    pub fn new(
        bridge: &'a Bridge,
        picking: &'a Picking,
        gifts: &'a [Gift],
    ) -> Self {
        Self {
            bridge,
            picking,
            gifts,
        }
    }

    /// The menu of the bare ground.
    pub fn ground() -> Vec<MenuItem> {
        vec![item("Found a Hamlet...", |world| world.found_hamlet())]
    }

    /// The menu of `thing`.
    pub fn of(&self, thing: Thing) -> Vec<MenuItem> {
        match thing {
            Thing::Robot(agent) | Thing::House(agent) => {
                let Some(me) = self.bridge.agent(agent) else {
                    return Vec::new();
                };
                if me.gone {
                    restore(me)
                } else if me.mayor {
                    self.for_mayor(me)
                } else {
                    self.for_resident(thing, agent, me)
                }
            }
            Thing::Common(town)
            | Thing::Workbench(town)
            | Thing::Clockwork(town) => {
                let common = Thing::Common(town);
                let mut items = vec![focus(common)];
                let name = self
                    .bridge
                    .title(town)
                    .unwrap_or_default()
                    .to_owned();
                items.push(item(
                    format!("Move {name}..."),
                    move |world| world.carry(Place::Town(town)),
                ));
                // The first hamlet is the home itself and stays.
                if let Some(id) =
                    self.bridge.hamlet(town).filter(|id| *id != HOME)
                {
                    let working = self
                        .bridge
                        .living(town)
                        .filter(|agent| agent.busy)
                        .map(|agent| agent.name.clone())
                        .collect();
                    items.push(
                        asking(
                            format!("Close Down {name}..."),
                            Deed::Close(id.to_owned()),
                            working,
                        )
                        .critical(),
                    );
                }
                items
            }
            Thing::Mascot | Thing::Hearth => vec![
                item("Change Your Look", |world| {
                    world.show(Selected::Hearth)
                }),
                focus(thing),
                item("Go to the Hearth", |world| {
                    world.focus(Thing::Hearth, false)
                }),
                item("Move the Hearth...", |world| {
                    world.carry(Place::Chief)
                }),
            ],
            Thing::Library => vec![
                focus(thing),
                item("Move the Library...", |world| {
                    world.carry(Place::Library)
                }),
            ],
        }
    }

    /// The menu of the mayor `me` of a hamlet.
    fn for_mayor(&self, me: &RosterAgent) -> Vec<MenuItem> {
        let (name, hamlet) = (me.name.clone(), me.hamlet.clone());
        vec![
            item("Talk to the Mayor", {
                let name = name.clone();
                move |world| world.talk_to(&name)
            }),
            item("Prompt", {
                let name = name.clone();
                move |world| world.show(Selected::Agent(name.clone()))
            }),
            stop("Stop the Mayor", &name),
            undo(&hamlet),
        ]
    }

    /// The menu of the agent `me`, who is not its hamlet's mayor.
    fn for_resident(
        &self,
        thing: Thing,
        agent: AgentId,
        me: &RosterAgent,
    ) -> Vec<MenuItem> {
        let name = me.name.clone();
        let mut items = vec![
            item(format!("Talk to {name}"), {
                let name = name.clone();
                move |world| world.talk_to(&name)
            }),
            item("Prompt", {
                let name = name.clone();
                move |world| world.show(Selected::Agent(name.clone()))
            }),
            focus(thing),
            item("Follow", move |world| world.focus(thing, true)),
        ];
        if me.busy {
            items.push(stop(format!("Stop {name}"), &name));
        }
        // The other of the two that are chosen, if there are two.
        let other = [self.picking.selected(), self.picking.also()]
            .into_iter()
            .flatten()
            .filter_map(agent_of)
            .filter(|other| *other != agent)
            .filter_map(|other| {
                Some((other, self.bridge.agent(other)?))
            })
            .find(|(_, other)| !other.mayor && !other.gone);
        let peers = self.bridge.peers(agent);
        if let Some((them, other)) = &other {
            let joined = peers.iter().any(|(peer, _)| peer == them);
            items.push(road(me, &other.name, !joined));
        }
        if !self.gifts.is_empty() {
            for gift in self.gifts {
                let text = match &gift.from {
                    Some(from) => {
                        format!("{} (of {from})", gift.skill)
                    }
                    None => format!("{} (starter)", gift.skill),
                };
                let (gift, name) = (gift.clone(), name.clone());
                items.push(
                    item(text, move |world| {
                        world.agents(&AgentsRequest::Give {
                            agent: name.clone(),
                            skill: gift.skill.clone(),
                            from: gift.from.clone(),
                        });
                    })
                    .under(GIVE),
                );
            }
        }
        if let Some((_, other)) = &other {
            for (absorbed, kept) in [(*other, me), (me, *other)] {
                items.push(
                    asking(
                        format!(
                            "Merge {} into {}...",
                            absorbed.name, kept.name
                        ),
                        Deed::Merge {
                            absorbed: absorbed.name.clone(),
                            kept: kept.name.clone(),
                            hamlet: kept.hamlet.clone(),
                        },
                        [absorbed, kept]
                            .into_iter()
                            .filter(|agent| agent.busy)
                            .map(|agent| agent.name.clone())
                            .collect(),
                    )
                    .critical(),
                );
            }
        }
        // The roads it has, besides the one to the other of two that
        // are chosen, which is offered above.
        for (peer, them) in peers {
            if them.mayor
                || other.as_ref().is_some_and(|o| o.0 == peer)
            {
                continue;
            }
            items.push(road(me, &them.name, false));
        }
        items.push(undo(&me.hamlet));
        let working = |busy: bool| {
            if busy {
                vec![me.name.clone()]
            } else {
                Vec::new()
            }
        };
        items.push(asking(
            format!("Make {name} Mayor..."),
            Deed::Mayor {
                name: name.clone(),
                hamlet: me.hamlet.clone(),
            },
            Vec::new(),
        ));
        items.push(
            asking(
                format!("Remove {name}..."),
                Deed::Remove {
                    name,
                    hamlet: me.hamlet.clone(),
                },
                working(me.busy),
            )
            .critical(),
        );
        items
    }
}

/// An item that builds a road from `me` to the agent `to`, or takes
/// the road up.
fn road(me: &RosterAgent, to: &str, build: bool) -> MenuItem {
    let (hamlet, from, to) =
        (me.hamlet.clone(), me.name.clone(), to.to_owned());
    if build {
        item(format!("Build a road to {to}"), move |world| {
            world.decree(&hamlet, format!("/road {from} {to}"))
        })
    } else {
        item(format!("Remove the road to {to}"), move |world| {
            world.decree(&hamlet, format!("/unroad {from} {to}"))
        })
    }
}

/// An item that brings `thing` to the middle of the view.
fn focus(thing: Thing) -> MenuItem {
    item("Focus", move |world| world.focus(thing, false))
}

/// An item that stops the agent `name`.
fn stop(text: impl Into<String>, name: &str) -> MenuItem {
    let name = name.to_owned();
    item(text, move |world| {
        world.agents(&AgentsRequest::Veto(Some(name.clone())));
    })
    .critical()
}

/// An item that takes back the last change of the hamlet `hamlet`.
fn undo(hamlet: &str) -> MenuItem {
    let hamlet = hamlet.to_owned();
    item("Undo the Last Change", move |world| {
        world.decree(&hamlet, "/undo".to_owned())
    })
}

/// The menu of the house of `gone`, which is a ruin.
fn restore(gone: &RosterAgent) -> Vec<MenuItem> {
    let (name, hamlet) = (gone.name.clone(), gone.hamlet.clone());
    vec![item(format!("Restore {name}"), move |world| {
        world.decree(&hamlet, format!("/restore {name}"))
    })]
}

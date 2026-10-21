//! The menu of an agent: everything else that can be done to it,
//! each a chief's command given to its hamlet, with a question first
//! for what cannot be taken back easily. "More" on an agent's page
//! opens it, and so does a card of a hamlet's agents.

use agents_ui::{
    AgentsWorldExt, Confirm, Focused, either, moved, show_chat,
};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, Gift, Papers, Roster, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, MenuItem, UiExt, ViewExt,
    World, column, derived, icon, keyed, menu_item, pressable,
};

use crate::agent::open_prompt;
use crate::select;

/// What the menu of an agent offers.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct About {
    name: String,
    /// Its hamlet, by its name on disk.
    hamlet: String,
    mayor: bool,
    busy: bool,
    /// The other living agents of its hamlet but the mayor, by name,
    /// whether each works, and whether a road joins it to them.
    others: Vec<(String, bool, bool)>,
    /// The agents a road joins it to, the mayor aside, by name.
    joined: Vec<String>,
    /// The skills it could be given, known for the agent of the open
    /// conversation.
    gifts: Vec<Gift>,
}

impl About {
    /// What the menu of the living agent `name` offers.
    pub(crate) fn of(world: &World, name: &str) -> Option<Self> {
        let roster = world.resource::<Roster>();
        let agent = roster
            .agents
            .iter()
            .find(|agent| agent.name == name && !agent.gone)?;
        let end = |hamlet: &str, number: u64| {
            roster.agents.iter().find(|one| {
                one.hamlet == hamlet
                    && one.number == number
                    && !one.gone
            })
        };
        let mut joined = roster
            .roads
            .iter()
            .filter_map(|road| {
                let [a, b] = &road.ends;
                let (a, b) = (end(&a.0, a.1)?, end(&b.0, b.1)?);
                match (a.name == name, b.name == name) {
                    (true, false) => Some(b),
                    (false, true) => Some(a),
                    _ => None,
                }
            })
            .filter(|peer| !peer.mayor)
            .map(|peer| peer.name.clone())
            .collect::<Vec<_>>();
        joined.sort();
        joined.dedup();
        let others = roster
            .agents
            .iter()
            .filter(|other| {
                other.hamlet == agent.hamlet
                    && other.name != name
                    && !other.gone
                    && !other.mayor
            })
            .map(|other| {
                let road = joined.contains(&other.name);
                (other.name.clone(), other.busy, road)
            })
            .collect();
        let open = world.resource::<Focused>().agent();
        let gifts = if open.as_deref() == Some(name) {
            world.resource::<Papers>().gifts.clone()
        } else {
            Vec::new()
        };
        Some(Self {
            name: name.to_string(),
            hamlet: agent.hamlet.clone(),
            mayor: agent.mayor,
            busy: agent.busy,
            others,
            joined,
            gifts,
        })
    }

    /// The items of its menu.
    pub(crate) fn items(&self) -> Vec<MenuItem> {
        let (name, hamlet) = (self.name.clone(), self.hamlet.clone());
        let mut items = Vec::new();
        let talked = name.clone();
        items.push(menu_item(
            if self.mayor {
                "Talk to the Mayor".to_string()
            } else {
                format!("Talk to {name}")
            },
            move |world| {
                select(world, Selected::Agent(talked.clone()));
                show_chat();
            },
        ));
        let prompted = name.clone();
        items.push(menu_item("Prompt", move |world| {
            open_prompt(world, &prompted)
        }));
        // The mayor's stop is always offered, as it may be about to
        // work.
        if self.busy || self.mayor {
            let stopped = name.clone();
            let words = if self.mayor {
                "Stop the Mayor".to_string()
            } else {
                format!("Stop {name}")
            };
            items.push(
                menu_item(words, move |world| {
                    world.ask(AgentsRequest::Veto(Some(
                        stopped.clone(),
                    )))
                })
                .critical(),
            );
        }
        if !self.mayor {
            self.resident(&mut items);
        }
        let undone = hamlet.clone();
        items.push(menu_item("Undo the Last Change", move |world| {
            world.decree(&undone, "/undo")
        }));
        if self.mayor {
            return items;
        }
        items.push(asking(
            format!("Make {name} Mayor..."),
            &hamlet,
            Confirm {
                question: format!(
                    "MAKE {} MAYOR?",
                    name.to_uppercase()
                ),
                outcome: format!(
                    "{name} runs the hamlet from now on, and \
                     today's mayor becomes one of its agents."
                ),
                working: Vec::new(),
                verb: "MAKE MAYOR".into(),
                then: Vec::new(),
            },
            format!("/mayor {name}"),
        ));
        items.push(
            asking(
                format!("Remove {name}..."),
                &hamlet,
                Confirm {
                    question: format!(
                        "REMOVE {}?",
                        name.to_uppercase()
                    ),
                    outcome: format!(
                        "The house of {name} goes to the archive, \
                         with its notes, skills and log. It can be \
                         restored."
                    ),
                    working: working(&[(&name, self.busy)]),
                    verb: "REMOVE".into(),
                    then: Vec::new(),
                },
                format!("/remove {name}"),
            )
            .critical(),
        );
        items
    }

    /// The items of an agent that is not a mayor: roads, skills and
    /// merges.
    fn resident(&self, items: &mut Vec<MenuItem>) {
        let (name, hamlet) = (&self.name, &self.hamlet);
        for (other, _, road) in &self.others {
            if *road {
                continue;
            }
            let text = format!("/road {name} {other}");
            let at = hamlet.clone();
            items.push(
                menu_item(other.clone(), move |world| {
                    world.decree(&at, text.clone())
                })
                .under("Build a road to"),
            );
        }
        for gift in &self.gifts {
            let (words, text) = match &gift.from {
                Some(from) => (
                    format!("{} (of {from})", gift.skill),
                    format!(
                        "/give {name} {} from {from}",
                        gift.skill
                    ),
                ),
                None => (
                    format!("{} (starter)", gift.skill),
                    format!("/give {name} {}", gift.skill),
                ),
            };
            let at = hamlet.clone();
            items.push(
                menu_item(words, move |world| {
                    world.decree(&at, text.clone())
                })
                .under("Give a skill..."),
            );
        }
        for (other, busy, _) in &self.others {
            let merge = |absorbed: (&str, bool),
                         kept: (&str, bool)| {
                asking(
                    format!(
                        "Merge {} into {}...",
                        absorbed.0, kept.0
                    ),
                    hamlet,
                    Confirm {
                        question: format!(
                            "MERGE {} INTO {}?",
                            absorbed.0.to_uppercase(),
                            kept.0.to_uppercase()
                        ),
                        outcome: format!(
                            "{} takes the notes, skills, files and \
                             roads of {}, whose house is archived. \
                             It can be split out again.",
                            kept.0, absorbed.0
                        ),
                        working: working(&[absorbed, kept]),
                        verb: "MERGE".into(),
                        then: Vec::new(),
                    },
                    format!("/merge {} into {}", absorbed.0, kept.0),
                )
            };
            items.push(merge((other, *busy), (name, self.busy)));
            items.push(merge((name, self.busy), (other, *busy)));
        }
        for peer in &self.joined {
            let text = format!("/unroad {name} {peer}");
            let at = hamlet.clone();
            items.push(menu_item(
                format!("Remove the road to {peer}"),
                move |world| world.decree(&at, text.clone()),
            ));
        }
    }
}

/// An item that asks `question` before it gives `text` to `hamlet`.
fn asking(
    words: String,
    hamlet: &str,
    question: Confirm,
    text: String,
) -> MenuItem {
    let hamlet = hamlet.to_string();
    menu_item(words, move |world| {
        let mut asked = question.clone();
        asked.then = vec![AgentsRequest::Decree {
            hamlet: hamlet.clone(),
            text: text.clone(),
        }];
        world.confirm(asked);
    })
}

/// The names of the agents of `pairs` that work.
fn working(pairs: &[(&str, bool)]) -> Vec<String> {
    pairs
        .iter()
        .filter(|(_, busy)| *busy)
        .map(|(name, _)| name.to_string())
        .collect()
}

/// The menu of the agent of the open conversation.
fn about(world: &World) -> Option<About> {
    About::of(world, &world.resource::<Focused>().agent()?)
}

/// The button that opens the menu of the agent of the open
/// conversation.
pub(crate) fn more() -> AnyView {
    keyed(
        derived(about).when(either(
            moved::<Focused>(),
            either(moved::<Roster>(), moved::<Papers>()),
        )),
        |about| match about {
            Some(about) => pressable((icon("more"),))
                .style(ButtonStyle::Plain)
                .tip("More to do with it")
                .menu_on_click(about.items())
                .boxed(),
            None => column(()).boxed(),
        },
    )
    .boxed()
}

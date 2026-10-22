//! "More" on an agent's page: everything else that can be done to
//! the agent of the open conversation, each a chief's command given
//! to its hamlet, with a question first for what cannot be taken
//! back easily.

use agents_ui::{
    Confirm, Focused, ask, confirm, either, moved, show_chat, titled,
};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, Gift, Papers, Recipient, Roster, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, MenuItem, UiExt, ViewExt,
    World, column, derived, icon, keyed, menu_item, pressable,
};

use crate::agent::{Tab, show};
use crate::select;

/// What the menu of the agent of the open conversation offers.
#[derive(Clone, PartialEq, Debug, Default)]
struct About {
    name: String,
    /// Its hamlet, by its name on disk.
    hamlet: String,
    mayor: bool,
    busy: bool,
    /// The other living agents of its hamlet but the mayor, by name,
    /// and whether each works.
    others: Vec<(String, bool)>,
    gifts: Vec<Gift>,
}

fn about(world: &World) -> Option<About> {
    let focus = world.resource::<Focused>().0.as_ref()?;
    let name = focus.open.agent.clone()?;
    let roster = world.resource::<Roster>();
    let agent = roster
        .agents
        .iter()
        .find(|agent| agent.name == name && !agent.gone)?;
    let others = roster
        .agents
        .iter()
        .filter(|other| {
            other.hamlet == agent.hamlet
                && other.name != name
                && !other.gone
                && !other.mayor
        })
        .map(|other| (other.name.clone(), other.busy))
        .collect();
    Some(About {
        hamlet: agent.hamlet.clone(),
        mayor: matches!(focus.with, Recipient::Mayor(_)),
        busy: agent.busy,
        others,
        gifts: world.resource::<Papers>().gifts.clone(),
        name,
    })
}

/// Gives the chief's command `text` to the hamlet `hamlet`.
fn decree(world: &mut World, hamlet: &str, text: String) {
    ask(
        world,
        AgentsRequest::Decree {
            hamlet: hamlet.to_string(),
            text,
        },
    );
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
        confirm(world, asked);
    })
}

/// The names of the agents of `pairs` that work.
fn working(pairs: &[(&str, bool)]) -> Vec<String> {
    pairs
        .iter()
        .filter(|(_, busy)| *busy)
        .map(|(name, _)| titled(name))
        .collect()
}

fn items(about: &About) -> Vec<MenuItem> {
    let (name, hamlet) = (about.name.clone(), about.hamlet.clone());
    let me = titled(&name);
    let mut items = Vec::new();
    let talked = name.clone();
    items.push(menu_item(
        if about.mayor {
            "Talk to the Mayor".to_string()
        } else {
            format!("Talk to {me}")
        },
        move |world| {
            select(world, Selected::Agent(talked.clone()));
            show_chat();
        },
    ));
    items.push(menu_item("Prompt", |world| show(world, Tab::Prompt)));
    if about.busy {
        let stopped = name.clone();
        let words = if about.mayor {
            "Stop the Mayor".to_string()
        } else {
            format!("Stop {me}")
        };
        items.push(
            menu_item(words, move |world| {
                ask(world, AgentsRequest::Veto(Some(stopped.clone())))
            })
            .critical(),
        );
    }
    if !about.mayor {
        for (other, _) in &about.others {
            let text = format!("/road {name} {other}");
            let at = hamlet.clone();
            items.push(
                menu_item(titled(other), move |world| {
                    decree(world, &at, text.clone())
                })
                .under("Build a road to"),
            );
        }
        for gift in &about.gifts {
            let (words, text) = match &gift.from {
                Some(from) => (
                    format!("{} (of {})", gift.skill, titled(from)),
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
                    decree(world, &at, text.clone())
                })
                .under("Give a skill"),
            );
        }
        for (other, busy) in &about.others {
            let them = titled(other);
            let both = [
                (name.as_str(), about.busy),
                (other.as_str(), *busy),
            ];
            let merge = |absorbed: &str, kept: &str| Confirm {
                question: format!(
                    "MERGE {} INTO {}?",
                    absorbed.to_uppercase(),
                    kept.to_uppercase()
                ),
                outcome: format!(
                    "{} takes the notes, skills, files and roads of \
                     {}, whose house is archived. It can be split \
                     out again.",
                    titled(kept),
                    titled(absorbed)
                ),
                working: working(&both),
                verb: "MERGE".into(),
                then: Vec::new(),
            };
            items.push(
                asking(
                    format!("{them} into {me}..."),
                    &hamlet,
                    merge(other, &name),
                    format!("/merge {other} into {name}"),
                )
                .under("Merge"),
            );
            items.push(
                asking(
                    format!("{me} into {them}..."),
                    &hamlet,
                    merge(&name, other),
                    format!("/merge {name} into {other}"),
                )
                .under("Merge"),
            );
        }
    }
    let undone = hamlet.clone();
    items.push(menu_item("Undo the Last Change", move |world| {
        decree(world, &undone, "/undo".into())
    }));
    if about.mayor {
        return items;
    }
    let alone = [(name.as_str(), about.busy)];
    items.push(asking(
        format!("Make {me} Mayor..."),
        &hamlet,
        Confirm {
            question: format!("MAKE {} MAYOR?", name.to_uppercase()),
            outcome: format!(
                "{me} runs the hamlet from now on, and today's \
                 mayor becomes one of its agents."
            ),
            working: Vec::new(),
            verb: "MAKE MAYOR".into(),
            then: Vec::new(),
        },
        format!("/mayor {name}"),
    ));
    items.push(
        asking(
            format!("Remove {me}..."),
            &hamlet,
            Confirm {
                question: format!("REMOVE {}?", name.to_uppercase()),
                outcome: format!(
                    "The house of {me} goes to the archive, with \
                     its notes, skills and log. It can be restored."
                ),
                working: working(&alone),
                verb: "REMOVE".into(),
                then: Vec::new(),
            },
            format!("/remove {name}"),
        )
        .critical(),
    );
    items
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
                .menu_on_click(items(about))
                .boxed(),
            None => column(()).boxed(),
        },
    )
    .boxed()
}

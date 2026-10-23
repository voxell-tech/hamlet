//! The deeds that end an agent or a hamlet or change who runs one,
//! and the question asked before each.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Cx, Face, FrameProps, Space, Tone, UiExt,
    ViewExt, button, column, description, label, row,
};

use crate::ask::AskWorldExt;
use crate::realm::HOME;

/// Something that ends an agent or a hamlet, or changes who runs
/// one.
#[derive(Clone, PartialEq, Debug)]
pub enum Deed {
    Remove {
        name: String,
        hamlet: String,
    },
    /// Merges `absorbed` into `kept`, of the hamlet `hamlet`.
    Merge {
        absorbed: String,
        kept: String,
        hamlet: String,
    },
    /// Closes down the hamlet of this name.
    Close(String),
    /// Makes the agent the mayor of its hamlet.
    Mayor {
        name: String,
        hamlet: String,
    },
}

/// A question before a deed, and the agents at work that the deed
/// cuts short.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Asking {
    pub deed: Deed,
    pub working: Vec<String>,
}

impl Deed {
    /// Gives the chief's command that does it to the hamlet it is
    /// about.
    fn run(&self, world: &mut World) {
        match self {
            Self::Remove { name, hamlet } => {
                world.decree(hamlet, format!("/remove {name}"));
            }
            Self::Merge {
                absorbed,
                kept,
                hamlet,
            } => {
                world.decree(
                    hamlet,
                    format!("/merge {absorbed} into {kept}"),
                );
            }
            Self::Mayor { name, hamlet } => {
                world.decree(hamlet, format!("/mayor {name}"));
            }
            Self::Close(hamlet) => {
                world
                    .decree(HOME, format!("/remove-hamlet {hamlet}"));
            }
        }
    }

    fn question(&self) -> String {
        match self {
            Self::Remove { name, .. } => {
                format!("REMOVE {}?", name.to_uppercase())
            }
            Self::Merge { absorbed, kept, .. } => format!(
                "MERGE {} INTO {}?",
                absorbed.to_uppercase(),
                kept.to_uppercase()
            ),
            Self::Close(hamlet) => {
                format!("CLOSE DOWN {}?", hamlet.to_uppercase())
            }
            Self::Mayor { name, .. } => {
                format!("MAKE {} MAYOR?", name.to_uppercase())
            }
        }
    }

    fn outcome(&self) -> String {
        match self {
            Self::Remove { name, .. } => format!(
                "The house of {name} goes to the archive, with its \
                 notes, skills and log. It can be restored."
            ),
            Self::Merge { absorbed, kept, .. } => format!(
                "{kept} takes the notes, skills, files and roads of \
                 {absorbed}, whose house is archived. It can be \
                 split out again."
            ),
            Self::Close(hamlet) => format!(
                "Everyone in {hamlet} stops and every house is \
                 archived. You can restore it from the list of \
                 hamlets."
            ),
            Self::Mayor { name, .. } => format!(
                "{name} runs the hamlet from now on, and today's \
                 mayor becomes one of its agents."
            ),
        }
    }

    fn verb(&self) -> &'static str {
        match self {
            Self::Remove { .. } => "REMOVE",
            Self::Merge { .. } => "MERGE",
            Self::Close(_) => "CLOSE DOWN",
            Self::Mayor { .. } => "MAKE MAYOR",
        }
    }
}

/// The question the card `card` asks.
pub fn view(card: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(asking) = cx.world.get::<Asking>(card).cloned()
        else {
            return cx.build(column(()));
        };
        let Asking { deed, working } = asking;
        let mut parts = vec![
            label(deed.question())
                .bold(true)
                .face(Face::Display)
                .boxed(),
            description(deed.outcome()).boxed(),
        ];
        if !working.is_empty() {
            let are = if working.len() == 1 { "is" } else { "are" };
            parts.push(
                label(format!(
                    "{} {are} working right now. What it is doing \
                     will be cut short.",
                    working.join(" and ")
                ))
                .tone(Tone::Critical)
                .boxed(),
            );
        }
        let verb = deed.verb();
        parts.push(
            row((
                button(verb).style(ButtonStyle::Critical).on_click(
                    move |world| {
                        world.despawn(card);
                        deed.run(world);
                    },
                ),
                button("CANCEL").style(ButtonStyle::Quiet).on_click(
                    move |world| {
                        world.despawn(card);
                    },
                ),
            ))
            .space(Space::Half)
            .boxed(),
        );
        cx.build(column(parts).space(Space::Half))
    })
}

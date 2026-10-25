//! The page of the chief: the look their mascot and Hearth wear, each
//! choice of it a row of buttons.

use hamlet_engine_bevy::api::agents::{
    AgentsRequest, Cloak, Hat, Holds, Light, Look, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, FrameProps, Space, Tone, UiExt, ViewExt,
    button, column, description, keyed, label, resource, row, scroll,
};

use crate::inspector::{HAMLETS, select};
use crate::shared::{Chiefed, breadcrumb};

/// One choice of the look: what it is, each way it can go with the
/// word for it, and how it is read from and written to a look.
struct Kind<T: 'static> {
    what: &'static str,
    all: &'static [(T, &'static str)],
    read: fn(&Look) -> T,
    write: fn(&mut Look, T),
}

/// One line of the page: what is chosen, and a button for each way.
fn choose<T>(kind: Kind<T>) -> AnyView
where
    T: Copy + PartialEq + Send + Sync + 'static,
{
    let Kind {
        what,
        all,
        read,
        write,
    } = kind;
    let choices = keyed(
        resource::<Chiefed, _>(move |chiefed| read(&chiefed.0.look)),
        move |now| {
            let buttons = all
                .iter()
                .map(|&(choice, word)| {
                    button(word.to_uppercase())
                        .style(if choice == *now {
                            ButtonStyle::Primary
                        } else {
                            ButtonStyle::Plain
                        })
                        .on_click(move |world| {
                            let mut look =
                                world.resource::<Chiefed>().0.look;
                            write(&mut look, choice);
                            world.agents(&AgentsRequest::SetLook(
                                look,
                            ));
                        })
                        .boxed()
                })
                .collect::<Vec<_>>();
            row(buttons).space(Space::Half).boxed()
        },
    );
    column((label(what).tone(Tone::Dim), choices))
        .space(Space::Half)
        .boxed()
}

/// The page of the chief. A press changes the mascot and the Hearth
/// at once.
pub(crate) fn chief() -> AnyView {
    column((
        breadcrumb(
            vec![HAMLETS.into()],
            "YOUR HEARTH",
            |world, _| select(world, Selected::Nothing),
        ),
        description(
            "This is you. Your cloak also colours the roof of the \
             Hearth.",
        ),
        scroll((
            choose(Kind {
                what: "Cloak",
                all: &[
                    (Cloak::River, "river"),
                    (Cloak::Ember, "ember"),
                    (Cloak::Moss, "moss"),
                    (Cloak::Plum, "plum"),
                    (Cloak::Mustard, "mustard"),
                    (Cloak::Rose, "rose"),
                    (Cloak::Slate, "slate"),
                    (Cloak::Cream, "cream"),
                ],
                read: |look| look.cloak,
                write: |look, cloak| look.cloak = cloak,
            }),
            choose(Kind {
                what: "Light of the wick",
                all: &[
                    (Light::Candle, "candle"),
                    (Light::Rune, "rune"),
                    (Light::Rosy, "rosy"),
                    (Light::Leaf, "leaf"),
                    (Light::Moon, "moon"),
                ],
                read: |look| look.light,
                write: |look, light| look.light = light,
            }),
            choose(Kind {
                what: "On the head",
                all: &[
                    (Hat::Pointed, "pointed"),
                    (Hat::Hood, "hood"),
                    (Hat::Crown, "crown"),
                    (Hat::Wreath, "wreath"),
                    (Hat::Bare, "bare"),
                ],
                read: |look| look.hat,
                write: |look, hat| look.hat = hat,
            }),
            choose(Kind {
                what: "In the hand",
                all: &[
                    (Holds::Quill, "quill"),
                    (Holds::Teacup, "teacup"),
                    (Holds::Book, "book"),
                    (Holds::Nothing, "nothing"),
                ],
                read: |look| look.holds,
                write: |look, holds| look.holds = holds,
            }),
        )),
    ))
    .space(Space::Gap)
    .boxed()
}

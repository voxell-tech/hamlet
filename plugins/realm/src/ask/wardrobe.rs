//! The card the chief's look is chosen on.

use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Space, Tone, UiExt,
    ViewExt, button, column, description, keyed, label, resource,
    row,
};

use crate::folk::chief::Wearing;
use crate::folk::chief::look::{Cloak, Hat, Holds, Light, Look};

/// On the card of the wardrobe.
#[derive(Component)]
pub struct Dressing;

/// A choice of the look, the words that name it and how it is read
/// from and written to a look.
struct Kind<T: 'static> {
    what: &'static str,
    all: &'static [T],
    name: fn(T) -> &'static str,
    read: fn(&Look) -> T,
    write: fn(&mut Look, T),
}

/// One line of the card: what is chosen, and a button for each way.
fn choose<T>(kind: Kind<T>) -> AnyView
where
    T: Copy + PartialEq + Send + Sync + 'static,
{
    let Kind {
        what,
        all,
        name,
        read,
        write,
    } = kind;
    let choices = keyed(
        resource::<Wearing, _>(move |wearing| read(&wearing.0)),
        move |now| {
            let buttons = all
                .iter()
                .map(|&choice| {
                    button(name(choice).to_uppercase())
                        .style(if choice == *now {
                            ButtonStyle::Primary
                        } else {
                            ButtonStyle::Plain
                        })
                        .on_click(move |world| {
                            let mut look =
                                world.resource::<Wearing>().0;
                            write(&mut look, choice);
                            world.agents(&AgentsRequest::SetLook(
                                look.into(),
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

/// The card `card`: the four choices of a look. A press changes the
/// mascot and the Hearth at once.
pub fn view(card: Entity) -> AnyView {
    column((
        label("CHANGE YOUR LOOK").bold(true).face(Face::Display),
        description(
            "This is you. Your cloak also colours the roof of the \
             Hearth.",
        ),
        choose(Kind {
            what: "Cloak",
            all: Cloak::ALL,
            name: Cloak::name,
            read: |look| look.cloak,
            write: |look, cloak| look.cloak = cloak,
        }),
        choose(Kind {
            what: "Light of the wick",
            all: Light::ALL,
            name: Light::name,
            read: |look| look.light,
            write: |look, light| look.light = light,
        }),
        choose(Kind {
            what: "On the head",
            all: Hat::ALL,
            name: Hat::name,
            read: |look| look.hat,
            write: |look, hat| look.hat = hat,
        }),
        choose(Kind {
            what: "In the hand",
            all: Holds::ALL,
            name: Holds::name,
            read: |look| look.holds,
            write: |look, holds| look.holds = holds,
        }),
        button("DONE").style(ButtonStyle::Primary).on_click(
            move |world| {
                world.despawn(card);
            },
        ),
    ))
    .space(Space::Half)
    .boxed()
}

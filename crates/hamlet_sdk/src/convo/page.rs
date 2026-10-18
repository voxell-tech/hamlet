//! A page about the agent the chat is with.

use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, label};
use bevy_fynix::{ViewExt, keyed, resource};

use super::Me;
use crate::looks::{Fills, filling};
use crate::platform::DisplayText;
use crate::{AnyView, View};

/// A page of `what` about the agent the chat is with, under whose
/// it is.
pub fn of_agent(
    what: &'static str,
    body: fn() -> AnyView,
) -> impl View {
    keyed(
        resource::<Me, _>(|m| (m.session, m.with.clone())),
        move |(_, with)| {
            let whose = match with {
                Some(name) => {
                    format!("{}: {what}", name.to_uppercase())
                }
                None => format!("THE MAYOR: {what}"),
            };
            column((
                label(whose)
                    .size(13.0)
                    .bold(true)
                    .tone(Tone::Faint)
                    .tagged(DisplayText),
                body(),
            ))
            .fills_height()
            .gap(12.0)
            .boxed()
        },
    )
    .within(filling())
}

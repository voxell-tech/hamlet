//! A question asked before something that cannot be taken back
//! easily: what it does, a warning for agents it cuts short, and its
//! verb beside CANCEL, on a card over the dock. A plugin that asks
//! declares the overlay [`CONFIRM`] and mounts [`confirmation`] in
//! it.

use hamlet_engine_sdk::AgentsRequest;
use hamlet_engine_sdk::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Space, Tone, UiExt,
    ViewExt, World, button, column, keyed, label, resource, row,
    show_overlay,
};

use crate::client::ask;

/// The name of the overlay a question is asked on.
pub const CONFIRM: &str = "confirm";

/// Takes the question away.
fn close(world: &mut World) {
    world.resource_mut::<Confirming>().0 = None;
    show_overlay(CONFIRM, false);
}

/// A question before a deed, and the requests that do it.
#[derive(Clone, PartialEq, Debug)]
pub struct Confirm {
    pub question: String,
    pub outcome: String,
    /// Agents at work that it cuts short, by name.
    pub working: Vec<String>,
    pub verb: String,
    pub then: Vec<AgentsRequest>,
}

/// The question shown, if any.
#[derive(Default)]
pub struct Confirming(pub Option<Confirm>);

/// Asks `question` before it is done.
pub fn confirm(world: &mut World, question: Confirm) {
    world.resource_or_init::<Confirming>().0 = Some(question);
    show_overlay(CONFIRM, true);
}

/// The question asked, where a panel shows it.
pub fn confirmation() -> AnyView {
    keyed(
        resource::<Confirming, _>(|asked| asked.0.clone()),
        |asked| {
            let Some(asked) = asked.clone() else {
                return column(()).boxed();
            };
            let mut parts = vec![
                label(asked.question.clone())
                    .bold(true)
                    .face(Face::Display)
                    .boxed(),
                label(asked.outcome.clone()).tone(Tone::Dim).boxed(),
            ];
            if !asked.working.is_empty() {
                let names = asked.working.join(" and ");
                let are = if asked.working.len() == 1 {
                    "is"
                } else {
                    "are"
                };
                parts.push(
                    label(format!(
                        "{names} {are} working right now. What it \
                         is doing will be cut short."
                    ))
                    .tone(Tone::Critical)
                    .boxed(),
                );
            }
            let then = asked.then.clone();
            parts.push(
                row((
                    button(asked.verb.clone())
                        .style(ButtonStyle::Critical)
                        .on_click(move |world| {
                            close(world);
                            for request in &then {
                                ask(world, request.clone());
                            }
                        }),
                    button("CANCEL")
                        .style(ButtonStyle::Quiet)
                        .on_click(close),
                ))
                .boxed(),
            );
            column(parts).space(Space::Half).boxed()
        },
    )
    .boxed()
}

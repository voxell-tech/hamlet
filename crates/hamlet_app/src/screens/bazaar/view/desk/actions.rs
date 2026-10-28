//! A version's facts, and what can be done with it as it stands: a
//! draft published unverified, paid to be verified or thrown away; an
//! unverified one paid to be verified; a rejected one published or
//! handed to a person.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    action, bad_action, bare, buttons, marked, note, primary_button,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Wraps;

use super::form::save_form;
use super::look::{boxed, line, lines, plain_box, state_pill, value};
use crate::bazaar::market::drafts::{
    Drafts, Step, Then, Which, discard, go_on,
};
use crate::bazaar::market::lot::{Lot, price_words};
use crate::bazaar::market::versions::{State, Version, size_words};
use crate::bazaar::market::{Market, ask_human};
use crate::bazaar::view::parts::{outcome, problem};
use crate::roll::{either, moved};

/// The box of `version`'s facts: when it was uploaded, its size and
/// how many files it has.
pub(super) fn facts(version: &Version) -> AnyView {
    boxed(
        format!("V{}", version.version),
        state_pill(version.state),
        lines(vec![
            line("uploaded", value(version.uploaded())),
            line("size", value(size_words(version.size))),
            line("files", value(version.file_count.to_string())),
        ]),
    )
}

/// What can be done with `version` of `lot`, and how the last thing
/// done went.
pub(super) fn actions(lot: &Lot, version: &Version) -> AnyView {
    let which = Which {
        id: lot.id.clone(),
        version: version.version.clone(),
    };
    let (kind, versions) = (lot.kind, lot.versions.clone());
    let state = version.state;
    let reasons = version.reasons.clone();
    let asked = version.human_asked_at.is_some();
    let now = derived(move |world: &World| {
        let drafts = world.resource::<Drafts>();
        let fee = drafts
            .fees
            .as_ref()
            .map(|fees| fees.to_verify(kind, &versions));
        let paying = drafts.paying.as_ref().is_some_and(|paying| {
            paying.which.version == which.version
        });
        (
            fee,
            drafts.job.as_ref().map(|job| job.step),
            drafts.said.clone(),
            paying,
        )
    })
    .when(either(moved::<Drafts>(), moved::<Market>()));
    let which = Which {
        id: lot.id.clone(),
        version: version.version.clone(),
    };
    let body = keyed(now, move |(fee, job, said, paying)| {
        let mut parts =
            by_state(state, &which, *fee, &reasons, asked);
        if let Some(step) = job {
            parts.push(note(going(*step)).boxed());
        }
        if *paying {
            parts.push(
                note("Waiting for Stripe to say it is paid...")
                    .boxed(),
            );
        }
        if let Some((said, wrong)) = said {
            parts.push(outcome(said.clone(), *wrong));
        }
        column(parts).gap(8.0).boxed()
    })
    .within(bare());
    plain_box(column((body,)).padding(UiRect::all(px(12.0))).boxed())
}

/// The words for `step` while it goes.
fn going(step: Step) -> &'static str {
    match step {
        Step::Saving => "Saving the draft...",
        Step::Covering => "Sending its cover...",
        Step::Publishing => "Publishing it...",
        Step::Verifying => "Opening Stripe...",
        Step::Discarding => "Discarding the draft...",
    }
}

/// The lamp-lit button to pay for `which` to be verified, for `fee`.
fn pay(which: &Which, fee: Option<u32>, draft: bool) -> AnyView {
    let words = match fee {
        Some(cents) => {
            format!("PAY TO VERIFY \u{b7} {}", price_words(cents))
        }
        None => "PAY TO VERIFY".to_string(),
    };
    let which = which.clone();
    marked(Glyph::Shield, words)
        .rules(primary_button)
        .tagged(Variant::Primary)
        .on_activate(move |world: &mut World| match draft {
            true => {
                save_form(world, Then::Verify);
            }
            false => go_on(world, which.clone(), Then::Verify),
        })
        .boxed()
}

/// ASK A HUMAN about `which`, or that one was asked.
fn ask(which: &Which, asked: bool) -> AnyView {
    let which = which.clone();
    match asked {
        true => note("A person will look at it.").boxed(),
        false => action(Glyph::Talk, "ASK A HUMAN", move |world| {
            ask_human(world, &which.id, &which.version);
        }),
    }
}

fn by_state(
    state: State,
    which: &Which,
    fee: Option<u32>,
    reasons: &[String],
    asked: bool,
) -> Vec<AnyView> {
    let line_of = |words: &'static str| {
        label(words).size(13.0).tone(Tone::Dim).boxed()
    };
    match state {
        State::Draft => {
            let thrown = which.clone();
            vec![
                buttons(vec![
                    action(
                        Glyph::Globe,
                        "PUBLISH UNVERIFIED",
                        |world| {
                            save_form(world, Then::Publish);
                        },
                    ),
                    pay(which, fee, true),
                    bad_action(
                        Glyph::Delete,
                        "DISCARD DRAFT",
                        move |world| {
                            discard(world, thrown.clone());
                        },
                    ),
                ])
                .boxed(),
                note(
                    "Optional \u{b7} locks this version \u{b7} no \
                     refunds.",
                )
                .boxed(),
            ]
        }
        State::Unverified => {
            let mut parts = vec![
                line_of("Live, without a badge.").boxed(),
                note(
                    "To verify it, make a new version and pay to \
                     verify that.",
                )
                .boxed(),
            ];
            // Published after a rejection, it can still go to a
            // person.
            if !reasons.is_empty() {
                parts.extend(
                    reasons.iter().map(|why| problem(why.clone())),
                );
                parts.push(ask(which, asked));
            }
            parts
        }
        State::Checking => vec![
            row((
                state_pill(State::Checking),
                note(
                    "Our agents are checking it. It goes live with \
                     a badge if all of them agree.",
                ),
            ))
            .gap(10.0)
            .align(AlignItems::Center)
            .tagged(Wraps)
            .boxed(),
        ],
        State::Verified => vec![
            row((
                state_pill(State::Verified),
                line_of("Live with a badge."),
            ))
            .gap(10.0)
            .align(AlignItems::Center)
            .boxed(),
        ],
        State::Rejected => {
            let mut parts = reasons
                .iter()
                .map(|why| problem(why.clone()))
                .collect::<Vec<_>>();
            let published = which.clone();
            let human = which.clone();
            let mut pressed = vec![action(
                Glyph::Globe,
                "PUBLISH UNVERIFIED",
                move |world| {
                    go_on(world, published.clone(), Then::Publish);
                },
            )];
            pressed.push(ask(&human, asked));
            parts.push(buttons(pressed).boxed());
            parts.push(
                note(
                    "Fix it in a new version, publish it without a \
                     badge, or ask a person to look again.",
                )
                .boxed(),
            );
            parts
        }
    }
}

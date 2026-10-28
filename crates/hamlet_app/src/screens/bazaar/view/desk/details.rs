//! A version's details: while it is a draft, the form they are typed
//! in, with its cover, title, summary, price, description and tags;
//! after, the box they are shown in, locked.

use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, bare, command, note, quiet, text_input,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::{DisplayText, Wraps};

use super::form::{Form, cents, least, price_of, save_form};
use super::look::{
    aside, boxed, initial, line, lines, plain_pill, value,
};
use super::{Desk, description, tag_field};
use crate::bazaar::covers::{
    MOST_BYTES, MOST_SIDE, Pick, Picked, pick,
};
use crate::bazaar::market::drafts::{Drafts, Then};
use crate::bazaar::market::lot::price_words;
use crate::bazaar::market::versions::{Details, Version};
use crate::bazaar::market::{Market, check_payouts, set_up_payouts};
use crate::bazaar::view::covers::chosen;
use crate::bazaar::view::parts::{problem, wide};
use crate::roll::{either, moved};

/// The frame a cover is shown in.
const COVER: Vec2 = Vec2::new(160.0, 90.0);

/// The details of `version`: its form while it is a draft, else the
/// box they are locked in.
pub(super) fn details(version: &Version, narrow: bool) -> AnyView {
    match version.state {
        crate::bazaar::market::versions::State::Draft => form(narrow),
        _ => locked(&version.details, narrow),
    }
}

/// The cover, else the title's initial, with `none` in its place.
fn cover_of(title: &str) -> AnyView {
    let title = title.to_string();
    chosen(COVER, move || initial(&title, COVER.y))
}

fn locked(details: &Details, narrow: bool) -> AnyView {
    let tags = match details.tags.is_empty() {
        true => note("none").boxed(),
        false => row(details
            .tags
            .iter()
            .map(|tag| plain_pill(tag.clone()))
            .collect::<Vec<_>>())
        .gap(4.0)
        .justify(JustifyContent::FlexEnd)
        .tagged(Wraps)
        .boxed(),
    };
    let summary = match details.summary.is_empty() {
        true => note("none").boxed(),
        false => value(details.summary.clone()),
    };
    let facts = lines(vec![
        line("title", value(details.title.clone())),
        line("summary", summary),
        line("price", value(price_words(details.price_cents))),
        line("tags", tags),
    ]);
    let cover = column((cover_of(&details.title),))
        .padding(UiRect::all(px(12.0)));
    let top = match narrow {
        true => column((cover, facts)).gap(0.0).boxed(),
        false => row((cover, column((facts,)).fills_width()))
            .gap(0.0)
            .boxed(),
    };
    let mut parts = vec![top];
    if !details.description.is_empty() {
        parts.push(
            column((
                label("description").size(12.0).tone(Tone::Accent),
                label(details.description.clone()).size(13.0),
            ))
            .gap(4.0)
            .padding(UiRect::axes(px(12.0), px(10.0)))
            .boxed(),
        );
    }
    boxed(
        "DETAILS".into(),
        aside("locked \u{b7} make a new version to change them"),
        lines(parts),
    )
}

/// A field of the form: its name over the words in it.
fn field(
    name: &'static str,
    read: fn(&Form) -> String,
    write: fn(&mut Form, String),
) -> AnyView {
    let typed = text_field(
        resource::<Desk, _>(move |desk| {
            desk.form.as_ref().map(read).unwrap_or_default()
        }),
        move |world: &mut World, typed| {
            if let Some(form) = &mut world.resource_mut::<Desk>().form
            {
                write(form, typed);
            }
        },
    )
    .rules(text_input);
    named(name, wide(typed))
}

fn named(name: &'static str, control: AnyView) -> AnyView {
    column((
        label(name)
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText),
        control,
    ))
    .gap(6.0)
    .align(AlignItems::Stretch)
    .boxed()
}

fn form(narrow: bool) -> AnyView {
    let title = derived(|world: &World| {
        world
            .resource::<Desk>()
            .form
            .as_ref()
            .map(|form| form.title.clone())
    })
    .when(moved::<Desk>());
    let cover = keyed(title, |title| {
        cover_of(title.as_deref().unwrap_or_default())
    })
    .within(bare());
    let change = command("CHANGE", |world| {
        let mut desk = world.resource_mut::<Desk>();
        desk.covering = !desk.covering;
    });
    let cover = column((cover, change, picking()))
        .gap(8.0)
        .align(AlignItems::FlexStart)
        .width(px(COVER.x + 8.0));
    let fields = column((
        field(
            "Title",
            |form| form.title.clone(),
            |form, typed| form.title = typed,
        ),
        field(
            "Summary",
            |form| form.summary.clone(),
            |form, typed| form.summary = typed,
        ),
        named("Price", price_field()),
    ))
    .gap(12.0)
    .fills_width();
    let top = match narrow {
        true => {
            column((cover.boxed(), fields.boxed())).gap(12.0).boxed()
        }
        false => {
            row((cover.boxed(), fields.boxed())).gap(16.0).boxed()
        }
    };
    let foot = keyed(
        resource::<Desk, _>(|desk| {
            desk.form.as_ref().and_then(|form| form.problem.clone())
        }),
        |why| match why {
            Some(why) => problem(why.clone()),
            None => bare().boxed(),
        },
    )
    .within(bare());
    let body = column((
        top,
        named("Description", description::description()),
        named("Tags", tag_field::tags()),
        foot,
        row((
            column(()).fills_width(),
            command("SAVE DRAFT", |world| {
                save_form(world, Then::Stay);
            }),
        )),
    ))
    .gap(14.0)
    .padding(UiRect::all(px(12.0)));
    boxed(
        "DETAILS".into(),
        aside("editable until you publish or verify"),
        body.boxed(),
    )
}

/// The price's field, its least and the way to be paid.
fn price_field() -> AnyView {
    let typed = text_field(
        resource::<Desk, _>(|desk| {
            desk.form
                .as_ref()
                .map(|form| form.price.clone())
                .unwrap_or_default()
        }),
        |world: &mut World, typed| {
            if let Some(form) = &mut world.resource_mut::<Desk>().form
            {
                form.price = typed;
            }
        },
    )
    .width(px(110.0))
    .rules(text_input);
    let now = derived(|world: &World| {
        let least = least(world);
        let price = world
            .resource::<Desk>()
            .form
            .as_ref()
            .map(|form| form.price.clone());
        let paid = world
            .resource::<Market>()
            .user()
            .and_then(|user| user.payouts)
            .is_some_and(|payouts| payouts.charges_enabled);
        (price.map(|price| cents(&price, least)), least, paid)
    })
    .when(either(
        either(moved::<Desk>(), moved::<Market>()),
        moved::<Drafts>(),
    ));
    let state = keyed(now, |(cents, least, paid)| {
        let hint =
            format!("At least {}, or 0 for free", price_of(*least));
        let mut parts = vec![match cents {
            Some(Err(_)) => problem(hint),
            _ => note(hint).boxed(),
        }];
        if matches!(cents, Some(Ok(cents)) if *cents > 0) && !paid {
            parts.push(
                row((
                    note("To be paid,"),
                    quiet(
                        Glyph::Coins,
                        "SET UP PAYOUTS",
                        set_up_payouts,
                    ),
                    note("with Stripe."),
                    quiet(
                        Glyph::Restore,
                        "CHECK AGAIN",
                        check_payouts,
                    ),
                ))
                .gap(6.0)
                .align(AlignItems::Center)
                .tagged(Wraps)
                .boxed(),
            );
        }
        column(parts).gap(6.0).boxed()
    })
    .within(bare());
    row((typed, state))
        .gap(10.0)
        .align(AlignItems::Center)
        .tagged(Wraps)
        .boxed()
}

/// Where the cover's picture comes from, while it is being chosen.
fn picking() -> AnyView {
    keyed(resource::<Desk, _>(|desk| desk.covering), |covering| {
        if !*covering {
            return bare().boxed();
        }
        let from = text_field(
            resource::<Pick, _>(|pick| pick.from.clone()),
            pick,
        )
        .width(px(COVER.x + 8.0))
        .rules(text_input);
        let state = keyed(
            resource::<Pick, _>(|pick| match &pick.state {
                Picked::Fetching => {
                    Some(Ok("Fetching it...".to_string()))
                }
                Picked::Got(picture) => Some(Ok(format!(
                    "{} by {} pixels.",
                    picture.size.x, picture.size.y
                ))),
                Picked::Failed(why) => Some(Err(why.clone())),
                Picked::Nothing => None,
            }),
            |state| match state {
                Some(Ok(words)) => note(words.clone()).boxed(),
                Some(Err(why)) => problem(why.clone()),
                None => bare().boxed(),
            },
        )
        .within(bare());
        column((
            from,
            note(format!(
                "A path or web address: PNG, JPEG or WebP, at most \
                 {} KB and {MOST_SIDE} pixels a side.",
                MOST_BYTES / 1024
            )),
            state,
        ))
        .gap(6.0)
        .boxed()
    })
    .within(bare())
    .boxed()
}

//! The bazaar's shelf: the wares of a source, narrowed, ordered and
//! shown a few at a time.

use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, dropdown, label, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_core::bazaar::{Bazaar, FetchWares, LookAt};
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{ListModels, Listing, Settings};
use hamlet_model::bazaar::{
    Sort, Ware, is_openrouter, makers, shown,
};

use super::{Browsing, SHOWN, added};
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, action, bare, buttons, note, plate, stated, tick,
    well,
};
use crate::platform::DisplayText;
use crate::roll::{either, moved};
use crate::{AnyView, View};

/// The prices a ware's price in can be kept under, in dollars for a
/// million tokens.
const PRICES: [(&str, Option<f64>); 5] = [
    ("Any price", None),
    ("Free", Some(0.0)),
    ("Up to $1", Some(1.0)),
    ("Up to $5", Some(5.0)),
    ("Up to $20", Some(20.0)),
];

/// The least context a ware can be asked to hold, in tokens.
const SIZES: [(&str, Option<u64>); 4] = [
    ("Any context", None),
    ("32k or more", Some(32_000)),
    ("128k or more", Some(128_000)),
    ("1M or more", Some(1_000_000)),
];

/// The field the shelf is narrowed in.
#[derive(Component)]
pub(super) struct Searching;

/// Follows what narrows the shelf as it is typed.
pub(super) fn read_words(
    field: Query<&Children, With<Searching>>,
    texts: Query<&EditableText>,
    mut browsing: ResMut<Browsing>,
) {
    let Some(text) = field
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != browsing.filter.words.as_str() {
        browsing.filter.words = text.value().to_string();
        browsing.shown = SHOWN;
    }
}

/// Changes what the chief is looking at, from the first rows again.
fn browse(world: &mut World, change: impl FnOnce(&mut Browsing)) {
    let mut browsing = world.resource_mut::<Browsing>();
    change(&mut browsing);
    browsing.shown = SHOWN;
}

pub(super) fn small(text: &'static str) -> impl View {
    label(text)
        .size(11.0)
        .bold(true)
        .tone(Tone::Faint)
        .tagged(DisplayText)
}

/// Dollars for a million tokens, to the cent or past it for less.
pub(super) fn dollars(price: f64) -> String {
    if price > 0.0 && price < 0.01 {
        format!("${price:.4}")
    } else {
        format!("${price:.2}")
    }
}

/// A ware's price in and out, in a few words.
pub(super) fn price_words(ware: &Ware) -> String {
    match ware.price() {
        None => "Price varies".into(),
        Some(price) if price.input == 0.0 && price.output == 0.0 => {
            "Free".into()
        }
        Some(price) => format!(
            "{} in, {} out",
            dollars(price.input),
            dollars(price.output)
        ),
    }
}

/// A count of tokens, short: `8k`, `1M`.
pub(super) fn tokens(count: u64) -> String {
    if count >= 1_000_000 && count.is_multiple_of(100_000) {
        let millions = count as f64 / 1e6;
        format!("{}M", (millions * 10.0).round() / 10.0)
    } else if count >= 1000 {
        format!("{}k", count / 1000)
    } else {
        count.to_string()
    }
}

/// What a row tells of a ware besides its name and price.
fn facts(ware: &Ware) -> String {
    let mut facts = vec![ware.id.clone()];
    if let Some(context) = ware.context {
        facts.push(format!("{} context", tokens(context)));
    }
    match ware.tools {
        Some(true) => facts.push("tools".into()),
        Some(false) => facts.push("no tools".into()),
        None => {}
    }
    if ware.reasoning == Some(true) {
        facts.push("reasoning".into());
    }
    if ware.sees() {
        facts.push("images".into());
    }
    if let Some(index) = ware.intelligence {
        facts.push(format!("intelligence {index:.0}"));
    }
    facts.join("  ·  ")
}

/// One row of the shelf, as it is drawn.
#[derive(Clone, PartialEq)]
struct Shown {
    id: String,
    title: String,
    price: String,
    facts: String,
    added: bool,
}

/// The rows the shelf shows, and the count of those past them.
fn rows_now(world: &World) -> (Vec<Shown>, usize) {
    let browsing = world.resource::<Browsing>();
    let bazaar = world.resource::<Bazaar>();
    let source = browsing.source.as_deref();
    let all = shown(
        browsing.wares(bazaar),
        &browsing.filter,
        browsing.sort,
    );
    let rows = all
        .iter()
        .take(browsing.shown)
        .map(|ware| Shown {
            id: ware.id.clone(),
            title: ware.title.clone(),
            price: price_words(ware),
            facts: facts(ware),
            added: added(world, source, &ware.id),
        })
        .collect::<Vec<_>>();
    let more = all.len() - rows.len();
    (rows, more)
}

/// A ware's row, which opens its page.
fn shown_row(shown: &Shown) -> AnyView {
    let id = shown.id.clone();
    let mut top = vec![
        row((label(shown.title.clone()).bold(true),))
            .fills_width()
            .boxed(),
    ];
    if shown.added {
        top.push(
            stated(Glyph::Check, "ADDED", Tone::Accent, 11.0).boxed(),
        );
    }
    top.push(
        label(shown.price.clone())
            .size(12.0)
            .tone(Tone::Faint)
            .wrap(false)
            .boxed(),
    );
    button(
        column((
            row(top).gap(10.0).align(AlignItems::Center),
            note(shown.facts.clone()),
        ))
        .gap(3.0)
        .grow(1.0)
        .min_width(px(0.0)),
    )
    .padding(UiRect::axes(px(10.0), px(8.0)))
    .rules(plate)
    .on_activate(move |world| look(world, &id))
    .boxed()
}

/// Opens the ware `id` on its page, and asks who serves it.
pub(super) fn look(world: &mut World, id: &str) {
    let mut browsing = world.resource_mut::<Browsing>();
    browsing.open = Some(id.to_string());
    browsing.said = None;
    if browsing.source.is_none() {
        world.trigger(LookAt(id.to_string()));
    }
}

/// The endpoints whose models can be browsed: OpenRouter first, then
/// each other endpoint of the settings, by name.
fn sources(world: &World) -> Vec<Option<String>> {
    let config = &world.resource::<Settings>().config;
    std::iter::once(None)
        .chain(
            config
                .endpoints
                .iter()
                .filter(|endpoint| !is_openrouter(&endpoint.base_url))
                .map(|endpoint| Some(endpoint.name.clone())),
        )
        .collect()
}

/// A dropdown over `options`, set from and into the browsing.
fn choice(
    options: Vec<String>,
    at: impl Fn(&Browsing) -> usize + Send + Sync + 'static,
    set: impl Fn(&mut Browsing, usize) + Send + Sync + 'static,
) -> AnyView {
    dropdown(
        options,
        resource::<Browsing, _>(at),
        Glyph::Chevron.image(ICON),
        move |world, chosen| browse(world, |b| set(b, chosen)),
    )
    .min_width(px(130.0))
    .boxed()
}

/// The choice of where the models shown come from.
fn source_choice() -> AnyView {
    let all = derived(sources)
        .when(either(moved::<Settings>(), moved::<Bazaar>()));
    keyed(all, |all| {
        let names = all
            .iter()
            .map(|source| match source {
                None => "OpenRouter".to_string(),
                Some(name) => format!("Endpoint: {name}"),
            })
            .collect::<Vec<_>>();
        let (at, set) = (all.clone(), all.clone());
        choice(
            names,
            move |b| {
                at.iter().position(|s| *s == b.source).unwrap_or(0)
            },
            move |b, chosen| {
                b.source = set.get(chosen).cloned().flatten();
                b.filter.maker = None;
            },
        )
    })
    .within(bare())
    .boxed()
}

/// The choice of the maker of the models shown.
fn maker_choice() -> AnyView {
    let all = derived(|world: &World| {
        let browsing = world.resource::<Browsing>();
        makers(browsing.wares(world.resource::<Bazaar>()))
    })
    .when(either(moved::<Browsing>(), moved::<Bazaar>()));
    keyed(all, |all| {
        if all.is_empty() {
            return bare().boxed();
        }
        let names = std::iter::once("Any maker".to_string())
            .chain(all.iter().cloned())
            .collect::<Vec<_>>();
        let (at, set) = (all.clone(), all.clone());
        choice(
            names,
            move |b| {
                b.filter.maker.as_ref().map_or(0, |maker| {
                    at.iter()
                        .position(|m| m == maker)
                        .map_or(0, |i| i + 1)
                })
            },
            move |b, chosen| {
                b.filter.maker = chosen
                    .checked_sub(1)
                    .and_then(|i| set.get(i).cloned());
            },
        )
    })
    .within(bare())
    .boxed()
}

/// A switch of the filter and its words.
fn switch(
    words: &'static str,
    on: fn(&Browsing) -> bool,
    set: fn(&mut Browsing, bool),
) -> AnyView {
    row((
        tick(resource::<Browsing, _>(on)).on_change(
            move |world, to| browse(world, |b| set(b, to)),
        ),
        label(words).size(12.0),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .boxed()
}

fn controls() -> impl View {
    let search = row((
        small("FIND"),
        text_field(
            resource::<Browsing, _>(|b| b.filter.words.clone()),
            |world, text| browse(world, |b| b.filter.words = text),
        )
        .grow(1.0)
        .rules(well)
        .tagged(Searching),
        action(Glyph::Restore, "LOOK AGAIN", |world| {
            world.trigger(FetchWares);
            world.trigger(ListModels);
        }),
    ))
    .gap(12.0)
    .align(AlignItems::Center);
    let orders = buttons(vec![
        source_choice(),
        choice(
            Sort::ALL.iter().map(|s| s.name().to_string()).collect(),
            |b| {
                Sort::ALL
                    .iter()
                    .position(|s| *s == b.sort)
                    .unwrap_or(0)
            },
            |b, chosen| b.sort = Sort::ALL[chosen],
        ),
        maker_choice(),
        choice(
            PRICES.iter().map(|(name, _)| name.to_string()).collect(),
            |b| {
                PRICES
                    .iter()
                    .position(|(_, most)| {
                        *most == b.filter.most_input
                    })
                    .unwrap_or(0)
            },
            |b, chosen| b.filter.most_input = PRICES[chosen].1,
        ),
        choice(
            SIZES.iter().map(|(name, _)| name.to_string()).collect(),
            |b| {
                SIZES
                    .iter()
                    .position(|(_, least)| {
                        *least == b.filter.least_context
                    })
                    .unwrap_or(0)
            },
            |b, chosen| b.filter.least_context = SIZES[chosen].1,
        ),
    ]);
    let switches = buttons(vec![
        switch(
            "Calls tools",
            |b| b.filter.tools,
            |b, on| b.filter.tools = on,
        ),
        switch(
            "Reasons",
            |b| b.filter.reasoning,
            |b, on| b.filter.reasoning = on,
        ),
        switch(
            "Reads images",
            |b| b.filter.sees,
            |b, on| b.filter.sees = on,
        ),
    ]);
    column((search, orders, switches)).gap(10.0)
}

/// How many wares are shown of how many, and how fresh they are.
fn summary(world: &World) -> String {
    let browsing = world.resource::<Browsing>();
    let bazaar = world.resource::<Bazaar>();
    let wares = browsing.wares(bazaar);
    let count = shown(wares, &browsing.filter, browsing.sort).len();
    match &browsing.source {
        None if bazaar.fetching => "Asking OpenRouter...".into(),
        None if wares.is_empty() => {
            bazaar.problem.as_ref().map_or_else(
                || {
                    "Nothing on the shelf yet. Choose Look Again."
                        .into()
                },
                |why| format!("OpenRouter could not be asked: {why}"),
            )
        }
        None => {
            let hours = hamlet_model::log::now_millis()
                .saturating_sub(bazaar.wares.fetched)
                / 3_600_000;
            let ago = match hours {
                0 => "less than an hour ago".to_string(),
                1 => "an hour ago".to_string(),
                hours => format!("{hours} hours ago"),
            };
            format!(
                "{count} of {} models. OpenRouter was asked {ago}.",
                wares.len()
            )
        }
        Some(name) => {
            let listing = world.resource::<Listing>();
            match listing.said.get(name) {
                Some(Err(why)) => {
                    format!("{name} could not be asked: {why}")
                }
                _ if wares.is_empty() && listing.asking > 0 => {
                    format!("Asking {name}...")
                }
                _ if wares.is_empty() => format!(
                    "Nothing from {name} yet. Choose Look Again."
                ),
                _ => format!(
                    "{count} of {} models from {name}. Little is \
                     told of them besides their names.",
                    wares.len()
                ),
            }
        }
    }
}

/// Who the chief came to choose for, and what came of the last thing
/// done.
pub(super) fn heading() -> AnyView {
    let now = derived(|world: &World| {
        let browsing = world.resource::<Browsing>();
        let agent = browsing
            .for_agent
            .filter(|agent| world.get_entity(*agent).is_ok())
            .map(|agent| hamlet_core::agents::name_of(world, agent));
        (agent, browsing.said.clone())
    })
    .when(moved::<Browsing>());
    keyed(now, |(agent, said)| {
        let mut parts = Vec::new();
        if let Some(agent) = agent {
            parts.push(
                row((
                    note(format!(
                        "Choosing a model for {agent}. Open one to \
                         have {agent} run it."
                    )),
                    action(Glyph::Cross, "FOR NOBODY", |world| {
                        world.resource_mut::<Browsing>().for_agent =
                            None;
                    }),
                ))
                .gap(12.0)
                .align(AlignItems::Center)
                .boxed(),
            );
        }
        if let Some((said, wrong)) = said {
            let tone =
                if *wrong { Tone::Critical } else { Tone::Accent };
            parts.push(
                label(said.clone()).size(12.0).tone(tone).boxed(),
            );
        }
        column(parts).gap(6.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// The shelf: the controls, then the rows.
pub(super) fn page() -> AnyView {
    let rows = derived(rows_now).when(either(
        moved::<Browsing>(),
        either(
            moved::<Bazaar>(),
            either(moved::<ModelCatalogue>(), moved::<Settings>()),
        ),
    ));
    let list = keyed(rows, |(rows, more)| {
        let mut shown =
            rows.iter().map(shown_row).collect::<Vec<_>>();
        if *more > 0 {
            shown.push(
                row((action(Glyph::More, "SHOW MORE", |world| {
                    world.resource_mut::<Browsing>().shown += SHOWN;
                }),))
                .padding(UiRect::top(px(6.0)))
                .boxed(),
            );
        }
        column(shown).gap(2.0).boxed()
    })
    .within(bare());
    let counted = derived(summary).when(either(
        moved::<Browsing>(),
        either(moved::<Bazaar>(), moved::<Listing>()),
    ));
    column((heading(), controls(), note(counted), list))
        .gap(12.0)
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_price_and_a_size_are_told_shortly() {
        assert_eq!(tokens(1_000_000), "1M");
        assert_eq!(tokens(1_050_000), "1050k");
        assert_eq!(tokens(2_500_000), "2.5M");
        assert_eq!(tokens(131_072), "131k");
        assert_eq!(dollars(0.0005), "$0.0005");
        assert_eq!(dollars(15.0), "$15.00");
        let ware = |input, output| Ware {
            input,
            output,
            ..Ware::default()
        };
        assert_eq!(price_words(&ware(Some(0.0), Some(0.0))), "Free");
        assert_eq!(
            price_words(&ware(None, Some(1.0))),
            "Price varies"
        );
        assert_eq!(
            price_words(&ware(Some(0.1), Some(0.5))),
            "$0.10 in, $0.50 out"
        );
    }
}

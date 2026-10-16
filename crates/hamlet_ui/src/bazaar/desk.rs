//! The seller's desk: a form to put an agent, a hamlet or tools up
//! for sale, or to change a listing and upload a new version of it,
//! and the page of what the chief sells and has bought.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_core::core::HomeDir;
use hamlet_core::market::{
    AskAccount, Market, Sell, Selling, SetUpPayouts, Shelve, Wares,
};
use hamlet_model::market::artisans::Craft;
use hamlet_model::market::lot::{
    Draft, Kind, Lot, Status, next_version, semver,
};
use hamlet_model::market::{Price, SELLERS_SHARE, plugins};
use hamlet_rules::market::tools;
use hamlet_rules::realm::Realm;

use super::artisans::{chip, part};
use super::trade;
use crate::icons::Glyph;
use crate::looks::{
    ICON, action, bare, buttons, good_action, note, page_head,
    stated, tick, well,
};
use crate::platform::Wraps;
use crate::roll::{either, moved};
use crate::{AnyView, Cx};

/// The longest a title, a summary and a description may be.
const TITLE: usize = 60;
const SUMMARY: usize = 140;
const DESCRIPTION: usize = 5000;
const CATEGORY: usize = 32;
const TAGS: usize = 8;
const TAG: usize = 24;

/// The least and most a paid listing may cost, in cents.
const LEAST_CENTS: u32 = 50;
const MOST_CENTS: u32 = 50_000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Desk>().add_systems(Update, read_fields);
}

/// What the chief is doing at the seller's desk.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(super) struct Desk {
    /// The desk's page open in place of the stall's, if one is.
    pub(super) page: Option<Page>,
    form: Form,
}

/// A page of the desk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    Form,
    Yours,
}

/// The listing being put up or changed.
#[derive(Clone, PartialEq, Debug)]
struct Form {
    /// The listing changed; `None` for a new one.
    id: Option<String>,
    kind: Kind,
    title: String,
    summary: String,
    description: String,
    category: String,
    /// Separated by commas.
    tags: String,
    /// In dollars, empty or `0` for free.
    price: String,
    /// A new version is uploaded with it.
    upload: bool,
    /// The agent, hamlet or tools chosen, by their place in the
    /// choices.
    chosen: Vec<usize>,
    notes: bool,
    /// The path of a picture for its cover.
    cover: String,
    version: String,
    changes: String,
    /// Why the form cannot be sent as it is.
    problem: Option<String>,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            id: None,
            kind: Kind::Agent,
            title: String::new(),
            summary: String::new(),
            description: String::new(),
            category: String::new(),
            tags: String::new(),
            price: String::new(),
            upload: true,
            chosen: vec![0],
            notes: true,
            cover: String::new(),
            version: "1.0.0".into(),
            changes: String::new(),
            problem: None,
        }
    }
}

/// A field of the form, on the field's node.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Title,
    Summary,
    Description,
    Category,
    Tags,
    Price,
    Cover,
    Version,
    Changes,
}

impl Field {
    fn of(self, form: &mut Form) -> &mut String {
        match self {
            Field::Title => &mut form.title,
            Field::Summary => &mut form.summary,
            Field::Description => &mut form.description,
            Field::Category => &mut form.category,
            Field::Tags => &mut form.tags,
            Field::Price => &mut form.price,
            Field::Cover => &mut form.cover,
            Field::Version => &mut form.version,
            Field::Changes => &mut form.changes,
        }
    }
}

/// Follows the form's fields as they are typed in, so that what is
/// sent is what shows.
fn read_fields(
    fields: Query<(&Field, &Children)>,
    texts: Query<&EditableText>,
    desk: Option<ResMut<Desk>>,
) {
    let Some(mut desk) = desk else {
        return;
    };
    for (field, children) in &fields {
        let typed = children
            .iter()
            .find_map(|child| texts.get(*child).ok())
            .map(|text| text.value().to_string());
        if let Some(typed) = typed
            && *field.of(&mut desk.bypass_change_detection().form)
                != typed
        {
            *field.of(&mut desk.form) = typed;
        }
    }
}

/// Opens the form to put up a new listing of `kind`, or `lot` to
/// change it.
pub(super) fn open_form(
    world: &mut World,
    kind: Kind,
    lot: Option<&Lot>,
) {
    let form = match lot {
        None => Form {
            kind,
            category: match kind {
                Kind::Agent | Kind::Hamlet => {
                    Craft::ALL[0].name().into()
                }
                Kind::Plugin => plugins::Kind::ALL[0].name().into(),
            },
            ..Form::default()
        },
        Some(lot) => Form {
            id: Some(lot.id.clone()),
            kind: lot.kind,
            title: lot.title.clone(),
            summary: lot.summary.clone(),
            description: lot.description.clone(),
            category: lot.category.clone(),
            tags: lot.tags.join(", "),
            price: match lot.price() {
                Price::Free => String::new(),
                Price::Once(dollars) => format!("{dollars:.2}"),
            },
            upload: lot.latest.is_none(),
            version: next_version(lot.version()),
            ..Form::default()
        },
    };
    let mut desk = world.resource_mut::<Desk>();
    desk.form = form;
    desk.page = Some(Page::Form);
    if let Some(mut market) = world.get_resource_mut::<Market>() {
        market.selling = None;
    }
}

/// Opens the form to change the listing `id` of the chief's.
pub(super) fn open_edit(world: &mut World, id: &str) {
    let Some(lot) = world.resource::<Market>().lot(id).cloned()
    else {
        return;
    };
    open_form(world, lot.kind, Some(&lot));
}

/// Opens the page of what the chief sells and has bought.
pub(super) fn open_yours(world: &mut World) {
    world.resource_mut::<Desk>().page = Some(Page::Yours);
    world.trigger(AskAccount);
}

fn close(world: &mut World) {
    world.resource_mut::<Desk>().page = None;
}

/// The desk's page open, if one is, else the stall's page `stall`
/// makes.
pub(super) fn or(stall: fn() -> AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        cx.world.init_resource::<Desk>();
        cx.build(
            keyed(
                resource::<Desk, _>(|desk| desk.page),
                move |page| match page {
                    Some(Page::Form) => form(),
                    Some(Page::Yours) => yours(),
                    None => stall(),
                },
            )
            .within(bare()),
        )
    })
}

/// What can be sold, by name, for a listing of `kind`.
fn choices(
    world: &World,
    kind: Kind,
    notes: bool,
) -> Vec<(String, Wares)> {
    let Some(home) = world.get_resource::<HomeDir>() else {
        return Vec::new();
    };
    let realm = Realm::new(home.0.root.clone());
    match kind {
        Kind::Agent => {
            let mut all = Vec::new();
            for place in realm.hamlets() {
                let Ok(hamlet) = realm.hamlet(&place.name) else {
                    continue;
                };
                for agent in hamlet.members() {
                    all.push((
                        format!(
                            "{} of {}",
                            agent.name,
                            place.shown()
                        ),
                        Wares::Agent {
                            hamlet: place.name.clone(),
                            agent: agent.name,
                            notes,
                        },
                    ));
                }
            }
            all
        }
        Kind::Hamlet => realm
            .hamlets()
            .iter()
            .map(|place| {
                (
                    place.shown().to_string(),
                    Wares::Hamlet {
                        name: place.name.clone(),
                        kind: String::new(),
                        notes,
                    },
                )
            })
            .collect(),
        Kind::Plugin => tools(&realm)
            .into_iter()
            .map(|tool| {
                (
                    format!(
                        "{} of {} in {}",
                        tool.name, tool.agent, tool.hamlet
                    ),
                    Wares::Tools(vec![tool.dir]),
                )
            })
            .collect(),
    }
}

/// The price typed, in cents, or why it is not one.
fn cents(price: &str) -> Result<u32, String> {
    let price = price.trim().trim_start_matches('$');
    if price.is_empty() {
        return Ok(0);
    }
    let dollars = price
        .parse::<f64>()
        .ok()
        .filter(|dollars| dollars.is_finite() && *dollars >= 0.0)
        .ok_or("The price is a number of dollars, such as 4.99.")?;
    let cents = (dollars * 100.0).round() as u32;
    match cents {
        0 => Ok(0),
        cents if (LEAST_CENTS..=MOST_CENTS).contains(&cents) => {
            Ok(cents)
        }
        _ => Err("A price is free, or from $0.50 to $500.".into()),
    }
}

/// The tags typed, or why they are not.
fn tags(typed: &str) -> Result<Vec<String>, String> {
    let mut all = Vec::<String>::new();
    for tag in typed.split(',') {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() || all.contains(&tag) {
            continue;
        }
        if tag.chars().count() > TAG {
            return Err(format!("A tag is at most {TAG} letters."));
        }
        all.push(tag);
    }
    if all.len() > TAGS {
        return Err(format!("At most {TAGS} tags."));
    }
    Ok(all)
}

/// What the form sends, or why it cannot be sent.
fn offer(world: &World, publish: bool) -> Result<Sell, String> {
    let form = &world.resource::<Desk>().form;
    let market = world.resource::<Market>();
    let title = form.title.trim();
    let summary = form.summary.trim();
    let category = form.category.trim();
    if title.is_empty() || title.chars().count() > TITLE {
        return Err(format!("A title is 1 to {TITLE} letters."));
    }
    if summary.is_empty() || summary.chars().count() > SUMMARY {
        return Err(format!("A summary is 1 to {SUMMARY} letters."));
    }
    if form.description.chars().count() > DESCRIPTION {
        return Err(format!(
            "A description is at most {DESCRIPTION} letters."
        ));
    }
    if category.is_empty() || category.chars().count() > CATEGORY {
        return Err(format!(
            "Choose a category of at most {CATEGORY} letters."
        ));
    }
    let price_cents = cents(&form.price)?;
    if publish
        && price_cents > 0
        && !market.user().is_some_and(|user| user.paid())
    {
        return Err("To sell for a price, set up payouts with \
                    Stripe first, or make it free."
            .into());
    }
    let wares = match form.upload {
        false => None,
        true => {
            if semver(form.version.trim()).is_none() {
                return Err("A version is three numbers, such as \
                            1.0.0."
                    .into());
            }
            let all = choices(world, form.kind, form.notes);
            let mut chosen = form
                .chosen
                .iter()
                .filter_map(|at| {
                    all.get(*at).map(|(_, wares)| wares.clone())
                })
                .collect::<Vec<_>>();
            let Some(first) = chosen.first().cloned() else {
                return Err("Choose what you sell.".into());
            };
            Some(match first {
                Wares::Tools(_) => Wares::Tools(
                    chosen
                        .drain(..)
                        .flat_map(|wares| match wares {
                            Wares::Tools(dirs) => dirs,
                            _ => Vec::new(),
                        })
                        .collect(),
                ),
                Wares::Hamlet { name, notes, .. } => Wares::Hamlet {
                    name,
                    kind: category.to_string(),
                    notes,
                },
                agent => agent,
            })
        }
    };
    if form.id.is_none() && wares.is_none() {
        return Err("Choose what you sell.".into());
    }
    let cover = form.cover.trim();
    Ok(Sell {
        id: form.id.clone(),
        draft: Draft {
            kind: Some(form.kind),
            title: title.to_string(),
            summary: summary.to_string(),
            description: form.description.trim().to_string(),
            category: category.to_string(),
            tags: tags(&form.tags)?,
            price_cents,
        },
        wares,
        version: form.version.trim().to_string(),
        changes: form.changes.trim().to_string(),
        cover: (!cover.is_empty()).then(|| expand(cover).into()),
        publish,
    })
}

/// `path` with a leading `~` as the chief's home folder.
fn expand(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

fn send(world: &mut World, publish: bool) {
    match offer(world, publish) {
        Ok(sell) => {
            world.resource_mut::<Desk>().form.problem = None;
            world.trigger(sell);
        }
        Err(why) => {
            world.resource_mut::<Desk>().form.problem = Some(why)
        }
    }
}

fn edit(world: &mut World, change: impl FnOnce(&mut Form)) {
    change(&mut world.resource_mut::<Desk>().form);
}

/// A line of the form: its name over a field tagged `field`.
fn field(
    name: &'static str,
    field: Field,
    hint: &'static str,
) -> AnyView {
    let value = resource::<Desk, _>(move |desk| {
        let mut form = desk.form.clone();
        field.of(&mut form).clone()
    });
    let mut parts = vec![
        text_field(value, move |world, typed| {
            edit(world, |form| *field.of(form) = typed)
        })
        .grow(1.0)
        .min_width(px(0.0))
        .rules(well)
        .tagged(field)
        .boxed(),
    ];
    if !hint.is_empty() {
        parts.push(note(hint).size(11.0).boxed());
    }
    part(name, column(parts).gap(4.0).boxed())
}

/// The form to put up or change a listing.
fn form() -> AnyView {
    let head = keyed(
        resource::<Desk, _>(|desk| {
            (desk.form.id.is_some(), desk.form.kind)
        }),
        |(editing, kind)| {
            let name = match (editing, kind) {
                (true, _) => "CHANGE A LISTING",
                (false, Kind::Agent) => "SELL AN AGENT",
                (false, Kind::Hamlet) => "SELL A HAMLET",
                (false, Kind::Plugin) => "SELL A PLUGIN",
            };
            page_head("BACK", close, Glyph::Coins, name).boxed()
        },
    )
    .within(bare());
    let kinds = keyed(
        resource::<Desk, _>(|desk| {
            (desk.form.kind, desk.form.id.is_some())
        }),
        |(chosen, editing)| {
            if *editing {
                return bare().boxed();
            }
            let all = Kind::ALL
                .into_iter()
                .map(|kind| {
                    chip(
                        kind_name(kind),
                        *chosen == kind,
                        move |world| {
                            let category = match kind {
                                Kind::Plugin => {
                                    plugins::Kind::ALL[0].name()
                                }
                                _ => Craft::ALL[0].name(),
                            };
                            edit(world, |form| {
                                form.kind = kind;
                                form.chosen = vec![0];
                                form.category = category.into();
                            })
                        },
                    )
                })
                .collect();
            part("What you sell", buttons(all).boxed())
        },
    )
    .within(bare());
    column((
        head,
        kinds,
        wares(),
        field(
            "Title",
            Field::Title,
            "What it is called, at most 60 letters.",
        ),
        field(
            "Summary",
            Field::Summary,
            "What it does, in a line of at most 140 letters.",
        ),
        field(
            "Description",
            Field::Description,
            "All a buyer should know.",
        ),
        category(),
        field(
            "Tags",
            Field::Tags,
            "Words it is found by, separated by commas.",
        ),
        price(),
        field(
            "Cover",
            Field::Cover,
            "The path of a PNG, JPEG or WebP picture, at most 2 MB. \
             Optional.",
        ),
        send_box(),
    ))
    .gap(14.0)
    .boxed()
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Agent => "Agent",
        Kind::Hamlet => "Hamlet",
        Kind::Plugin => "Plugin",
    }
}

/// The choice of what is packed as the listing's version.
fn wares() -> AnyView {
    let now = derived(|world: &World| {
        let form = &world.resource::<Desk>().form;
        let names = choices(world, form.kind, form.notes)
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        (
            form.kind,
            form.id.is_some(),
            form.upload,
            form.chosen.clone(),
            form.notes,
            names,
        )
    })
    .when(either(moved::<Desk>(), moved::<Market>()));
    keyed(now, |(kind, editing, upload, chosen, notes, names)| {
        let kind = *kind;
        let mut parts = Vec::new();
        if *editing {
            parts.push(
                row((
                    tick(*upload).on_change(|world, on| {
                        edit(world, |form| form.upload = on)
                    }),
                    label("Upload a new version").size(12.0),
                ))
                .gap(8.0)
                .align(AlignItems::Center)
                .boxed(),
            );
        }
        if !*upload {
            return column(parts).gap(8.0).boxed();
        }
        if names.is_empty() {
            parts.push(
                note(match kind {
                    Kind::Agent => "The realm has no agents to sell.",
                    Kind::Hamlet => {
                        "The realm has no hamlets to sell."
                    }
                    Kind::Plugin => {
                        "No agent has a tool of its own to sell: a \
                         folder with a manifest.json in the tools of \
                         its house."
                    }
                })
                .boxed(),
            );
            return column(parts).gap(8.0).boxed();
        } else if kind == Kind::Plugin {
            let all = names
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    let on = chosen.contains(&at);
                    row((
                        tick(on).on_change(move |world, on| {
                            edit(world, |form| {
                                form.chosen
                                    .retain(|kept| *kept != at);
                                if on {
                                    form.chosen.push(at);
                                    form.chosen.sort();
                                }
                            })
                        }),
                        label(name.clone()).size(12.0),
                    ))
                    .gap(8.0)
                    .align(AlignItems::Center)
                    .boxed()
                })
                .collect::<Vec<_>>();
            parts.push(part(
                "Its tools",
                column(all).gap(4.0).boxed(),
            ));
        } else {
            let at = chosen.first().copied().unwrap_or(0);
            parts.push(part(
                match kind {
                    Kind::Agent => "The agent",
                    _ => "The hamlet",
                },
                dropdown(
                    names.clone(),
                    at,
                    Glyph::Chevron.image(ICON),
                    |world, at| {
                        edit(world, |form| form.chosen = vec![at])
                    },
                )
                .min_width(px(220.0))
                .boxed(),
            ));
            parts.push(
                column((
                    row((
                        tick(*notes).on_change(|world, on| {
                            edit(world, |form| form.notes = on)
                        }),
                        label("Include notes").size(12.0),
                    ))
                    .gap(8.0)
                    .align(AlignItems::Center),
                    note(
                        "Notes are what an agent remembers. Read \
                         them before you sell: anything private in \
                         them goes to every buyer.",
                    )
                    .size(11.0),
                ))
                .gap(4.0)
                .boxed(),
            );
        }
        parts.push(
            row((
                column((field("Version", Field::Version, ""),))
                    .width(px(140.0))
                    .shrink(0.0),
                column((field("What changed", Field::Changes, ""),))
                    .grow(1.0)
                    .min_width(px(0.0)),
            ))
            .gap(10.0)
            .boxed(),
        );
        column(parts).gap(10.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// The choice of the shelf it sits on.
fn category() -> AnyView {
    keyed(
        resource::<Desk, _>(|desk| {
            (desk.form.kind, desk.form.category.clone())
        }),
        |(kind, chosen)| {
            let names: Vec<&'static str> = match kind {
                Kind::Agent => {
                    Craft::ALL.iter().map(|c| c.name()).collect()
                }
                Kind::Plugin => plugins::Kind::ALL
                    .iter()
                    .map(|k| k.name())
                    .collect(),
                Kind::Hamlet => {
                    return field(
                        "Kind of hamlet",
                        Field::Category,
                        "Such as Publishing house, in a few words.",
                    );
                }
            };
            let all = names
                .into_iter()
                .map(|name| {
                    chip(
                        name,
                        chosen.eq_ignore_ascii_case(name),
                        move |world| {
                            edit(world, |form| {
                                form.category = name.into()
                            })
                        },
                    )
                })
                .collect();
            part("Category", buttons(all).boxed())
        },
    )
    .within(bare())
    .boxed()
}

/// The price, what the seller is paid of it, and the way to be paid.
fn price() -> AnyView {
    let now = derived(|world: &World| {
        let form = &world.resource::<Desk>().form;
        let market = world.resource::<Market>();
        let payouts = market.user().and_then(|user| user.payouts);
        (cents(&form.price), payouts)
    })
    .when(either(moved::<Desk>(), moved::<Market>()));
    let state = keyed(now, |(cents, payouts)| {
        let words = match cents {
            Err(why) => {
                return label(why.clone())
                    .size(11.0)
                    .tone(Tone::Critical)
                    .boxed();
            }
            Ok(0) => {
                return note("Free. Leave it empty or 0.")
                    .size(11.0)
                    .boxed();
            }
            Ok(cents) => {
                let paid = *cents as f64 * f64::from(SELLERS_SHARE)
                    / 10_000.0;
                format!(
                    "You receive ${paid:.2} of each sale, \
                     {SELLERS_SHARE}%."
                )
            }
        };
        let enabled = payouts.is_some_and(|p| p.enabled);
        let mut parts = vec![note(words).size(11.0).boxed()];
        if !enabled {
            let started = payouts.is_some_and(|p| p.connected);
            parts.push(
                column((
                    label(match started {
                        true => {
                            "Stripe has not finished checking your \
                             account yet."
                        }
                        false => {
                            "To be paid, set up payouts with Stripe."
                        }
                    })
                    .size(12.0)
                    .tone(Tone::Critical),
                    buttons(vec![
                        good_action(
                            Glyph::Coins,
                            "SET UP PAYOUTS",
                            |world| world.trigger(SetUpPayouts),
                        ),
                        action(
                            Glyph::Restore,
                            "CHECK AGAIN",
                            |world| world.trigger(AskAccount),
                        ),
                    ]),
                ))
                .gap(6.0)
                .boxed(),
            );
        }
        column(parts).gap(6.0).boxed()
    })
    .within(bare());
    column((field("Price in dollars", Field::Price, ""), state))
        .gap(4.0)
        .boxed()
}

/// The buttons that send the form, and how sending goes.
fn send_box() -> AnyView {
    let now = derived(|world: &World| {
        let desk = world.resource::<Desk>();
        let market = world.resource::<Market>();
        (
            desk.form.problem.clone(),
            market.selling.clone(),
            desk.form.id.is_some(),
        )
    })
    .when(either(moved::<Desk>(), moved::<Market>()));
    keyed(now, |(problem, selling, editing)| {
        let mut parts = vec![];
        match selling {
            Some(Selling::Going(doing)) => {
                parts.push(
                    stated(
                        Glyph::Clock,
                        format!("{doing}..."),
                        Tone::Accent,
                        12.0,
                    )
                    .boxed(),
                );
            }
            _ => parts.push(
                buttons(vec![
                    good_action(
                        Glyph::Coins,
                        "PUT UP FOR SALE",
                        |world| send(world, true),
                    ),
                    action(
                        Glyph::Notes,
                        if *editing {
                            "SAVE"
                        } else {
                            "SAVE AS DRAFT"
                        },
                        |world| send(world, false),
                    ),
                ])
                .boxed(),
            ),
        }
        if let Some(why) = problem {
            parts.push(
                label(why.clone())
                    .size(12.0)
                    .tone(Tone::Critical)
                    .boxed(),
            );
        }
        match selling {
            Some(Selling::Done(title, status)) => {
                let words = match status {
                    Status::Listed => {
                        format!("{title} is on the shelves.")
                    }
                    _ => format!(
                        "{title} is saved. It is not on the shelves \
                         yet."
                    ),
                };
                parts.push(
                    buttons(vec![
                        label(words)
                            .size(12.0)
                            .tone(Tone::Accent)
                            .boxed(),
                        action(Glyph::Book, "SEE YOURS", open_yours),
                    ])
                    .boxed(),
                );
            }
            Some(Selling::Failed(why)) => {
                parts.push(
                    label(why.clone())
                        .size(12.0)
                        .tone(Tone::Critical)
                        .boxed(),
                );
            }
            _ => {}
        }
        column(parts).gap(8.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// What the chief sells and has bought.
fn yours() -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        (
            market.mine.clone(),
            market.library.clone(),
            market.user().is_some(),
        )
    })
    .when(moved::<Market>());
    let body = keyed(now, |(mine, library, signed)| {
        if !*signed {
            return note(
                "Sign in with GitHub to see what you sell and own.",
            )
            .boxed();
        }
        let mut selling =
            mine.iter().map(sold_row).collect::<Vec<_>>();
        if selling.is_empty() {
            selling.push(note("You sell nothing yet.").boxed());
        }
        let mut bought = library
            .iter()
            .map(|owned| {
                let lot = &owned.listing;
                column((
                    row((
                        label(lot.title.clone())
                            .size(13.0)
                            .bold(true),
                        note(kind_name(lot.kind)).size(11.0),
                    ))
                    .gap(8.0)
                    .align(AlignItems::Center),
                    trade::deal(
                        lot.kind,
                        lot.id.clone(),
                        lot.price(),
                        &lot.seller.seller(),
                    ),
                ))
                .gap(6.0)
                .boxed()
            })
            .collect::<Vec<_>>();
        if bought.is_empty() {
            bought.push(note("You have bought nothing yet.").boxed());
        }
        column((
            part(
                "For sale by you",
                column(selling).gap(10.0).boxed(),
            ),
            part("Bought", column(bought).gap(12.0).boxed()),
        ))
        .gap(18.0)
        .boxed()
    })
    .within(bare());
    column((page_head("BACK", close, Glyph::Book, "YOURS"), body))
        .gap(16.0)
        .boxed()
}

fn status_words(status: Status) -> &'static str {
    match status {
        Status::Draft => "Draft",
        Status::Listed => "On the shelves",
        Status::Unlisted => "Off the shelves",
        Status::Removed => "Taken down by the bazaar",
    }
}

/// One listing the chief sells, with the ways to change it.
fn sold_row(lot: &Lot) -> AnyView {
    let id = lot.id.clone();
    let mut acts = vec![action(Glyph::Rename, "EDIT", {
        let id = id.clone();
        move |world| open_edit(world, &id)
    })];
    match lot.status {
        Status::Listed => {
            acts.push(action(Glyph::Lower, "UNLIST", {
                let id = id.clone();
                move |world| {
                    world.trigger(Shelve {
                        id: id.clone(),
                        on: false,
                    })
                }
            }))
        }
        Status::Draft | Status::Unlisted => {
            acts.push(good_action(Glyph::Coins, "LIST", {
                let id = id.clone();
                move |world| {
                    world.trigger(Shelve {
                        id: id.clone(),
                        on: true,
                    })
                }
            }))
        }
        Status::Removed => {}
    }
    let version = match lot.version() {
        "" => "no version yet".to_string(),
        version => format!("version {version}"),
    };
    column((
        row((
            label(lot.title.clone()).size(13.0).bold(true),
            note(kind_name(lot.kind)).size(11.0),
        ))
        .gap(8.0)
        .align(AlignItems::Center)
        .tagged(Wraps),
        note(format!(
            "{}  {}  {}  {} own it",
            status_words(lot.status),
            lot.price().words(),
            version,
            lot.owners
        ))
        .size(11.0),
        buttons(acts),
    ))
    .gap(4.0)
    .padding(UiRect::vertical(px(2.0)))
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_and_tags_are_read_as_the_bazaar_takes_them() {
        assert_eq!(cents(""), Ok(0));
        assert_eq!(cents("$4.99"), Ok(499));
        assert_eq!(cents("0"), Ok(0));
        assert!(cents("0.10").is_err());
        assert!(cents("501").is_err());
        assert!(cents("abc").is_err());
        assert_eq!(
            tags(" Code, review,code ,, ").unwrap(),
            ["code", "review"]
        );
        assert!(tags("a,b,c,d,e,f,g,h,i").is_err());
    }
}

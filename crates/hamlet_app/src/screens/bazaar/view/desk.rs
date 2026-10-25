//! The seller's desk, laid out as Settings lays out its pages: a
//! form to put an agent, a hamlet or tools up for sale, or to change
//! a listing, its cover and upload a new version of it, and the page
//! of what the chief sells and has bought, listed as the Inspector
//! lists its hamlets.

use bevy::ecs::name::Name;
use bevy::ecs::query::Without;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, label, row, scroll,
    segmented, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::{AgentNumber, HamletId, Left, LivesIn};
use hamlet_protocol::client::{Address, PackableTool, Packing};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, ICON, action, bare, buttons, command, filling, headed,
    note, text_input,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::widgets::switch::switch as toggle;

use super::covers::{chosen, now_of};
use super::parts::{
    Line, crumb, mark as accent, nothing, outcome, page_head,
    problem, section, sections, wide,
};
use super::trade;
use super::wares::{open_lot, price as price_of};
use crate::bazaar::covers::{
    MOST_BYTES, MOST_SIDE, Pick, Picked, pick,
};
use crate::bazaar::listings::SELLERS_SHARE;
use crate::bazaar::market::deal::hamlets_of;
use crate::bazaar::market::lot::{
    Draft, Kind, Lot, Payouts, Status, next_version, price_words,
    semver,
};
use crate::bazaar::market::sell::Step;
use crate::bazaar::market::{
    self, Housed, Market, Offer, Selling, ask_account, ask_tools,
    set_up_payouts,
};
use crate::bazaar::samples::agents::Craft;
use crate::bazaar::samples::plugins::Kind as Shelf;
use crate::roll::{either, hamlets_moved, moved};

/// The longest a title, a summary, a description and a category may
/// be, in letters, and the most tags and letters of each.
const TITLE: usize = 60;
const SUMMARY: usize = 140;
const DESCRIPTION: usize = 5000;
const CATEGORY: usize = 32;
const TAGS: usize = 8;
const TAG: usize = 24;

/// The least and most a paid listing may cost, in cents.
const LEAST_CENTS: u32 = 50;
const MOST_CENTS: u32 = 50_000;

/// What the chief is doing at the seller's desk.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Desk {
    /// The desk's page open in place of the stall's, if one is.
    pub page: Option<Page>,
    pub form: Form,
}

/// A page of the desk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Page {
    Form,
    Yours,
}

/// The listing being put up or changed.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Form {
    /// The listing changed; `None` for a new one.
    pub id: Option<String>,
    pub kind: Kind,
    pub title: String,
    pub summary: String,
    pub description: String,
    pub category: String,
    /// Separated by commas.
    pub tags: String,
    /// In dollars, empty or `0` for free.
    pub price: String,
    /// A new version is uploaded with it.
    pub upload: bool,
    /// What is sold, by its place among the choices of its kind.
    pub chosen: Vec<usize>,
    pub notes: bool,
    pub version: String,
    pub changes: String,
    /// Why the form cannot be sent as it is.
    pub problem: Option<String>,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            id: None,
            kind: Kind::Agent,
            title: String::new(),
            summary: String::new(),
            description: String::new(),
            category: first_category(Kind::Agent).into(),
            tags: String::new(),
            price: String::new(),
            upload: true,
            chosen: vec![0],
            notes: false,
            version: "1.0.0".into(),
            changes: String::new(),
            problem: None,
        }
    }
}

/// A field of the form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Title,
    Summary,
    Description,
    Category,
    Tags,
    Price,
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
            Field::Version => &mut form.version,
            Field::Changes => &mut form.changes,
        }
    }
}

/// The first category of a listing of `kind`.
fn first_category(kind: Kind) -> &'static str {
    match kind {
        Kind::Agent => Craft::ALL[0].name(),
        Kind::Plugin => Shelf::ALL[0].name(),
        Kind::Hamlet => "",
    }
}

/// Opens the form to put up a new listing of `kind`, or `lot` to
/// change it.
pub(crate) fn open_form(
    world: &mut World,
    kind: Kind,
    lot: Option<&Lot>,
) {
    let kind = lot.map_or(kind, |lot| lot.kind);
    let form = match lot {
        None => Form {
            kind,
            category: first_category(kind).into(),
            ..Form::default()
        },
        Some(lot) => Form {
            id: Some(lot.id.clone()),
            kind,
            title: lot.title.clone(),
            summary: lot.summary.clone(),
            description: lot.description.clone(),
            category: lot.category.clone(),
            tags: lot.tags.join(", "),
            price: match lot.price_cents {
                0 => String::new(),
                cents => {
                    format!("{}.{:02}", cents / 100, cents % 100)
                }
            },
            upload: lot.latest.is_none(),
            version: next_version(lot.version()),
            ..Form::default()
        },
    };
    let mut desk = world.resource_mut::<Desk>();
    desk.form = form;
    desk.page = Some(Page::Form);
    world.resource_mut::<Selling>().0 = None;
    *world.resource_mut::<Pick>() = Pick::default();
    if kind == Kind::Plugin {
        ask_tools(world);
    }
}

/// Opens the form to change the listing `id` of the chief's.
pub(crate) fn open_edit(world: &mut World, id: &str) {
    let lot = world.resource::<Market>().lot(id).cloned();
    if let Some(lot) = lot {
        open_form(world, lot.kind, Some(&lot));
    }
}

/// Opens the page of what the chief sells and has bought.
pub(crate) fn open_yours(world: &mut World) {
    world.resource_mut::<Desk>().page = Some(Page::Yours);
    ask_account(world);
}

fn close(world: &mut World) {
    world.resource_mut::<Desk>().page = None;
}

/// The desk's page open, if one is, else the stall's page `stall`
/// makes. `back` names the stall on the way back to it.
pub(crate) fn or(
    back: &'static str,
    stall: impl Fn() -> AnyView + Send + Sync + 'static,
) -> AnyView {
    let page = resource::<Desk, _>(|desk| desk.page);
    keyed(page, move |page| match page {
        Some(Page::Form) => form(back),
        Some(Page::Yours) => yours(back),
        None => stall(),
    })
    .within(filling())
    .boxed()
}

/// A living agent: its name, its hamlet and the name it is shown by.
pub(crate) type Living = (String, String, String);

/// The living agents of the realm, by hamlet then name.
fn agents_of(world: &World) -> Vec<Living> {
    let hamlets = hamlets_of(world);
    let Some(mut query) = world
        .try_query_filtered::<(&Name, &LivesIn), (
            bevy::ecs::query::With<AgentNumber>,
            Without<Left>,
        )>()
    else {
        return Vec::new();
    };
    let mut all = query
        .iter(world)
        .filter_map(|(name, lives)| {
            let hamlet = world.get::<HamletId>(lives.0)?.0.clone();
            let shown = hamlets
                .iter()
                .find(|(id, _)| *id == hamlet)
                .map_or_else(
                    || hamlet.clone(),
                    |(_, shown)| shown.clone(),
                );
            Some((name.as_str().to_string(), hamlet, shown))
        })
        .collect::<Vec<_>>();
    let order = |hamlet: &str| {
        hamlets.iter().position(|(id, _)| id == hamlet)
    };
    all.sort_by(|a, b| (order(&a.1), &a.0).cmp(&(order(&b.1), &b.0)));
    all
}

/// What can be sold as a listing of `kind`, by name: the living
/// `agents`, the `hamlets` by id and shown name, and the `tools`.
pub(crate) fn choices(
    agents: &[Living],
    hamlets: &[(String, String)],
    tools: &[PackableTool],
    kind: Kind,
) -> Vec<(String, Packing)> {
    match kind {
        Kind::Agent => agents
            .iter()
            .map(|(name, hamlet, shown)| {
                (
                    format!("{name} of {shown}"),
                    Packing::Agent {
                        agent: Address {
                            agent: name.clone(),
                            hamlet: hamlet.clone(),
                        },
                        notes: false,
                    },
                )
            })
            .collect(),
        Kind::Hamlet => hamlets
            .iter()
            .map(|(id, shown)| {
                (
                    shown.clone(),
                    Packing::Hamlet {
                        hamlet: id.clone(),
                        kind: String::new(),
                        notes: false,
                    },
                )
            })
            .collect(),
        Kind::Plugin => tools
            .iter()
            .map(|tool| {
                (
                    tool.name.clone(),
                    Packing::Tools {
                        tools: vec![tool.name.clone()],
                    },
                )
            })
            .collect(),
    }
}

/// The choices of the form's kind as the world stands.
fn choices_now(world: &World, kind: Kind) -> Vec<(String, Packing)> {
    let tools = match &world.resource::<Housed>().tools {
        Some(Ok(tools)) => tools.clone(),
        _ => Vec::new(),
    };
    choices(&agents_of(world), &hamlets_of(world), &tools, kind)
}

/// The price typed, in cents, or why it is not one.
pub(crate) fn cents(price: &str) -> Result<u32, String> {
    let price = price.trim().trim_start_matches('$');
    if price.is_empty() {
        return Ok(0);
    }
    let dollars = price
        .parse::<f64>()
        .ok()
        .filter(|dollars| dollars.is_finite() && *dollars >= 0.0)
        .ok_or(
            "Write the price as a number of dollars, such as 4.99.",
        )?;
    match (dollars * 100.0).round() as u32 {
        0 => Ok(0),
        cents if (LEAST_CENTS..=MOST_CENTS).contains(&cents) => {
            Ok(cents)
        }
        _ => {
            Err("Make it free, or price it from $0.50 to $500."
                .into())
        }
    }
}

/// The tags typed, or why they are not.
pub(crate) fn tags(typed: &str) -> Result<Vec<String>, String> {
    let mut all = Vec::<String>::new();
    for tag in typed.split(',') {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() || all.contains(&tag) {
            continue;
        }
        if tag.chars().count() > TAG {
            return Err(format!(
                "Shorten each tag to at most {TAG} letters."
            ));
        }
        all.push(tag);
    }
    if all.len() > TAGS {
        return Err(format!("Give it at most {TAGS} tags."));
    }
    Ok(all)
}

/// What `form` packs among `choices`, or why there is nothing.
fn wares(
    form: &Form,
    choices: &[(String, Packing)],
    category: &str,
) -> Result<Packing, String> {
    let mut chosen = form
        .chosen
        .iter()
        .filter_map(|at| choices.get(*at))
        .map(|(_, wares)| wares.clone());
    let notes = form.notes;
    match chosen.next() {
        None => Err("Choose what you sell.".into()),
        Some(Packing::Tools { mut tools }) => {
            for more in chosen {
                if let Packing::Tools { tools: more } = more {
                    tools.extend(more);
                }
            }
            Ok(Packing::Tools { tools })
        }
        Some(Packing::Hamlet { hamlet, .. }) => Ok(Packing::Hamlet {
            hamlet,
            kind: category.to_string(),
            notes,
        }),
        Some(Packing::Agent { agent, .. }) => {
            Ok(Packing::Agent { agent, notes })
        }
    }
}

/// What `form` sends, its wares among `choices`, or why it cannot be
/// sent.
pub(crate) fn offer(
    form: &Form,
    choices: &[(String, Packing)],
    payouts: Option<Payouts>,
    publish: bool,
) -> Result<Offer, String> {
    let paid = payouts.is_some_and(|payouts| payouts.enabled);
    let title = form.title.trim();
    let summary = form.summary.trim();
    let category = form.category.trim();
    if title.is_empty() || title.chars().count() > TITLE {
        return Err(format!(
            "Give it a title of at most {TITLE} letters."
        ));
    }
    if summary.is_empty() || summary.chars().count() > SUMMARY {
        return Err(format!(
            "Write a summary, a line of at most {SUMMARY} letters."
        ));
    }
    if form.description.chars().count() > DESCRIPTION {
        return Err(format!(
            "Shorten the description to at most {DESCRIPTION} \
             letters."
        ));
    }
    if category.is_empty() || category.chars().count() > CATEGORY {
        return Err(match form.kind {
            Kind::Hamlet => format!(
                "Say what kind of hamlet it is, in at most \
                 {CATEGORY} letters."
            ),
            _ => "Choose a category.".into(),
        });
    }
    let price_cents = cents(&form.price)?;
    if publish && price_cents > 0 && !paid {
        return Err("To sell for a price, set up payouts with \
                    Stripe first, or make it free."
            .into());
    }
    let wares = match form.upload {
        false => None,
        true if semver(form.version.trim()).is_none() => {
            return Err("Write the version as three numbers, such \
                        as 1.0.0."
                .into());
        }
        true => Some(wares(form, choices, category)?),
    };
    if form.id.is_none() && wares.is_none() {
        return Err("Choose what you sell.".into());
    }
    Ok(Offer {
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
        cover: None,
        publish,
    })
}

fn send(world: &mut World, publish: bool) {
    let payouts = world
        .resource::<Market>()
        .user()
        .and_then(|user| user.payouts);
    let form = world.resource::<Desk>().form.clone();
    let choices = choices_now(world, form.kind);
    let cover = match &world.resource::<Pick>().state {
        Picked::Nothing => Ok::<_, String>(None),
        Picked::Got(picture) => Ok(Some((
            picture.bytes.clone(),
            picture.media.to_string(),
        ))),
        Picked::Fetching => Err("Wait for the cover to come, or \
                                 clear where it is."
            .into()),
        Picked::Failed(_) => Err("The cover could not be had: \
                                  clear where it is, or give \
                                  another."
            .into()),
    };
    let offered =
        offer(&form, &choices, payouts, publish).and_then(|offer| {
            Ok(Offer {
                cover: cover?,
                ..offer
            })
        });
    match offered {
        Ok(offer) => {
            world.resource_mut::<Desk>().form.problem = None;
            market::sell(world, offer);
        }
        Err(why) => {
            world.resource_mut::<Desk>().form.problem = Some(why);
        }
    }
}

fn edit(world: &mut World, change: impl FnOnce(&mut Form)) {
    change(&mut world.resource_mut::<Desk>().form);
}

/// A setting of the form, as Settings has one: its name over it, and
/// what goes in it, dim, under it.
fn setting(
    name: &'static str,
    control: AnyView,
    hint: Option<String>,
) -> AnyView {
    let mut parts = vec![
        label(name)
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText)
            .boxed(),
        control,
    ];
    parts.extend(hint.map(|hint| note(hint).boxed()));
    column(parts)
        .gap(6.0)
        .align(AlignItems::Stretch)
        .width(percent(100.0))
        .boxed()
}

/// A field of the form: its name, the words in it and its hint.
fn line(
    name: &'static str,
    which: Field,
    hint: &'static str,
) -> AnyView {
    let value = resource::<Desk, _>(move |desk| {
        let mut form = desk.form.clone();
        which.of(&mut form).clone()
    });
    let typed = text_field(value, move |world, typed| {
        edit(world, |form| *which.of(form) = typed);
    })
    .rules(text_input)
    .boxed();
    setting(name, typed, (!hint.is_empty()).then(|| hint.to_string()))
}

/// A part of the form under its head, as Settings heads the parts of
/// a page: its icon and its name, over `settings`.
fn part_of(
    which: Glyph,
    title: &'static str,
    settings: Vec<AnyView>,
) -> AnyView {
    let mut all = vec![
        headed(
            which,
            label(title)
                .size(12.0)
                .bold(true)
                .tone(Tone::Faint)
                .tagged(DisplayText),
        )
        .boxed(),
    ];
    all.extend(settings);
    column(all).gap(12.0).boxed()
}

/// A page of the desk: the way back to the stall `back`, then `body`
/// in a scroll.
fn desk_page(
    back: &'static str,
    here: &str,
    body: AnyView,
) -> AnyView {
    column((
        crumb(back, close, here.to_string()),
        scroll((body,)).fills_height(),
    ))
    .gap(14.0)
    .fills_height()
    .boxed()
}

/// The form to put up or change a listing, as Settings lays out a
/// page: its head, its parts each under a section name, and the
/// buttons that send it at its end.
fn form(back: &'static str) -> AnyView {
    let now = derived(|world: &World| {
        let form = &world.resource::<Desk>().form;
        let changed = form.id.as_ref().map(|id| {
            world
                .resource::<Market>()
                .lot(id)
                .map_or(String::new(), |lot| lot.title.clone())
        });
        (changed, form.kind)
    })
    .when(either(moved::<Desk>(), moved::<Market>()));
    keyed(now, move |(changed, kind)| {
        let editing = changed.is_some();
        let (here, head, about) = match (changed, kind) {
            (Some(title), _) => (
                "CHANGE",
                format!("Change {title}"),
                "Change how it is listed, or upload a new version \
                 of it.",
            ),
            (None, kind) => {
                let head = match kind {
                    Kind::Agent => "Sell an agent",
                    Kind::Hamlet => "Sell a hamlet",
                    Kind::Plugin => "Sell tools",
                };
                (
                    "SELL",
                    head.to_string(),
                    "Put up what you made, for other chiefs to buy \
                     at the bazaar.",
                )
            }
        };
        let mut parts = vec![
            page_head(
                &head,
                note(about).boxed(),
                String::new(),
                None,
            ),
            part_of(
                Glyph::Lower,
                "WHAT YOU SELL",
                vec![kinds(), sold()],
            ),
            part_of(
                Glyph::Rename,
                "HOW IT IS LISTED",
                vec![
                    line(
                        "Title",
                        Field::Title,
                        "What it is called, in at most 60 letters.",
                    ),
                    line(
                        "Summary",
                        Field::Summary,
                        "What it does, in one line of at most 140 \
                         letters, on its row.",
                    ),
                    line(
                        "Description",
                        Field::Description,
                        "All a buyer should know, on its page under \
                         the summary.",
                    ),
                    category(),
                    line(
                        "Tags",
                        Field::Tags,
                        "Words it is found by, separated by commas, \
                         at most 8.",
                    ),
                ],
            ),
            part_of(Glyph::Notes, "COVER", vec![cover()]),
            part_of(Glyph::Coins, "PRICE", vec![price()]),
        ];
        if editing {
            parts.push(part_of(
                Glyph::Book,
                "ON THE SHELVES",
                vec![shelves()],
            ));
        }
        parts.push(footer());
        parts.push(trade::account("to put it up."));
        desk_page(back, here, sections(parts))
    })
    .within(filling())
    .boxed()
}

/// The kind of listing put up, while it is a new one.
fn kinds() -> AnyView {
    keyed(
        resource::<Desk, _>(|desk| desk.form.id.is_some()),
        |editing| {
            if *editing {
                return nothing();
            }
            let all = [Kind::Agent, Kind::Hamlet, Kind::Plugin];
            let chosen = resource::<Desk, _>(move |desk| {
                all.iter()
                    .position(|kind| *kind == desk.form.kind)
                    .unwrap_or(0)
            });
            let pick = segmented(
                ["AN AGENT", "A HAMLET", "TOOLS"],
                chosen,
                move |world, at| {
                    let kind = all[at];
                    if kind == Kind::Plugin {
                        ask_tools(world);
                    }
                    edit(world, |form| {
                        form.kind = kind;
                        form.chosen = vec![0];
                        form.category = first_category(kind).into();
                    });
                },
            );
            setting("Kind", pick.boxed(), None)
        },
    )
    .within(bare())
    .boxed()
}

/// What the choice of what is sold is drawn from.
#[derive(Clone, PartialEq)]
struct Choosing {
    kind: Kind,
    editing: bool,
    upload: bool,
    names: Vec<String>,
    chosen: Vec<usize>,
    notes: bool,
    /// Why the tools could not be had, for tools.
    problem: Option<String>,
}

/// A switch with its words beside it.
fn switch(
    words: impl Into<String>,
    on: bool,
    set: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> AnyView {
    row((toggle(on, set), label(words.into()).size(13.0)))
        .gap(8.0)
        .align(AlignItems::Center)
        .boxed()
}

/// The choice of what is packed as the listing's version.
fn sold() -> AnyView {
    let now = derived(|world: &World| {
        let form = &world.resource::<Desk>().form;
        let problem =
            match (&world.resource::<Housed>().tools, form.kind) {
                (Some(Err(why)), Kind::Plugin) => Some(why.clone()),
                _ => None,
            };
        Choosing {
            kind: form.kind,
            editing: form.id.is_some(),
            upload: form.upload,
            names: choices_now(world, form.kind)
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            chosen: form.chosen.clone(),
            notes: form.notes,
            problem,
        }
    })
    .when(either(
        either(moved::<Desk>(), hamlets_moved()),
        moved::<Housed>(),
    ));
    keyed(now, |now| {
        let mut parts = Vec::new();
        if now.editing {
            parts.push(switch(
                "Upload a new version",
                now.upload,
                |world, on| {
                    edit(world, |form| form.upload = on);
                },
            ));
        }
        if !now.upload {
            return column(parts).gap(12.0).boxed();
        }
        if now.names.is_empty() {
            parts.push(match (now.kind, &now.problem) {
                (_, Some(why)) => problem(format!(
                    "The tools could not be had: {why}"
                )),
                (Kind::Agent, _) => {
                    note("The realm has no agents to sell.").boxed()
                }
                (Kind::Hamlet, _) => {
                    note("The realm has no hamlets to sell.").boxed()
                }
                (Kind::Plugin, _) => note(
                    "No tool added by hand can be sold: a folder \
                     with a manifest.json in the home's tools, its \
                     name in lowercase letters, digits, - and _.",
                )
                .boxed(),
            });
            return column(parts).gap(12.0).boxed();
        }
        match now.kind {
            Kind::Plugin => {
                let tools = now
                    .names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| {
                        switch(
                            name.clone(),
                            now.chosen.contains(&at),
                            move |world, on| {
                                edit(world, |form| {
                                    form.chosen
                                        .retain(|kept| *kept != at);
                                    if on {
                                        form.chosen.push(at);
                                        form.chosen.sort();
                                    }
                                });
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                parts.push(setting(
                    "The tools",
                    column(tools).gap(6.0).boxed(),
                    None,
                ));
            }
            kind => {
                let at = now.chosen.first().copied().unwrap_or(0);
                parts.push(setting(
                    match kind {
                        Kind::Agent => "The agent",
                        _ => "The hamlet",
                    },
                    wide(
                        dropdown(
                            now.names.clone(),
                            at,
                            Glyph::Chevron.image(ICON),
                            |world, at| {
                                edit(world, |form| {
                                    form.chosen = vec![at];
                                });
                            },
                        )
                        .width(percent(100.0))
                        .max_width(percent(100.0)),
                    ),
                    None,
                ));
                parts.push(setting(
                    "Its notes",
                    switch(
                        "Include its notes",
                        now.notes,
                        |world, on| {
                            edit(world, |form| form.notes = on);
                        },
                    ),
                    Some(
                        "Notes are what an agent remembers. Read \
                         them before you sell: anything private in \
                         them goes to every buyer."
                            .into(),
                    ),
                ));
            }
        }
        parts.push(line(
            "Version",
            Field::Version,
            "Three numbers, such as 1.0.0, higher than the last.",
        ));
        parts.push(line(
            "What changed",
            Field::Changes,
            "What is new in it, said beside the version on its page.",
        ));
        column(parts).gap(12.0).width(percent(100.0)).boxed()
    })
    .within(bare().width(percent(100.0)))
    .boxed()
}

/// What the desk shows of the cover.
#[derive(Clone, PartialEq, Debug)]
enum Covering {
    None,
    /// The listing changed keeps the cover it has.
    Kept,
    Fetching,
    /// A picture came, of this size.
    Got(String),
    Failed(String),
}

/// The cover chosen by the file or the address of a picture, shown as
/// it will be.
fn cover() -> AnyView {
    let from = text_field(
        resource::<Pick, _>(|pick| pick.from.clone()),
        pick,
    )
    .rules(text_input)
    .boxed();
    let hint = format!(
        "The path of a picture on this computer, or its address on \
         the web: a PNG, JPEG or WebP of at most {} KB and \
         {MOST_SIDE} pixels a side; 640 by 360 pixels shows best.",
        MOST_BYTES / 1024
    );
    let now = derived(|world: &World| {
        match &world.resource::<Pick>().state {
            Picked::Nothing if now_of(world).is_some() => {
                Covering::Kept
            }
            Picked::Nothing => Covering::None,
            Picked::Fetching => Covering::Fetching,
            Picked::Got(picture) => Covering::Got(format!(
                "{} by {} pixels, {} KB.",
                picture.size.x,
                picture.size.y,
                picture.bytes.len().div_ceil(1024)
            )),
            Picked::Failed(why) => Covering::Failed(why.clone()),
        }
    })
    .when(either(
        either(moved::<Pick>(), moved::<Desk>()),
        moved::<Market>(),
    ));
    let state = keyed(now, |now| match now {
        Covering::None => nothing(),
        Covering::Kept => column((
            chosen(),
            note("Its cover now. Give another picture to change it."),
        ))
        .gap(6.0)
        .boxed(),
        Covering::Fetching => note("Fetching it...").boxed(),
        Covering::Got(size) => column((
            chosen(),
            row((
                column((note(size.clone()),)).fills_width(),
                command("CLEAR", |world| pick(world, String::new())),
            ))
            .gap(12.0)
            .align(AlignItems::Center),
        ))
        .gap(6.0)
        .boxed(),
        Covering::Failed(why) => {
            problem(format!("{why} Give another picture."))
        }
    })
    .within(bare());
    column((setting("Picture", from, Some(hint)), state.boxed()))
        .gap(12.0)
        .boxed()
}

/// The choice of the shelf it sits on, or the kind of hamlet it is.
fn category() -> AnyView {
    keyed(resource::<Desk, _>(|desk| desk.form.kind), |kind| {
        let names = match kind {
            Kind::Agent => Craft::ALL.map(Craft::name).to_vec(),
            Kind::Plugin => Shelf::ALL.map(Shelf::name).to_vec(),
            Kind::Hamlet => {
                return line(
                    "Kind of hamlet",
                    Field::Category,
                    "What kind of hamlet it is, in a few words, at \
                     most 32 letters.",
                );
            }
        };
        let shown = names.clone();
        let at = resource::<Desk, _>(move |desk| {
            shown
                .iter()
                .position(|name| {
                    desk.form.category.eq_ignore_ascii_case(name)
                })
                .unwrap_or(0)
        });
        let pick = dropdown(
            names.iter().map(ToString::to_string).collect::<Vec<_>>(),
            at,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                let name = names[at];
                edit(world, |form| form.category = name.into());
            },
        )
        .width(percent(100.0))
        .max_width(percent(100.0));
        setting(
            "Category",
            wide(pick),
            Some("The shelf it is found on.".into()),
        )
    })
    .within(bare().width(percent(100.0)))
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
    let state =
        keyed(now, |(cents, payouts)| payout_words(cents, *payouts))
            .within(bare());
    column((
        line(
            "Price in dollars",
            Field::Price,
            "Such as 4.99. Leave it empty, or 0, to give it for \
             free.",
        ),
        state.boxed(),
    ))
    .gap(12.0)
    .boxed()
}

fn payout_words(
    cents: &Result<u32, String>,
    payouts: Option<Payouts>,
) -> AnyView {
    let words = match cents {
        Err(why) => return problem(why.clone()),
        Ok(0) => return nothing(),
        Ok(cents) => format!(
            "You receive {} of each sale, {SELLERS_SHARE}%.",
            price_words(cents * SELLERS_SHARE / 100)
        ),
    };
    let mut parts = vec![note(words).boxed()];
    if !payouts.is_some_and(|payouts| payouts.enabled) {
        let started =
            payouts.is_some_and(|payouts| payouts.connected);
        parts.push(
            note(match started {
                true => {
                    "Stripe has not finished checking your account \
                     yet. Check again once it has."
                }
                false => {
                    "To be paid, set up payouts with Stripe, or make \
                     it free."
                }
            })
            .boxed(),
        );
        parts.push(
            buttons(vec![
                action(
                    Glyph::Coins,
                    "SET UP PAYOUTS",
                    set_up_payouts,
                ),
                command("CHECK AGAIN", ask_account).boxed(),
            ])
            .boxed(),
        );
    }
    column(parts).gap(6.0).boxed()
}

/// How the listing changed stands on the shelves, and the way to take
/// it off them.
fn shelves() -> AnyView {
    let now = derived(|world: &World| {
        let id = world.resource::<Desk>().form.id.clone()?;
        let lot = world.resource::<Market>().lot(&id)?;
        Some((id, lot.status, status_words(lot)))
    })
    .when(either(moved::<Desk>(), moved::<Market>()));
    keyed(now, |now| {
        let Some((id, status, words)) = now.clone() else {
            return nothing();
        };
        let mut parts = vec![note(words).boxed()];
        if status == Status::Listed {
            parts.push(
                row((action(
                    Glyph::Cross,
                    "TAKE IT OFF THE SHELVES",
                    move |world| market::shelve(world, &id, false),
                ),))
                .boxed(),
            );
        }
        column(parts).gap(6.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// Where `lot` the chief sells stands, in a few words.
fn status_words(lot: &Lot) -> String {
    let version = match lot.version() {
        "" => "no version yet".to_string(),
        version => format!("version {version}"),
    };
    format!(
        "{}, {version}, {} own it.",
        lot.status.words(),
        lot.owners
    )
}

/// The end of the form, as Settings has its footer: what sending does
/// over the buttons that send it, then how sending goes and what came
/// of it.
fn footer() -> AnyView {
    let now = derived(|world: &World| {
        let desk = world.resource::<Desk>();
        let step = world
            .resource::<Selling>()
            .0
            .as_ref()
            .map(|sale| sale.step.clone());
        (desk.form.problem.clone(), step, desk.form.id.is_some())
    })
    .when(either(moved::<Desk>(), moved::<Selling>()));
    keyed(now, |(problem_now, step, editing)| {
        let going = match step {
            Some(Step::Packing(_)) => Some("Packing it..."),
            Some(Step::Saving) => Some("Sending it to the bazaar..."),
            Some(Step::Covering) => Some("Sending its cover..."),
            Some(Step::Uploading) => Some("Uploading it..."),
            Some(Step::Publishing) => {
                Some("Putting it on the shelves...")
            }
            _ => None,
        };
        let mut pressed = Vec::new();
        let mut parts = Vec::new();
        match going {
            Some(going) => parts.push(note(going).boxed()),
            None => {
                parts.push(
                    note(
                        "A draft is kept at the bazaar, off the \
                         shelves, until you list it.",
                    )
                    .boxed(),
                );
                pressed.push(action(
                    Glyph::Coins,
                    "PUT UP FOR SALE",
                    |world| send(world, true),
                ));
                pressed.push(action(
                    Glyph::Notes,
                    if *editing {
                        "SAVE THE CHANGES"
                    } else {
                        "SAVE AS A DRAFT"
                    },
                    |world| send(world, false),
                ));
            }
        }
        if let Some(Step::Done(..)) = step {
            pressed.push(action(Glyph::Book, "YOURS", open_yours));
        }
        if !pressed.is_empty() {
            parts.push(buttons(pressed).boxed());
        }
        parts.extend(problem_now.clone().map(problem));
        match step {
            Some(Step::Done(title, status)) => {
                let words = match status {
                    Status::Listed => {
                        format!("{title} is on the shelves.")
                    }
                    _ => format!(
                        "{title} is saved. It is not on the shelves \
                         yet."
                    ),
                };
                parts.push(outcome(words, false));
            }
            Some(Step::Failed(why)) => {
                parts.push(problem(why.clone()));
            }
            _ => {}
        }
        column(parts).gap(8.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// What the chief sells and has bought, as the Inspector lists its
/// hamlets: a row each, which opens it.
fn yours(back: &'static str) -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        let bought = market
            .library
            .iter()
            .map(|lot| (lot.clone(), kept_words(market, lot)))
            .collect::<Vec<_>>();
        (
            market.mine.clone(),
            bought,
            market.user().map(|user| user.login.clone()),
        )
    })
    .when(moved::<Market>());
    let body = keyed(now, |(mine, bought, login)| {
        let about = match login {
            Some(login) => {
                format!("What @{login} sells and has bought.")
            }
            None => "What you sell and have bought, once you sign \
                     in."
            .into(),
        };
        let mut parts = vec![page_head(
            "Yours",
            note(about).boxed(),
            String::new(),
            None,
        )];
        if login.is_some() {
            let selling = match mine.is_empty() {
                true => note(
                    "You sell nothing yet. SELL AN AGENT, SELL A \
                     HAMLET or SELL TOOLS puts up what you made.",
                )
                .boxed(),
                false => column(
                    mine.iter().map(sold_row).collect::<Vec<_>>(),
                )
                .gap(2.0)
                .boxed(),
            };
            let owned = match bought.is_empty() {
                true => note("You have bought nothing yet.").boxed(),
                false => column(
                    bought
                        .iter()
                        .map(|(lot, kept)| bought_row(lot, kept))
                        .collect::<Vec<_>>(),
                )
                .gap(2.0)
                .boxed(),
            };
            parts.push(section("FOR SALE BY YOU", selling));
            parts.push(section("BOUGHT BY YOU", owned));
        }
        parts.push(trade::account("to see what you sell and own."));
        sections(parts)
    })
    .within(bare());
    desk_page(back, "YOURS", body.boxed())
}

/// How the bought `lot` stands in the home, in a few words, if it is
/// installed.
fn kept_words(market: &Market, lot: &Lot) -> Option<String> {
    let kept = market.bought.get(&lot.id)?;
    let latest = lot.version();
    Some(match semver(latest) > semver(&kept.version) {
        true => format!("Version {latest} is out"),
        false => format!("Installed, version {}", kept.version),
    })
}

/// One listing the chief bought, which opens its page, where it is
/// installed; `kept` says how it stands in the home.
fn bought_row(lot: &Lot, kept: &Option<String>) -> AnyView {
    let opened = lot.clone();
    Line {
        name: lot.title.clone(),
        price: String::new(),
        purpose: lot.summary.clone(),
        meta: format!(
            "{}, by {}",
            lot.kind.name(),
            lot.seller.shown()
        ),
        mark: kept.as_deref().map(accent),
    }
    .view(move |world| open_lot(world, &opened))
}

/// One listing the chief sells, which opens it to be changed.
fn sold_row(lot: &Lot) -> AnyView {
    let id = lot.id.clone();
    Line {
        name: lot.title.clone(),
        price: price_of(lot.price_cents),
        purpose: lot.summary.clone(),
        meta: status_words(lot),
        mark: None,
    }
    .view(move |world| open_edit(world, &id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAID: Option<Payouts> = Some(Payouts {
        connected: true,
        enabled: true,
    });

    fn tool(name: &str) -> PackableTool {
        PackableTool {
            description: format!("{name} does it."),
            name: name.into(),
        }
    }

    #[test]
    fn a_form_offers_what_is_chosen_and_refuses_a_price_unpaid() {
        let all = choices(
            &[],
            &[],
            &[tool("a"), tool("b"), tool("c")],
            Kind::Plugin,
        );
        assert_eq!(all[0].0, "a");
        let form = Form {
            kind: Kind::Plugin,
            title: "Cal".into(),
            summary: "Dates.".into(),
            category: "Data".into(),
            price: "4.99".into(),
            chosen: vec![0, 2],
            ..Form::default()
        };
        assert!(offer(&form, &all, None, true).is_err());
        let offered = offer(&form, &all, None, false).unwrap();
        assert_eq!(
            offered.wares,
            Some(Packing::Tools {
                tools: vec!["a".into(), "c".into()]
            })
        );
        assert_eq!(offered.draft.price_cents, 499);
        let none = Form {
            chosen: Vec::new(),
            ..form
        };
        assert!(offer(&none, &all, PAID, true).is_err());
    }

    #[test]
    fn an_agent_or_a_hamlet_is_packed_with_its_notes_as_chosen() {
        let agents =
            [("ada".into(), "mill".into(), "The Mill".into())];
        let hamlets = [("mill".to_string(), "The Mill".to_string())];
        let sold = choices(&agents, &hamlets, &[], Kind::Agent);
        assert_eq!(sold[0].0, "ada of The Mill");
        let form = Form {
            title: "Ada".into(),
            summary: "Mills.".into(),
            notes: true,
            ..Form::default()
        };
        let offered = offer(&form, &sold, None, true).unwrap();
        assert_eq!(
            offered.wares,
            Some(Packing::Agent {
                agent: Address {
                    agent: "ada".into(),
                    hamlet: "mill".into(),
                },
                notes: true
            })
        );
        let sold = choices(&agents, &hamlets, &[], Kind::Hamlet);
        let form = Form {
            kind: Kind::Hamlet,
            category: "Flour mill".into(),
            ..form
        };
        let offered = offer(&form, &sold, None, true).unwrap();
        assert_eq!(
            offered.wares,
            Some(Packing::Hamlet {
                hamlet: "mill".into(),
                kind: "Flour mill".into(),
                notes: true
            })
        );
    }

    #[test]
    fn a_price_is_free_or_within_the_bazaar_s_bounds() {
        assert_eq!(cents(""), Ok(0));
        assert_eq!(cents("$4.99"), Ok(499));
        assert!(cents("0.10").is_err());
        assert!(cents("five").is_err());
        assert_eq!(tags("Maps, maps, Ink").unwrap(), ["maps", "ink"]);
        let many =
            (0..9).map(|n| format!("t{n}")).collect::<Vec<_>>();
        assert!(tags(&many.join(",")).is_err());
    }
}

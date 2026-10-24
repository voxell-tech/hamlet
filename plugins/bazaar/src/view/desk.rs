//! The seller's desk, laid out as Settings lays out its pages: a
//! form to put an agent, a hamlet or tools up for sale, or to change
//! a listing, its cover and upload a new version of it, and the page
//! of what the chief sells and has bought, listed as the Inspector
//! lists its hamlets.

use agents_ui::{
    Rostered, breadcrumb, either, link_rows, moved, part, setting,
};
use hamlet_engine_bevy::api::agents::{HousedTool, Packing};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, FrameProps, Space, UiExt, ViewExt, button,
    column, derived, description, dropdown, field, icon_button,
    keyed, resource, row, scroll, switch,
};
use serde::{Deserialize, Serialize};

use crate::covers::{PICTURE_HOST, Pick, Picked, pick};
use crate::listings::SELLERS_SHARE;
use crate::market::lot::{
    Draft, Kind, Lot, Payouts, Status, next_version, price_words,
    semver,
};
use crate::market::sell::Step;
use crate::market::{
    self, Housed, Market, Offer, Selling, ask_account, ask_tools,
    set_up_payouts,
};
use crate::picture::{MOST_BYTES, MOST_SIDE};
use crate::samples::agents::Craft;
use crate::samples::plugins::Kind as Shelf;
use crate::view::covers::{chosen, now_of};
use crate::view::parts::{
    Line, mark as accent, outcome, page_head, problem, section,
    sections,
};
use crate::view::trade;
use crate::view::wares::{open_lot, price as price_of};

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

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Desk>();
}

/// What the chief is doing at the seller's desk.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Desk {
    /// The desk's page open in place of the stall's, if one is.
    pub page: Option<Page>,
    pub form: Form,
}

/// A page of the desk.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
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
    /// What is in it while it is empty.
    fn placeholder(self) -> &'static str {
        match self {
            Field::Title => "What it is called",
            Field::Summary => "What it does, in a line",
            Field::Description => "All a buyer should know",
            Field::Category => "Such as Publishing house",
            Field::Tags => "Words it is found by",
            Field::Price => "Free, or such as 4.99",
            Field::Version => "Such as 1.0.0",
            Field::Changes => "What is new in it",
        }
    }

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
    .boxed()
}

/// What can be sold as a listing of `kind`, by name.
pub(crate) fn choices(
    roster: &Rostered,
    tools: &[HousedTool],
    kind: Kind,
) -> Vec<(String, Packing)> {
    let roster = &roster.0;
    let title = |hamlet: &str| {
        roster
            .hamlets
            .iter()
            .find(|one| one.id == hamlet)
            .map_or(hamlet.to_string(), |one| one.title.clone())
    };
    match kind {
        Kind::Agent => roster
            .agents
            .iter()
            .filter(|agent| !agent.gone)
            .map(|agent| {
                (
                    format!(
                        "{} of {}",
                        agent.name,
                        title(&agent.hamlet)
                    ),
                    Packing::Agent {
                        hamlet: agent.hamlet.clone(),
                        name: agent.name.clone(),
                        notes: false,
                    },
                )
            })
            .collect(),
        Kind::Hamlet => roster
            .hamlets
            .iter()
            .map(|hamlet| {
                (
                    hamlet.title.clone(),
                    Packing::Hamlet {
                        name: hamlet.id.clone(),
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
                    format!(
                        "{} of {} in {}",
                        tool.name,
                        tool.agent,
                        title(&tool.hamlet)
                    ),
                    Packing::Tools(vec![tool.clone()]),
                )
            })
            .collect(),
    }
}

/// The choices of the form's kind as the world stands.
fn choices_now(world: &World, kind: Kind) -> Vec<(String, Packing)> {
    let tools = match &world.resource::<Housed>().0 {
        Some(Ok(tools)) => tools.clone(),
        _ => Vec::new(),
    };
    choices(world.resource::<Rostered>(), &tools, kind)
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
        Some(Packing::Tools(mut tools)) => {
            for more in chosen {
                if let Packing::Tools(more) = more {
                    tools.extend(more);
                }
            }
            Ok(Packing::Tools(tools))
        }
        Some(Packing::Hamlet { name, .. }) => Ok(Packing::Hamlet {
            name,
            kind: category.to_string(),
            notes,
        }),
        Some(Packing::Agent { hamlet, name, .. }) => {
            Ok(Packing::Agent {
                hamlet,
                name,
                notes,
            })
        }
    }
}

/// What `form` sends, its wares among `choices`, or why it cannot be
/// sent.
pub(crate) fn offer(
    form: &Form,
    choices: &[(String, Packing)],
    paid: bool,
    publish: bool,
) -> Result<Offer, String> {
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
    let paid = world
        .resource::<Market>()
        .user()
        .is_some_and(|user| user.paid());
    let form = world.resource::<Desk>().form.clone();
    let choices = choices_now(world, form.kind);
    let cover = match &world.resource::<Pick>().state {
        Picked::Nothing => Ok::<_, String>(None),
        Picked::Got(picture) => Ok(Some((
            picture.bytes.to_vec(),
            picture.header.format.media().to_string(),
        ))),
        Picked::Fetching => Err("Wait for the cover to come, or \
                                 clear its address."
            .into()),
        Picked::Failed(_) => Err("The cover could not be had: \
                                  clear its address, or give \
                                  another."
            .into()),
    };
    let offered =
        offer(&form, &choices, paid, publish).and_then(|offer| {
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
            world.resource_mut::<Desk>().form.problem = Some(why)
        }
    }
}

fn edit(world: &mut World, change: impl FnOnce(&mut Form)) {
    change(&mut world.resource_mut::<Desk>().form);
}

/// A field of the form, as Settings has a setting: its name over it,
/// and what goes in it, dim, under it.
fn line(
    name_of: &'static str,
    which: Field,
    hint: impl Into<String>,
) -> AnyView {
    let value = resource::<Desk, _>(move |desk| {
        let mut form = desk.form.clone();
        which.of(&mut form).clone()
    });
    let lines_of = if which == Field::Description { 4 } else { 1 };
    let typed = field(value)
        .placeholder(which.placeholder())
        .lines(lines_of)
        .on_commit(move |world, typed| {
            edit(world, |form| *which.of(form) = typed)
        })
        .boxed();
    let hint = hint.into();
    setting(
        name_of,
        typed,
        (!hint.is_empty()).then_some(hint.as_str()),
    )
}

/// The settings of a part of the form, as Settings lays a page's.
fn fields(parts: Vec<AnyView>) -> AnyView {
    column(parts).boxed()
}

/// A part of the form under its head, as Settings heads the parts of
/// a page: its icon and its name, over `settings`.
fn part_of(
    which: &str,
    title: &str,
    settings: Vec<AnyView>,
) -> AnyView {
    let mut all = vec![part(which, title)];
    all.extend(settings);
    column(all).boxed()
}

/// A button of a row of choices, lit when it is the one chosen, as
/// Welcome offers who gives the model.
fn chip(
    text: &str,
    on: bool,
    then: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(text.to_uppercase())
        .style(if on {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Plain
        })
        .on_click(then)
        .boxed()
}

/// The way back to the stall `back` from the desk's page `here`.
fn crumb(back: &'static str, here: &str) -> AnyView {
    breadcrumb(
        vec![back.to_string()],
        here.to_string(),
        |world, _| close(world),
    )
}

/// A page of the desk: the way back, then `body` in a scroll.
fn desk_page(
    back: &'static str,
    here: &str,
    body: AnyView,
) -> AnyView {
    column((
        crumb(back, here),
        // Room inside the scroll, as a wrapped paragraph can be laid
        // a little wider than the scroll is.
        scroll((column((body,)).padding(Space::Half),)),
    ))
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
                description(about).boxed(),
                String::new(),
                None,
            ),
            part_of("tray", "WHAT YOU SELL", vec![kinds(), sold()]),
            part_of(
                "pencil",
                "HOW IT IS LISTED",
                vec![
                    line(
                        "Title",
                        Field::Title,
                        "At most 60 letters.",
                    ),
                    line(
                        "Summary",
                        Field::Summary,
                        "One line of at most 140 letters, on its \
                         row.",
                    ),
                    line(
                        "Description",
                        Field::Description,
                        "On its page, under the summary.",
                    ),
                    category(),
                    line(
                        "Tags",
                        Field::Tags,
                        "Separated by commas, at most 8.",
                    ),
                ],
            ),
            part_of("page", "COVER", vec![cover()]),
            part_of("coins", "PRICE", vec![price()]),
        ];
        if editing {
            parts.push(part_of(
                "book",
                "ON THE SHELVES",
                vec![shelves()],
            ));
        }
        parts.push(footer());
        parts.push(trade::account("to put it up."));
        desk_page(back, here, sections(parts))
    })
    .boxed()
}

/// The kind of listing put up, while it is a new one.
fn kinds() -> AnyView {
    keyed(
        resource::<Desk, _>(|desk| {
            (desk.form.kind, desk.form.id.is_some())
        }),
        |(chosen, editing)| {
            if *editing {
                return column(()).boxed();
            }
            let all = Kind::ALL
                .into_iter()
                .map(|kind| {
                    let words = match kind {
                        Kind::Agent => "An agent",
                        Kind::Hamlet => "A hamlet",
                        Kind::Plugin => "Tools",
                    };
                    chip(words, *chosen == kind, move |world| {
                        if kind == Kind::Plugin {
                            ask_tools(world);
                        }
                        edit(world, |form| {
                            form.kind = kind;
                            form.chosen = vec![0];
                            form.category =
                                first_category(kind).into();
                        })
                    })
                })
                .collect::<Vec<_>>();
            setting("Kind", row(all).space(Space::Half).boxed(), None)
        },
    )
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

/// The choice of what is packed as the listing's version.
fn sold() -> AnyView {
    let now = derived(|world: &World| {
        let form = &world.resource::<Desk>().form;
        let problem = match (&world.resource::<Housed>().0, form.kind)
        {
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
        either(moved::<Desk>(), moved::<Rostered>()),
        moved::<Housed>(),
    ));
    keyed(now, |now| {
        let mut parts = Vec::new();
        if now.editing {
            parts.push(
                switch("Upload a new version", now.upload)
                    .on_toggle(|world, on| {
                        edit(world, |form| form.upload = on)
                    })
                    .boxed(),
            );
        }
        if !now.upload {
            return fields(parts);
        }
        if now.names.is_empty() {
            parts.push(match (now.kind, &now.problem) {
                (_, Some(why)) => problem(format!(
                    "The tools could not be had: {why}"
                )),
                (Kind::Agent, _) => {
                    description("The realm has no agents to sell.")
                        .boxed()
                }
                (Kind::Hamlet, _) => {
                    description("The realm has no hamlets to sell.")
                        .boxed()
                }
                (Kind::Plugin, _) => description(
                    "No agent has a tool of its own to sell: a \
                     folder with a manifest.json in the tools of \
                     its house.",
                )
                .boxed(),
            });
            return fields(parts);
        }
        match now.kind {
            Kind::Plugin => {
                let tools = now
                    .names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| {
                        switch(name.clone(), now.chosen.contains(&at))
                            .on_toggle(move |world, on| {
                                edit(world, |form| {
                                    form.chosen
                                        .retain(|kept| *kept != at);
                                    if on {
                                        form.chosen.push(at);
                                        form.chosen.sort();
                                    }
                                })
                            })
                            .boxed()
                    })
                    .collect::<Vec<_>>();
                parts.push(setting(
                    "The tools",
                    column(tools).space(Space::Half).boxed(),
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
                    dropdown(now.names.clone(), at as u32)
                        .on_pick(|world, at| {
                            edit(world, |form| {
                                form.chosen = vec![at as usize]
                            })
                        })
                        .boxed(),
                    None,
                ));
                parts.push(setting(
                    "Its notes",
                    switch("Include its notes", now.notes)
                        .on_toggle(|world, on| {
                            edit(world, |form| form.notes = on)
                        })
                        .boxed(),
                    Some(
                        "Notes are what an agent remembers. Read \
                         them before you sell: anything private in \
                         them goes to every buyer.",
                    ),
                ));
            }
        }
        parts.push(line(
            "Version",
            Field::Version,
            "Three numbers, higher than the last.",
        ));
        parts.push(line(
            "What changed",
            Field::Changes,
            "Said beside the version on its page.",
        ));
        fields(parts)
    })
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

/// The cover chosen by the address of a picture, shown as it will be.
fn cover() -> AnyView {
    let address = field(resource::<Pick, _>(|pick| pick.url.clone()))
        .placeholder("Address of a picture")
        .on_commit(pick)
        .boxed();
    let hint = format!(
        "A PNG, JPEG or WebP of at most {} KB and {MOST_SIDE} \
         pixels a side, at an address on {PICTURE_HOST} or the \
         bazaar's server; 640 by 360 pixels shows best. A plugin \
         cannot read your files, so there is none to choose: give \
         the address of the picture.",
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
                picture.header.width,
                picture.header.height,
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
        Covering::None => column(()).boxed(),
        Covering::Kept => column((
            chosen(),
            description(
                "Its cover now. Give an address to change it.",
            ),
        ))
        .space(Space::Half)
        .boxed(),
        Covering::Fetching => description("Fetching it...").boxed(),
        Covering::Got(size) => column((
            chosen(),
            row((
                column((description(size.clone()),)).grow(1.0),
                button("CLEAR")
                    .style(ButtonStyle::Quiet)
                    .on_click(|world| pick(world, String::new())),
            ))
            .space(Space::Gap),
        ))
        .space(Space::Half)
        .boxed(),
        Covering::Failed(why) => problem(format!(
            "{why} Give the address of another picture."
        )),
    });
    fields(vec![
        setting("Picture", address, Some(hint.as_str())),
        state.boxed(),
    ])
}

/// The choice of the shelf it sits on, or the kind of hamlet it is.
fn category() -> AnyView {
    keyed(
        resource::<Desk, _>(|desk| {
            (desk.form.kind, desk.form.category.clone())
        }),
        |(kind, chosen)| {
            let names = match kind {
                Kind::Agent => Craft::ALL.map(Craft::name).to_vec(),
                Kind::Plugin => Shelf::ALL.map(Shelf::name).to_vec(),
                Kind::Hamlet => {
                    return line(
                        "Kind of hamlet",
                        Field::Category,
                        "In a few words, at most 32 letters.",
                    );
                }
            };
            let at = names
                .iter()
                .position(|name| chosen.eq_ignore_ascii_case(name))
                .unwrap_or(0);
            let pick = dropdown(
                names
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>(),
                at as u32,
            )
            .on_pick(move |world, at| {
                let name = names[at as usize];
                edit(world, |form| form.category = name.into())
            });
            setting(
                "Category",
                pick.boxed(),
                Some("The shelf it is found on."),
            )
        },
    )
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
        keyed(now, |(cents, payouts)| payout_words(cents, *payouts));
    fields(vec![
        line(
            "Price in dollars",
            Field::Price,
            "Leave it empty, or 0, to give it for free.",
        ),
        state.boxed(),
    ])
}

fn payout_words(
    cents: &Result<u32, String>,
    payouts: Option<Payouts>,
) -> AnyView {
    let words = match cents {
        Err(why) => return problem(why.clone()),
        Ok(0) => return column(()).boxed(),
        Ok(cents) => format!(
            "You receive {} of each sale, {SELLERS_SHARE}%.",
            price_words(cents * SELLERS_SHARE / 100)
        ),
    };
    let mut parts = vec![description(words).boxed()];
    if !payouts.is_some_and(|payouts| payouts.enabled) {
        let started =
            payouts.is_some_and(|payouts| payouts.connected);
        parts.push(problem(match started {
            true => {
                "Stripe has not finished checking your account yet. \
                 Check again once it has."
            }
            false => {
                "To be paid, set up payouts with Stripe, or make it \
                 free."
            }
        }));
        parts.push(
            row((
                icon_button("coins", "SET UP PAYOUTS")
                    .on_click(set_up_payouts),
                button("CHECK AGAIN")
                    .style(ButtonStyle::Quiet)
                    .on_click(ask_account),
            ))
            .space(Space::Half)
            .boxed(),
        );
    }
    column(parts).space(Space::Half).boxed()
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
            return column(()).boxed();
        };
        let mut parts = vec![description(words).boxed()];
        if status == Status::Listed {
            parts.push(
                row((icon_button(
                    "cross",
                    "TAKE IT OFF THE SHELVES",
                )
                .on_click(move |world| {
                    market::shelve(world, &id, false)
                }),))
                .boxed(),
            );
        }
        column(parts).space(Space::Half).boxed()
    })
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
            Some(Step::Packing) => Some("Packing it...".to_string()),
            Some(Step::Saving) => {
                Some("Sending it to the bazaar...".into())
            }
            Some(Step::Covering) => {
                Some("Sending its cover...".into())
            }
            Some(Step::Uploading(_, at)) => {
                Some(format!("Uploading it, part {}...", at + 1))
            }
            Some(Step::Releasing) => {
                Some("Making its version...".into())
            }
            Some(Step::Publishing) => {
                Some("Putting it on the shelves...".into())
            }
            _ => None,
        };
        let mut buttons = Vec::new();
        let mut parts = Vec::new();
        match going {
            Some(going) => parts.push(description(going).boxed()),
            None => {
                parts.push(
                    description(
                        "A draft is kept at the bazaar, off the \
                         shelves, until you list it.",
                    )
                    .boxed(),
                );
                buttons.push(
                    icon_button("coins", "PUT UP FOR SALE")
                        .on_click(|world| send(world, true))
                        .boxed(),
                );
                buttons.push(
                    icon_button(
                        "page",
                        if *editing {
                            "SAVE THE CHANGES"
                        } else {
                            "SAVE AS A DRAFT"
                        },
                    )
                    .on_click(|world| send(world, false))
                    .boxed(),
                );
            }
        }
        if let Some(Step::Done(..)) = step {
            buttons.push(
                icon_button("book", "YOURS")
                    .on_click(open_yours)
                    .boxed(),
            );
        }
        if !buttons.is_empty() {
            parts.push(row(buttons).space(Space::Half).boxed());
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
                parts.push(problem(why.clone()))
            }
            _ => {}
        }
        column(parts).space(Space::Half).boxed()
    })
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
            description(about).boxed(),
            String::new(),
            None,
        )];
        if login.is_some() {
            let selling = match mine.is_empty() {
                true => description(
                    "You sell nothing yet. SELL AN AGENT, SELL A \
                     HAMLET or SELL TOOLS puts up what you made.",
                )
                .boxed(),
                false => {
                    link_rows(mine.iter().map(sold_row).collect())
                }
            };
            let owned = match bought.is_empty() {
                true => description("You have bought nothing yet.")
                    .boxed(),
                false => link_rows(
                    bought
                        .iter()
                        .map(|(lot, kept)| bought_row(lot, kept))
                        .collect(),
                ),
            };
            parts.push(section("FOR SALE BY YOU", selling));
            parts.push(section("BOUGHT BY YOU", owned));
        }
        parts.push(trade::account("to see what you sell and own."));
        sections(parts)
    });
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
    use hamlet_engine_bevy::api::agents::{
        Roster, RosterAgent, RosterHamlet,
    };

    use super::*;

    fn tool(name: &str) -> HousedTool {
        HousedTool {
            hamlet: "mill".into(),
            agent: "ada".into(),
            name: name.into(),
        }
    }

    #[test]
    fn a_form_offers_what_is_chosen_and_refuses_a_price_unpaid() {
        let roster = Rostered(Roster::default());
        let all = choices(
            &roster,
            &[tool("a"), tool("b"), tool("c")],
            Kind::Plugin,
        );
        assert_eq!(all[0].0, "a of ada in mill");
        let form = Form {
            kind: Kind::Plugin,
            title: "Cal".into(),
            summary: "Dates.".into(),
            category: "Data".into(),
            price: "4.99".into(),
            chosen: vec![0, 2],
            ..Form::default()
        };
        assert!(offer(&form, &all, false, true).is_err());
        let offered = offer(&form, &all, false, false).unwrap();
        assert_eq!(
            offered.wares,
            Some(Packing::Tools(vec![tool("a"), tool("c")]))
        );
        assert_eq!(offered.draft.price_cents, 499);
        let none = Form {
            chosen: Vec::new(),
            ..form
        };
        assert!(offer(&none, &all, true, true).is_err());
    }

    fn agent(name: &str, hamlet: &str) -> RosterAgent {
        RosterAgent {
            name: name.into(),
            number: 1,
            hamlet: hamlet.into(),
            mayor: false,
            purpose: String::new(),
            state: String::new(),
            busy: false,
            gone: false,
            merged_into: None,
            waiting: 0,
            model: String::new(),
            account: String::new(),
            spent: String::new(),
            since: 0,
            until: None,
            doing: Default::default(),
        }
    }

    fn hamlet(id: &str, title: &str) -> RosterHamlet {
        RosterHamlet {
            id: id.into(),
            title: title.into(),
            mayor: String::new(),
            living: 1,
            working: 0,
            waiting: 0,
            gone: 0,
            state: String::new(),
            account: String::new(),
            folder: None,
            manual: false,
            mode: String::new(),
            before: Vec::new(),
            spot: None,
        }
    }

    #[test]
    fn an_agent_or_a_hamlet_is_packed_with_its_notes_as_chosen() {
        let roster = Rostered(Roster {
            agents: vec![agent("ada", "mill")],
            hamlets: vec![hamlet("mill", "The Mill")],
            ..Roster::default()
        });
        let agents = choices(&roster, &[], Kind::Agent);
        assert_eq!(agents[0].0, "ada of The Mill");
        let form = Form {
            title: "Ada".into(),
            summary: "Mills.".into(),
            notes: true,
            ..Form::default()
        };
        let offered = offer(&form, &agents, false, true).unwrap();
        assert_eq!(
            offered.wares,
            Some(Packing::Agent {
                hamlet: "mill".into(),
                name: "ada".into(),
                notes: true
            })
        );
        let hamlets = choices(&roster, &[], Kind::Hamlet);
        let form = Form {
            kind: Kind::Hamlet,
            category: "Flour mill".into(),
            ..form
        };
        let offered = offer(&form, &hamlets, false, true).unwrap();
        assert_eq!(
            offered.wares,
            Some(Packing::Hamlet {
                name: "mill".into(),
                kind: "Flour mill".into(),
                notes: true
            })
        );
    }
}

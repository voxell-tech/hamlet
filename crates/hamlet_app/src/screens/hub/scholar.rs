//! The Hub's page of a scholar: who it is, what it knows, its
//! versions and hires, and what a chief can do with it.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{
    Deeds, LivesIn, Pin, Scholar, name_of, scholar_address,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, ICON, action, back_link, bad_action, bare, beside,
    breadcrumb, buttons, description, good_action, hairline, note,
    scroll_area, text_input, title,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::thread::dim;
use super::{Showing, memory, show};
use crate::chat;
use crate::convo::talk_to;
use crate::overview::{heading, typed};

/// The deeds a scholar's page offers, in the names the server
/// advertises them by.
const DEEDS: [&str; 13] = [
    "name-scholar",
    "welcome",
    "turn-away",
    "move-house",
    "tag",
    "pin",
    "revert",
    "fork",
    "purge",
    "dream",
    "remove-scholar",
    "restore-scholar",
    "erase-scholar",
];

/// What a scholar's page is asking for words to do.
#[derive(Resource, Default, Clone, PartialEq, Eq)]
pub struct Form {
    act: Option<Act>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Act {
    Tag,
    /// A fork from a version, or from `main`.
    Fork(Option<String>),
    Purge,
    Remove,
    /// The erasing for good of a scholar that was archived.
    Erase,
    /// An undoing of the commit a version names.
    Revert(String),
}

/// The first and second field of the form.
#[derive(Component)]
struct First;

#[derive(Component)]
struct Second;

/// Where `hire` stands: mayor of its hamlet, or in it under its lead.
fn place_of(world: &World, hire: Entity) -> String {
    let hamlet = world
        .get::<LivesIn>(hire)
        .map(|lives| name_of(world, lives.0))
        .unwrap_or_default();
    if world.get::<hamlet_client::realm::Mayor>(hire).is_some() {
        return format!("mayor of {hamlet}");
    }
    let lead = world
        .get::<hamlet_client::realm::Parent>(hire)
        .and_then(|parent| {
            hamlet_client::realm::agent(world, parent.0)
        })
        .map(|lead| hamlet_client::realm::shown_of(world, lead));
    match lead {
        Some(lead) => format!("in {hamlet}, under {lead}"),
        None => format!("in {hamlet}"),
    }
}

/// A hire of the scholar as the page shows it.
#[derive(Clone, PartialEq)]
struct HireShown {
    entity: Entity,
    address: String,
    /// What it goes by, and where it stands in its hamlet.
    called: String,
    place: String,
    pin: Option<String>,
}

/// A scholar and its hires as the page reads them.
#[derive(Clone, PartialEq)]
struct Shown {
    /// The scholar's entity, for what acts on it in the world.
    entity: Entity,
    scholar: protocol::Scholar,
    hires: Vec<HireShown>,
    allowed: Vec<&'static str>,
}

fn shown(world: &World, entity: Entity) -> Option<Shown> {
    let scholar = world.get::<Scholar>(entity)?.0.clone();
    let hires = scholar
        .hires
        .iter()
        .filter_map(|id| hamlet_client::realm::agent(world, *id))
        .filter(|hire| {
            hamlet_client::realm::id_of(world, *hire).is_some()
        })
        .map(|hire| {
            let address = name_of(world, hire);
            HireShown {
                entity: hire,
                called: hamlet_client::realm::shown_of(world, hire),
                place: place_of(world, hire),
                pin: world.get::<Pin>(hire).map(|pin| pin.0.clone()),
                address,
            }
        })
        .collect();
    let allowed = DEEDS
        .into_iter()
        .filter(|deed| Deeds::is_allowed(world, deed))
        .collect();
    Some(Shown {
        entity,
        scholar,
        hires,
        allowed,
    })
}

/// Gives the Hand a command about the scholar.
fn tell_hand(world: &mut World, text: String) {
    chat::command(world, text);
}

fn ask(world: &mut World, act: Act) {
    world.resource_mut::<Form>().act = Some(act);
}

/// The command `act` makes of the scholar at `address`, called
/// `name`, and the words typed.
fn command(
    address: &str,
    name: &str,
    act: &Act,
    first: &str,
    second: &str,
) -> Option<String> {
    let (first, second) = (first.trim(), second.trim());
    if first.is_empty() && !matches!(act, Act::Revert(_)) {
        return None;
    }
    Some(match act {
        Act::Tag => format!("/tag {address} {first} {second}"),
        Act::Fork(Some(from)) => {
            format!("/fork {address}@{from} {first}")
        }
        Act::Fork(None) => format!("/fork {address} {first}"),
        Act::Purge => format!("/purge {address} {first}"),
        Act::Remove => format!("/remove-scholar {address} {first}"),
        // Only when its name is typed, as nothing brings it back.
        Act::Erase if first == name => {
            format!("/erase-scholar {address}")
        }
        Act::Erase => return None,
        Act::Revert(to) => format!("/revert {address} {to} {first}"),
    })
}

fn run(world: &mut World, address: &str, name: &str, act: &Act) {
    let first = typed::<First>(world);
    let second = typed::<Second>(world);
    if let Some(text) = command(address, name, act, &first, &second) {
        tell_hand(world, text);
        world.resource_mut::<Form>().act = None;
    }
}

fn field<M: Component>(marker: M) -> impl View {
    text_field(String::new(), |_, _| {})
        .grow(1.0)
        .min_width(px(0.0))
        .rules(text_input)
        .tagged(marker)
}

/// The fields and buttons of the deed being asked for.
fn form(address: String, name: String, act: Act) -> AnyView {
    let (words, first, second) = match &act {
        Act::Tag => (
            "Name this version of its main, with a note.".to_string(),
            "version",
            Some("note"),
        ),
        Act::Fork(Some(from)) => (
            format!("Make a new scholar from {from}."),
            "new scholar's name",
            None,
        ),
        Act::Fork(None) => (
            "Make a new scholar from its main.".to_string(),
            "new scholar's name",
            None,
        ),
        Act::Purge => (
            "Take a file out of its whole history.".to_string(),
            "path",
            None,
        ),
        Act::Remove => {
            ("Archive it and end its hires.".to_string(), "why", None)
        }
        Act::Erase => (
            "Erase it, its repository and its hires for good. \
             Nothing brings it back."
                .to_string(),
            "type its name",
            None,
        ),
        Act::Revert(_) => (
            "Undo that version's commit of its main.".to_string(),
            "why (optional)",
            None,
        ),
    };
    let remove = matches!(act, Act::Remove | Act::Purge | Act::Erase);
    let go = {
        let act = act.clone();
        move |world: &mut World| run(world, &address, &name, &act)
    };
    let ready = if remove {
        bad_action(Glyph::Check, "Do it", go)
    } else {
        good_action(Glyph::Check, "Do it", go)
    };
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let mut parts = vec![
            note(words).boxed(),
            row((
                label(first).tone(Tone::Faint).wrap(false),
                field(First),
            ))
            .gap(gap)
            .align(AlignItems::Center)
            .boxed(),
        ];
        if let Some(second) = second {
            parts.push(
                row((
                    label(second).tone(Tone::Faint).wrap(false),
                    field(Second),
                ))
                .gap(gap)
                .align(AlignItems::Center)
                .boxed(),
            );
        }
        parts.push(
            buttons(vec![
                ready,
                action(Glyph::Cross, "Cancel", |world| {
                    world.resource_mut::<Form>().act = None;
                }),
            ])
            .boxed(),
        );
        column(parts).gap(gap).boxed()
    })
    .boxed()
}

fn form_view(address: String, name: String) -> impl View {
    keyed(resource::<Form, _>(|form| form.act.clone()), move |act| {
        match act {
            Some(act) => {
                form(address.clone(), name.clone(), act.clone())
            }
            None => column(()).boxed(),
        }
    })
    .within(bare())
}

/// Where the scholar stands, said beside its name: in the city, at
/// the harbor or archived.
fn standing(scholar: &protocol::Scholar) -> (&'static str, Tone) {
    match (&scholar.left, scholar.arrival) {
        (Some(_), _) => ("Archived", Tone::Critical),
        (None, protocol::Arrival::Housed) => {
            ("In the city", Tone::Dim)
        }
        (None, protocol::Arrival::Waiting) => {
            ("Waits at the harbor", Tone::Critical)
        }
        (None, protocol::Arrival::Turned) => {
            ("Turned away at the harbor", Tone::Dim)
        }
    }
}

/// The deeds that change where the scholar stands.
fn arrival(shown: &Shown) -> AnyView {
    let address = scholar_address(shown.scholar.id);
    let can = |deed| shown.allowed.contains(&deed);
    let mut buttons_row = Vec::new();
    match (&shown.scholar.left, shown.scholar.arrival) {
        (Some(_), _) => {
            if can("restore-scholar") {
                let address = address.clone();
                buttons_row.push(good_action(
                    Glyph::Restore,
                    "Restore",
                    move |world| {
                        tell_hand(
                            world,
                            format!("/restore-scholar {address}"),
                        );
                    },
                ));
            }
        }
        // Picked up and set down in the world; sending it back is
        // among the careful acts at the foot.
        (None, protocol::Arrival::Housed) => {
            if can("move-house") {
                let scholar = shown.entity;
                buttons_row.push(
                    action(Glyph::Move, "Move house", move |world| {
                        hamlet_realm::orders::carry_house(
                            world, scholar,
                        );
                    })
                    .tooltip(|| {
                        note(
                            "Pick its house up and set it down \
                             elsewhere in the city",
                        )
                    })
                    .boxed(),
                );
            }
        }
        (None, _) => {
            if can("welcome") {
                let address = address.clone();
                buttons_row.push(good_action(
                    Glyph::Check,
                    "Welcome into the city",
                    move |world| {
                        tell_hand(
                            world,
                            format!("/welcome {address}"),
                        );
                    },
                ));
            }
        }
    }
    buttons(buttons_row).boxed()
}

/// The versions, each with what can be done from it.
fn versions(shown: &Shown) -> AnyView {
    let can = |deed| shown.allowed.contains(&deed);
    let mut parts = vec![heading(Glyph::History, "Versions")];
    if shown.scholar.versions.is_empty() {
        parts.push(note("No version is named yet.").boxed());
    }
    // Newest first; each with its own menu of what can be done from
    // it, named in full there.
    for version in shown.scholar.versions.iter().rev() {
        let name = version.name.clone();
        let mut choices = Vec::<(String, Act)>::new();
        if can("fork") {
            choices.push((
                format!("Make a new scholar from {name}"),
                Act::Fork(Some(name.clone())),
            ));
        }
        if can("revert") {
            choices.push((
                format!("Undo the change {name} named"),
                Act::Revert(name.clone()),
            ));
        }
        let followers = shown
            .hires
            .iter()
            .filter(|hire| hire.pin.as_deref() == Some(name.as_str()))
            .map(|hire| hire.called.clone())
            .collect::<Vec<_>>();
        let mut line = vec![
            label(name.clone()).bold(true).boxed(),
            dim(version.note.clone()).boxed(),
        ];
        if !followers.is_empty() {
            line.push(
                label(format!(
                    "followed by {}",
                    followers.join(", ")
                ))
                .tone(Tone::Accent)
                .boxed(),
            );
        }
        line.push(frame().grow(1.0).boxed());
        if !choices.is_empty() {
            let names = choices
                .iter()
                .map(|(n, _)| n.clone())
                .collect::<Vec<_>>();
            let acts = choices
                .into_iter()
                .map(|(_, a)| a)
                .collect::<Vec<_>>();
            let none = names.len();
            line.push(
                dropdown(
                    names,
                    none,
                    Glyph::Chevron.image(ICON),
                    move |world, chosen| {
                        if let Some(act) = acts.get(chosen) {
                            ask(world, act.clone());
                        }
                    },
                )
                .placeholder("Actions")
                .boxed(),
            );
        }
        parts.push(
            row(line).gap(8.0).align(AlignItems::Center).boxed(),
        );
    }
    let mut acts = Vec::new();
    if can("tag") {
        acts.push(
            action(Glyph::Plus, "Save as a version", |world| {
                ask(world, Act::Tag);
            })
            .tooltip(|| note("Name what the scholar knows now"))
            .boxed(),
        );
    }
    if can("fork") {
        acts.push(
            action(
                Glyph::NewSession,
                "New scholar from this one",
                |world| {
                    ask(world, Act::Fork(None));
                },
            )
            .tooltip(|| {
                note(
                    "Start another scholar knowing all this one \
                     knows now",
                )
            })
            .boxed(),
        );
    }
    parts.push(buttons(acts).boxed());
    column(parts).gap(6.0).boxed()
}

/// Its hires, each led to and pinned to a version from here.
fn hires(shown: &Shown) -> AnyView {
    let mut parts = vec![
        heading(Glyph::Robot, "Hires"),
        note("Agents at work in hamlets as this scholar.").boxed(),
    ];
    if shown.hires.is_empty() {
        parts.push(note("Nobody works as it now.").boxed());
    }
    let pins = shown.allowed.contains(&"pin");
    let mut names = vec!["main".to_string()];
    names.extend(
        shown.scholar.versions.iter().map(|v| v.name.clone()),
    );
    // `main` is what the scholar knows now.
    let words = names
        .iter()
        .map(|name| match name.as_str() {
            "main" => "Latest".to_string(),
            version => version.to_string(),
        })
        .collect::<Vec<_>>();
    for shown_hire in &shown.hires {
        let hire = shown_hire.entity;
        let address = shown_hire.address.clone();
        let choices = names.clone();
        // The version it follows now, main when it is not pinned.
        let now = shown_hire
            .pin
            .as_ref()
            .and_then(|pin| choices.iter().position(|v| v == pin))
            .unwrap_or(0);
        let mut line = vec![
            button(label(shown_hire.called.clone()).bold(true))
                .padding(UiRect::all(px(0.0)))
                .rules(back_link)
                .tagged(Variant::Text)
                .tooltip({
                    let address = address.clone();
                    move || note(address.clone())
                })
                .on_activate(move |world| talk_to(world, hire))
                .boxed(),
            label(shown_hire.place.clone()).tone(Tone::Dim).boxed(),
            frame().grow(1.0).boxed(),
        ];
        if pins {
            let picked = choices.clone();
            line.push(
                label("Works from")
                    .tone(Tone::Faint)
                    .tooltip(|| {
                        note(
                            "The version of what the scholar knows \
                             that this hire works from",
                        )
                    })
                    .boxed(),
            );
            line.push(
                dropdown(
                    words.clone(),
                    now,
                    Glyph::Chevron.image(ICON),
                    move |world, chosen| {
                        if chosen != now
                            && let Some(to) = picked.get(chosen)
                        {
                            tell_hand(
                                world,
                                format!("/pin {address} {to}"),
                            );
                        }
                    },
                )
                .boxed(),
            );
        }
        parts.push(
            row(line).gap(8.0).align(AlignItems::Center).boxed(),
        );
    }
    column(parts).gap(6.0).boxed()
}

/// Its skills and the character a chief may edit.
fn knowledge() -> impl View {
    column((
        memory::character_section(),
        heading(Glyph::Notes, "Memory"),
        memory::view(),
        heading(Glyph::Book, "Skills"),
        memory::skills(),
    ))
    .gap(6.0)
}

/// What the scholar is called, typed over: Enter gives it the name.
/// Names are labels that may change and repeat, so the scholar stays
/// the one at its address.
fn renaming(shown: &Shown) -> AnyView {
    if shown.scholar.left.is_some()
        || !shown.allowed.contains(&"name-scholar")
    {
        return frame().boxed();
    }
    let address = scholar_address(shown.scholar.id);
    let had = shown.scholar.name.clone();
    let field = text_field(had.clone(), move |world, typed| {
        let typed = typed.trim().to_string();
        if !typed.is_empty() && typed != had {
            tell_hand(
                world,
                format!("/name-scholar {address} {typed}"),
            );
        }
    })
    .grow(1.0)
    .min_width(px(0.0))
    .rules(text_input);
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        row((label("Called").tone(Tone::Faint).wrap(false), field))
            .gap(gap)
            .align(AlignItems::Center)
            .boxed()
    })
    .boxed()
}

/// What it is good to do to the scholar now and then.
fn deeds(shown: &Shown) -> AnyView {
    let address = scholar_address(shown.scholar.id);
    if shown.scholar.left.is_some()
        || !shown.allowed.contains(&"dream")
    {
        return frame().boxed();
    }
    with_cx(move |cx| {
        let address = address.clone();
        row((
            action(Glyph::Moon, "Dream now", move |world| {
                tell_hand(world, format!("/dream {address}"));
            }),
            description(
                "Brings home what its hires learned, and tidies its \
                 memory.",
            ),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    })
    .boxed()
}

/// What changes the scholar for good or sends it away, kept at the
/// foot of its page under its own heading.
fn careful(shown: &Shown) -> AnyView {
    let address = scholar_address(shown.scholar.id);
    let can = |deed| shown.allowed.contains(&deed);
    let living = shown.scholar.left.is_none();
    let mut acts = Vec::new();
    if living
        && shown.scholar.arrival == protocol::Arrival::Housed
        && can("turn-away")
    {
        acts.push(action(
            Glyph::Anchor,
            "Send to the harbor",
            move |world| {
                tell_hand(world, format!("/turn-away {address}"));
            },
        ));
    }
    if living && can("purge") {
        acts.push(
            bad_action(
                Glyph::Delete,
                "Erase a file from its history",
                |world| {
                    ask(world, Act::Purge);
                },
            )
            .tooltip(|| {
                note(
                    "Removes one file, like memory/notes.md, from \
                     every commit and version, for good",
                )
            })
            .boxed(),
        );
    }
    if living && can("remove-scholar") {
        acts.push(bad_action(Glyph::Gone, "Remove", |world| {
            ask(world, Act::Remove);
        }));
    }
    if !living && can("erase-scholar") {
        acts.push(bad_action(
            Glyph::Delete,
            "Erase for good",
            |world| {
                ask(world, Act::Erase);
            },
        ));
    }
    if acts.is_empty() {
        return frame().boxed();
    }
    column((heading(Glyph::Stop, "Careful"), buttons(acts)))
        .gap(6.0)
        .boxed()
}

/// The scholar's page: where it stands, its hires, what it knows, its
/// versions, then what changes it for good.
fn body(shown: &Shown) -> AnyView {
    let name = shown.scholar.name.clone();
    let address = scholar_address(shown.scholar.id);
    let rename = renaming(shown);
    let (stands, tone) = standing(&shown.scholar);
    let heading = column((
        beside((title(name.clone()), label(stands).tone(tone))),
        description(
            "A scholar is who its hires are when they are not at \
             work: its character, memory and skills.",
        ),
    ))
    .gap(4.0);
    let (head, versions_part, hires_part, deeds_part, careful_part) = (
        arrival(shown),
        versions(shown),
        hires(shown),
        deeds(shown),
        careful(shown),
    );
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let section = cx.theme().space.section;
        column((
            breadcrumb(
                vec!["Realm".into()],
                name.clone(),
                |world, _| show(world, Showing::Realm),
            ),
            heading,
            head,
            rename,
            deeds_part,
            form_view(address, name),
            hairline(line),
            hires_part,
            hairline(line),
            knowledge(),
            hairline(line),
            versions_part,
            hairline(line),
            careful_part,
        ))
        .gap(section)
    })
    .boxed()
}

/// The page of `scholar`.
pub(super) fn page(scholar: Entity) -> impl View {
    column((scroll_area((keyed(
        derived(move |world: &World| shown(world, scholar))
            .when(every_frame()),
        |shown| match shown {
            Some(shown) => body(shown),
            None => dim("That scholar is not in the city.").boxed(),
        },
    )
    .within(bare()),)),))
    .fills_height()
}

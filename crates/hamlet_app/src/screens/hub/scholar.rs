//! The Hub's page of a scholar: who it is, what it knows, its
//! versions and hires, and what a chief can do with it.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{Deeds, Scholar, name_of};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, ICON, action, back_link, bad_action, bare, breadcrumb,
    buttons, good_action, hairline, lettered, note, scroll_area,
    status_label_large, text_input, title,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::papers::{self, Papers};
use super::thread::dim;
use super::{Showing, memory, show};
use crate::chat;
use crate::convo::talk_to;
use crate::overview::{heading, typed};

/// The deeds a scholar's page offers, in the names the server
/// advertises them by.
const DEEDS: [&str; 10] = [
    "welcome",
    "turn-away",
    "tag",
    "pin",
    "revert",
    "fork",
    "purge",
    "dream",
    "remove-scholar",
    "restore-scholar",
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
    /// An undoing of the commit a version names.
    Revert(String),
}

/// The first and second field of the form.
#[derive(Component)]
struct First;

#[derive(Component)]
struct Second;

/// A scholar and its hires as the page reads them.
#[derive(Clone, PartialEq)]
struct Shown {
    scholar: protocol::Scholar,
    hires: Vec<(Entity, String)>,
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
        .map(|hire| (hire, name_of(world, hire)))
        .collect();
    let allowed = DEEDS
        .into_iter()
        .filter(|deed| Deeds::is_allowed(world, deed))
        .collect();
    Some(Shown {
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

/// The command `act` makes of `name` and the words typed.
fn command(
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
        Act::Tag => format!("/tag {name} {first} {second}"),
        Act::Fork(Some(from)) => {
            format!("/fork {name}@{from} {first}")
        }
        Act::Fork(None) => format!("/fork {name} {first}"),
        Act::Purge => format!("/purge {name} {first}"),
        Act::Remove => format!("/remove-scholar {name} {first}"),
        Act::Revert(to) => format!("/revert {name} {to} {first}"),
    })
}

fn run(world: &mut World, name: &str, act: &Act) {
    let first = typed::<First>(world);
    let second = typed::<Second>(world);
    if let Some(text) = command(name, act, &first, &second) {
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
fn form(name: String, act: Act) -> AnyView {
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
        Act::Revert(_) => (
            "Undo that version's commit of its main.".to_string(),
            "why (optional)",
            None,
        ),
    };
    let remove = act == Act::Remove || act == Act::Purge;
    let go = {
        let act = act.clone();
        move |world: &mut World| run(world, &name, &act)
    };
    let ready = if remove {
        bad_action(Glyph::Check, "DO IT", go)
    } else {
        good_action(Glyph::Check, "DO IT", go)
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
                action(Glyph::Cross, "CANCEL", |world| {
                    world.resource_mut::<Form>().act = None;
                }),
            ])
            .boxed(),
        );
        column(parts).gap(gap).boxed()
    })
    .boxed()
}

fn form_view(name: String) -> impl View {
    keyed(resource::<Form, _>(|form| form.act.clone()), move |act| {
        match act {
            Some(act) => form(name.clone(), act.clone()),
            None => column(()).boxed(),
        }
    })
    .within(bare())
}

/// What its arrival says, and the deeds that change it.
fn arrival(shown: &Shown) -> AnyView {
    let name = shown.scholar.name.clone();
    let can = |deed| shown.allowed.contains(&deed);
    let (glyph, words, tone) =
        match (&shown.scholar.left, shown.scholar.arrival) {
            (Some(_), _) => (Glyph::Gone, "Archived", Tone::Critical),
            (None, protocol::Arrival::Housed) => {
                (Glyph::Hamlet, "Lives in the city", Tone::Accent)
            }
            (None, protocol::Arrival::Waiting) => {
                (Glyph::Bell, "Waits at the harbor", Tone::Critical)
            }
            (None, protocol::Arrival::Turned) => {
                (Glyph::Gone, "Turned away at the harbor", Tone::Dim)
            }
        };
    let mut buttons_row = Vec::new();
    match (&shown.scholar.left, shown.scholar.arrival) {
        (Some(_), _) => {
            if can("restore-scholar") {
                let name = name.clone();
                buttons_row.push(good_action(
                    Glyph::Restore,
                    "RESTORE",
                    move |world| {
                        tell_hand(
                            world,
                            format!("/restore-scholar {name}"),
                        );
                    },
                ));
            }
        }
        (None, protocol::Arrival::Housed) => {
            if can("turn-away") {
                let name = name.clone();
                buttons_row.push(action(
                    Glyph::Back,
                    "TURN AWAY",
                    move |world| {
                        tell_hand(
                            world,
                            format!("/turn-away {name}"),
                        );
                    },
                ));
            }
        }
        (None, _) => {
            if can("welcome") {
                let name = name.clone();
                buttons_row.push(good_action(
                    Glyph::Check,
                    "WELCOME INTO THE CITY",
                    move |world| {
                        tell_hand(world, format!("/welcome {name}"));
                    },
                ));
            }
        }
    }
    column((
        status_label_large(glyph, words, tone),
        buttons(buttons_row),
    ))
    .gap(6.0)
    .boxed()
}

/// The versions, each with what can be done from it.
fn versions(shown: &Shown) -> AnyView {
    let can = |deed| shown.allowed.contains(&deed);
    let mut parts = vec![heading(Glyph::History, "VERSIONS")];
    if shown.scholar.versions.is_empty() {
        parts.push(note("No version is tagged yet.").boxed());
    }
    for version in &shown.scholar.versions {
        let mut acts = Vec::new();
        if can("fork") {
            let from = version.name.clone();
            acts.push(action(
                Glyph::NewSession,
                "FORK",
                move |world| {
                    ask(world, Act::Fork(Some(from.clone())));
                },
            ));
        }
        if can("revert") {
            let to = version.name.clone();
            acts.push(action(
                Glyph::Restore,
                "REVERT",
                move |world| {
                    ask(world, Act::Revert(to.clone()));
                },
            ));
        }
        parts.push(
            column((
                row((
                    label(version.name.clone()).bold(true),
                    dim(version.note.clone()),
                ))
                .gap(8.0)
                .align(AlignItems::Center),
                buttons(acts),
            ))
            .gap(4.0)
            .boxed(),
        );
    }
    let mut acts = Vec::new();
    if can("tag") {
        acts.push(action(Glyph::Plus, "TAG ITS MAIN", |world| {
            ask(world, Act::Tag);
        }));
    }
    if can("fork") {
        acts.push(action(
            Glyph::NewSession,
            "FORK ITS MAIN",
            |world| {
                ask(world, Act::Fork(None));
            },
        ));
    }
    parts.push(buttons(acts).boxed());
    column(parts).gap(6.0).boxed()
}

/// Its hires, each led to and pinned to a version from here.
fn hires(shown: &Shown) -> AnyView {
    let mut parts = vec![heading(Glyph::Robot, "HIRES")];
    if shown.hires.is_empty() {
        parts.push(note("Nobody works as it now.").boxed());
    }
    let pins = shown.allowed.contains(&"pin");
    let mut names = vec!["main".to_string()];
    names.extend(
        shown.scholar.versions.iter().map(|v| v.name.clone()),
    );
    for (hire, name) in &shown.hires {
        let hire = *hire;
        let address = name.clone();
        let choices = names.clone();
        let past = choices.len();
        let mut line = vec![
            lettered(name.clone())
                .rules(back_link)
                .tagged(Variant::Text)
                .on_activate(move |world| talk_to(world, hire))
                .boxed(),
        ];
        if pins {
            let picked = choices.clone();
            line.push(
                dropdown(
                    choices,
                    past,
                    Glyph::Chevron.image(ICON),
                    move |world, chosen| {
                        if let Some(to) = picked.get(chosen) {
                            tell_hand(
                                world,
                                format!("/pin {address} {to}"),
                            );
                        }
                    },
                )
                .placeholder("PIN TO")
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
    let skills = keyed(
        resource::<Papers, _>(|p| p.skills.clone()),
        |skills| {
            if skills.is_empty() {
                return note("No skills of its own yet.").boxed();
            }
            column(
                skills
                    .iter()
                    .map(|(name, does)| {
                        crate::lists::item(
                            name.clone(),
                            does.clone(),
                            Tone::Body,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(4.0)
            .boxed()
        },
    )
    .within(bare());
    let character = keyed(
        resource::<Papers, _>(|p| p.character.clone()),
        |text| {
            if text.is_empty() {
                note("It has no character written.").boxed()
            } else {
                hamlet_ui::markdown::fixed(text, Tone::Body).boxed()
            }
        },
    )
    .within(bare());
    column((
        heading(Glyph::Robot, "CHARACTER"),
        character,
        row((action(Glyph::Rename, "EDIT", papers::edit_character),)),
        heading(Glyph::Notes, "MEMORY"),
        memory::view(),
        heading(Glyph::Book, "SKILLS"),
        skills,
    ))
    .gap(6.0)
}

/// What can be done to the scholar as a whole.
fn deeds(shown: &Shown) -> AnyView {
    let name = shown.scholar.name.clone();
    let can = |deed| shown.allowed.contains(&deed);
    let living = shown.scholar.left.is_none();
    let mut acts = Vec::new();
    if living && can("dream") {
        let name = name.clone();
        acts.push(action(Glyph::Moon, "DREAM", move |world| {
            tell_hand(world, format!("/dream {name}"));
        }));
    }
    if living && can("purge") {
        acts.push(bad_action(
            Glyph::Delete,
            "PURGE A FILE",
            |world| {
                ask(world, Act::Purge);
            },
        ));
    }
    if living && can("remove-scholar") {
        acts.push(bad_action(Glyph::Gone, "REMOVE", |world| {
            ask(world, Act::Remove);
        }));
    }
    buttons(acts).boxed()
}

fn body(shown: &Shown) -> AnyView {
    let name = shown.scholar.name.clone();
    let (head, versions_part, hires_part, deeds_part) =
        (arrival(shown), versions(shown), hires(shown), deeds(shown));
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let section = cx.theme().space.section;
        column((
            breadcrumb(
                vec!["HOME".into()],
                name.to_uppercase(),
                |world, _| show(world, Showing::Home),
            ),
            title(name.to_uppercase()),
            head,
            deeds_part,
            form_view(name),
            hairline(line),
            knowledge(),
            hairline(line),
            versions_part,
            hairline(line),
            hires_part,
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

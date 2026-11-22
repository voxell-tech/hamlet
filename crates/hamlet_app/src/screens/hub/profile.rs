//! The Hub's page of a profile.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, menu_button, row, text_field,
};
use bevy_fynix::{
    Binding, ScopedExt, ViewExt, component, derived, every_frame,
    keyed,
};
use hamlet_client::realm::{
    Actions, Learned, LivesIn, Pin, Profile, name_of, profile_address,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, ICON, Looks, action, bad_action, bare, beside, buttons,
    crumb, crumb_slash, crumbs, filling, good_action, note,
    scroll_area, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::details::{section, section_line};
use super::thread::dim;
use super::{Showing, memory, open_hamlet, rail, show};
use crate::convo::talk_to;
use crate::profile_tag::{learned_mark, learned_words};
use crate::standing::Standing;
use crate::{chat, world_menu};

/// The server's names of the actions a profile's page offers.
const ACTIONS: [&str; 13] = [
    "name-profile",
    "welcome",
    "turn-away",
    "move-house",
    "tag",
    "pin",
    "revert",
    "fork",
    "purge",
    "dream",
    "remove-profile",
    "restore-profile",
    "erase-profile",
];

/// What a Hub's profile page is asking for words to do.
#[derive(Component, Default, Clone, PartialEq, Eq)]
pub struct Form {
    act: Option<Act>,
    /// The profile it was asked about.
    of: Option<Entity>,
    /// Whether the reason it may take is shown.
    more: bool,
}

impl Form {
    /// What it asks for words to do to `profile`.
    fn act_on(&self, profile: Entity) -> Option<&Act> {
        self.act.as_ref().filter(|_| self.of == Some(profile))
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Act {
    Tag,
    /// A fork from a version, or from `main`.
    Fork(Option<String>),
    /// The taking of one file out of its whole history.
    Purge(String),
    Remove,
    /// The erasing for good of a profile that was archived.
    Erase,
    /// An undoing of the commit a version names.
    Revert(String),
}

/// The first or the second field of a Hub's form.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct Field {
    hub: Entity,
    second: bool,
}

/// Where `agent` stands: lead of its hamlet, or in it under its
/// parent.
fn place_of(world: &World, agent: Entity) -> Place {
    let hamlet = world
        .get::<LivesIn>(agent)
        .map(|lives| (lives.0, name_of(world, lives.0)));
    if world.get::<hamlet_client::realm::Lead>(agent).is_some() {
        return Place::Lead(hamlet);
    }
    let parent = world
        .get::<hamlet_client::realm::Parent>(agent)
        .and_then(|parent| {
            hamlet_client::realm::agent(world, parent.0)
        })
        // The Hand above a lead is not in its hamlet.
        .filter(|parent| world.get::<LivesIn>(*parent).is_some())
        .map(|parent| {
            (parent, hamlet_client::realm::shown_of(world, parent))
        });
    Place::Under { hamlet, parent }
}

/// Where an agent stands.
#[derive(Clone, PartialEq)]
enum Place {
    Lead(Option<(Entity, String)>),
    Under {
        hamlet: Option<(Entity, String)>,
        parent: Option<(Entity, String)>,
    },
}

/// An agent of the profile as the page shows it.
#[derive(Clone, PartialEq)]
struct HamletAgentShown {
    entity: Entity,
    address: String,
    /// What it goes by, and where it stands in its hamlet.
    called: String,
    place: Place,
    pin: Option<String>,
    /// Whether it holds what it learned that the profile lacks.
    learned: bool,
}

/// A profile and its agents as the page reads them.
#[derive(Clone, PartialEq)]
struct Shown {
    entity: Entity,
    profile: protocol::Profile,
    agents: Vec<HamletAgentShown>,
    allowed: Vec<&'static str>,
}

fn shown(world: &World, entity: Entity) -> Option<Shown> {
    let profile = world.get::<Profile>(entity)?.0.clone();
    let agents = profile
        .agents
        .iter()
        .filter_map(|id| hamlet_client::realm::agent(world, *id))
        .filter(|agent| {
            hamlet_client::realm::id_of(world, *agent).is_some()
        })
        .map(|agent| {
            let address = name_of(world, agent);
            HamletAgentShown {
                entity: agent,
                called: hamlet_client::realm::shown_of(world, agent),
                place: place_of(world, agent),
                pin: world.get::<Pin>(agent).map(|pin| pin.0.clone()),
                learned: world.get::<Learned>(agent).is_some(),
                address,
            }
        })
        .collect();
    let allowed = ACTIONS
        .into_iter()
        .filter(|name| Actions::is_allowed(world, name))
        .collect();
    Some(Shown {
        entity,
        profile,
        agents,
        allowed,
    })
}

/// Gives the Hand a command about the profile.
fn tell_hand(world: &mut World, text: String) {
    chat::command(world, text);
}

/// Asks in `hub` for the words to do `act` to the profile it shows.
fn ask(world: &mut World, hub: Entity, act: Act) {
    let Some(Showing::Profile(profile)) = super::showing(world, hub)
    else {
        return;
    };
    world.entity_mut(hub).insert(Form {
        act: Some(act),
        of: Some(profile),
        more: false,
    });
}

/// Lets go of what `hub`'s form asks.
fn drop_form(world: &mut World, hub: Entity) {
    world.entity_mut(hub).insert(Form::default());
}

/// The words typed in `field`.
fn typed(world: &mut World, field: Field) -> String {
    let roots = world
        .query::<(&Field, &Children)>()
        .iter(world)
        .filter(|(of, _)| **of == field)
        .flat_map(|(_, children)| children.iter().copied())
        .collect::<Vec<_>>();
    roots
        .into_iter()
        .find_map(|child| world.get::<EditableText>(child))
        .map(|text| text.value().to_string())
        .unwrap_or_default()
}

/// The command `act` makes of the profile and the words typed.
fn command(
    address: &str,
    name: &str,
    act: &Act,
    first: &str,
    second: &str,
) -> Option<String> {
    let (first, second) = (first.trim(), second.trim());
    // A reason may be left out.
    if first.is_empty()
        && !matches!(
            act,
            Act::Revert(_) | Act::Purge(_) | Act::Remove
        )
    {
        return None;
    }
    Some(match act {
        Act::Tag => format!("/tag {address} {first} {second}"),
        Act::Fork(Some(from)) => {
            format!("/fork {address}@{from} {first}")
        }
        Act::Fork(None) => format!("/fork {address} {first}"),
        Act::Purge(path) => format!("/purge {address} {path}"),
        Act::Remove => format!("/remove-profile {address} {first}")
            .trim_end()
            .to_string(),
        // Only when its name is typed, as nothing brings it back.
        Act::Erase if first == name => {
            format!("/erase-profile {address}")
        }
        Act::Erase => return None,
        Act::Revert(to) => format!("/revert {address} {to} {first}"),
    })
}

fn run(
    world: &mut World,
    hub: Entity,
    address: &str,
    name: &str,
    act: &Act,
) {
    let first = typed(world, Field { hub, second: false });
    let second = typed(world, Field { hub, second: true });
    if let Some(text) = command(address, name, act, &first, &second) {
        if *act == Act::Erase
            && let Some(profile) = profile_at(world, address)
        {
            super::erasure::expect_profile(
                world,
                profile,
                name.into(),
            );
        }
        tell_hand(world, text);
        drop_form(world, hub);
    }
}

/// The profile at `address`.
fn profile_at(world: &World, address: &str) -> Option<Entity> {
    let mut profiles = world.try_query::<(Entity, &Profile)>()?;
    profiles
        .iter(world)
        .find(|(_, profile)| profile_address(profile.0.id) == address)
        .map(|(entity, _)| entity)
}

fn field(field: Field) -> impl View {
    text_field(Binding::new(String::new(), |_, _| {}))
        .grow(1.0)
        .min_width(px(0.0))
        .rules(text_input)
        .tagged(field)
}

/// The fields and buttons of the action being asked for in `hub`.
fn form(
    hub: Entity,
    address: String,
    name: String,
    act: Act,
) -> AnyView {
    let (words, first, second) = match &act {
        Act::Tag => (
            "Name this version of its main, with a note.".to_string(),
            Some("version"),
            Some("note"),
        ),
        Act::Fork(Some(from)) => (
            format!("Make a new profile from {from}."),
            Some("new profile's name"),
            None,
        ),
        Act::Fork(None) => (
            "Make a new profile from its main.".to_string(),
            Some("new profile's name"),
            None,
        ),
        Act::Purge(path) => (
            format!(
                "Take {path} out of every commit and version, for \
                 good."
            ),
            None,
            None,
        ),
        Act::Remove => (
            "Archive it and end its agents.".to_string(),
            Some("why"),
            None,
        ),
        Act::Erase => (
            "Erase it, its repository and its agents for good. \
             Nothing brings it back."
                .to_string(),
            Some("type its name"),
            None,
        ),
        Act::Revert(_) => (
            "Undo that version's commit of its main.".to_string(),
            Some("why"),
            None,
        ),
    };
    // A reason may be left out, so it waits under "More options".
    let optional = matches!(act, Act::Remove | Act::Revert(_));
    let remove = is_careful(&act);
    let go = {
        let act = act.clone();
        move |world: &mut World| {
            run(world, hub, &address, &name, &act);
        }
    };
    let ready = if remove {
        bad_action(Glyph::Check, "Do It", go)
    } else {
        good_action(Glyph::Check, "Do It", go)
    };
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let mut parts = vec![note(words).boxed()];
        if let Some(first) = first {
            let line = move || {
                row((
                    label(first).tone(Tone::Faint).wrap(false),
                    field(Field { hub, second: false })
                        .tagged(world_menu::TakesFocus),
                ))
                .gap(gap)
                .align(AlignItems::Center)
                .boxed()
            };
            parts.push(if optional {
                world_menu::optional_in::<Form>(
                    hub,
                    |form| form.more,
                    |form| form.more = !form.more,
                    line,
                )
                .boxed()
            } else {
                line()
            });
        }
        if let Some(second) = second {
            parts.push(
                row((
                    label(second).tone(Tone::Faint).wrap(false),
                    field(Field { hub, second: true }),
                ))
                .gap(gap)
                .align(AlignItems::Center)
                .boxed(),
            );
        }
        parts.push(
            buttons(vec![
                ready,
                action(Glyph::Cross, "Cancel", move |world| {
                    drop_form(world, hub);
                }),
                world_menu::send_keys(),
            ])
            .boxed(),
        );
        column(parts).gap(gap).boxed()
    })
    .boxed()
}

/// The form of the action asked for in `hub` about `profile`, where
/// `here` places it.
fn form_view(
    hub: Entity,
    profile: Entity,
    address: String,
    name: String,
    here: fn(&Act) -> bool,
) -> impl View {
    keyed(
        component::<Form, _>(hub, move |form| {
            form.and_then(|form| form.act_on(profile)).cloned()
        }),
        move |act| match act {
            Some(act) if here(act) => {
                form(hub, address.clone(), name.clone(), act.clone())
            }
            _ => column(()).boxed(),
        },
    )
    .within(bare())
}

/// Whether `act` changes the profile for good.
fn is_careful(act: &Act) -> bool {
    matches!(act, Act::Purge(_) | Act::Remove | Act::Erase)
}

/// Whether `act` is done from the foot of the page.
fn is_of_foot(act: &Act) -> bool {
    matches!(act, Act::Remove | Act::Erase)
}

/// Whether `act` is done from the menu of a memory file.
fn is_of_memory(act: &Act) -> bool {
    matches!(act, Act::Purge(_))
}

/// Whether `act` is done from the card of its versions.
fn is_of_versions(act: &Act) -> bool {
    matches!(act, Act::Tag | Act::Fork(Some(_)) | Act::Revert(_))
}

/// Whether `act` is done from the buttons under the profile's name.
fn is_of_header(act: &Act) -> bool {
    matches!(act, Act::Fork(None))
}

/// Where the profile stands, said beside its name.
fn standing(profile: &protocol::Profile) -> (&'static str, Tone) {
    match (&profile.left, profile.arrival) {
        (Some(_), _) => ("Archived", Tone::Dim),
        (None, protocol::Arrival::Housed) => {
            ("In the city", Tone::Dim)
        }
        (None, protocol::Arrival::Waiting) => {
            ("Waits at the harbor", Standing::Waits.words())
        }
        (None, protocol::Arrival::Turned) => {
            ("Turned away at the harbor", Tone::Critical)
        }
        (None, protocol::Arrival::Apart) => {
            ("A blank agent's own", Tone::Dim)
        }
    }
}

/// The actions under its name that change where it stands.
fn arrival(hub: Entity, shown: &Shown) -> AnyView {
    let address = profile_address(shown.profile.id);
    let can = |name| shown.allowed.contains(&name);
    let mut buttons_row = Vec::new();
    if shown.profile.left.is_none() && can("fork") {
        buttons_row.push(
            action(
                Glyph::Profile,
                "New Profile from This One",
                move |world| {
                    ask(world, hub, Act::Fork(None));
                },
            )
            .tooltip(|| {
                note(
                    "Start another profile knowing all this one \
                     knows now",
                )
            })
            .boxed(),
        );
    }
    match (&shown.profile.left, shown.profile.arrival) {
        (Some(_), _) => {
            if can("restore-profile") {
                let address = address.clone();
                buttons_row.push(good_action(
                    Glyph::Restore,
                    "Restore",
                    move |world| {
                        tell_hand(
                            world,
                            format!("/restore-profile {address}"),
                        );
                    },
                ));
            }
        }
        // Moved in the world, or sent back to the harbor.
        (None, protocol::Arrival::Housed) => {
            if can("move-house") {
                let profile = shown.entity;
                buttons_row.push(
                    action(Glyph::Move, "Move House", move |world| {
                        hamlet_realm::orders::carry_house(
                            world, profile,
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
        (None, protocol::Arrival::Apart) => {}
        (None, _) => {
            if can("welcome") {
                let address = address.clone();
                buttons_row.push(good_action(
                    Glyph::Check,
                    "Welcome into the City",
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

/// The versions in their section, each with its menu.
fn versions(hub: Entity, shown: &Shown) -> AnyView {
    let can = |name| shown.allowed.contains(&name);
    let address = profile_address(shown.profile.id);
    let profile = shown.profile.name.clone();
    let mut parts = Vec::new();
    if shown.profile.versions.is_empty() {
        parts.push(note("No version is named yet.").boxed());
    }
    // Newest first.
    for version in shown.profile.versions.iter().rev() {
        let name = version.name.clone();
        let followers = shown
            .agents
            .iter()
            .filter(|agent| {
                agent.pin.as_deref() == Some(name.as_str())
            })
            .map(|agent| agent.called.clone())
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
        let mut actions = Vec::new();
        if can("fork") {
            actions.push((
                format!("Make a new profile from {name}"),
                Act::Fork(Some(name.clone())),
            ));
        }
        if can("revert") {
            actions.push((
                format!("Undo the change saved as {name}"),
                Act::Revert(name.clone()),
            ));
        }
        if !actions.is_empty() {
            line.push(action_menu(
                hub,
                "What can be done from this version",
                actions,
            ));
        }
        parts.push(
            row(line).gap(8.0).align(AlignItems::Center).boxed(),
        );
    }
    let mut acts = Vec::new();
    if can("tag") {
        acts.push(
            action(Glyph::Plus, "Save as a Version", move |world| {
                ask(world, hub, Act::Tag);
            })
            .tooltip(|| note("Name what the profile knows now"))
            .boxed(),
        );
    }
    parts.push(buttons(acts).boxed());
    parts.push(
        form_view(
            hub,
            shown.entity,
            address,
            profile,
            is_of_versions,
        )
        .boxed(),
    );
    section(Glyph::History, "Versions", None, column(parts).gap(4.0))
}

/// The "..." button that opens `actions`.
fn action_menu(
    hub: Entity,
    tip: &'static str,
    actions: Vec<(String, Act)>,
) -> AnyView {
    let (words, acts): (Vec<_>, Vec<_>) = actions.into_iter().unzip();
    with_cx(move |cx| {
        let theme = cx.theme();
        let acts = acts.clone();
        menu_button("", words.clone(), move |world, at| {
            if let Some(act) = acts.get(at) {
                ask(world, hub, act.clone());
            }
        })
        .icon(Glyph::More.image(ICON))
        .padding(UiRect::axes(px(8.0), px(4.0)))
        .fill(theme.palette.button)
        .border(theme.space.border)
        .border_color(theme.palette.mark)
        .radius(theme.space.round)
        .tooltip(move || note(tip))
    })
    .boxed()
}

/// Where an agent stands, its hamlet and parent as links.
fn place_view(place: &Place) -> AnyView {
    let words =
        |text: &'static str| label(text).tone(Tone::Dim).boxed();
    let hamlet = |found: &Option<(Entity, String)>| {
        found.clone().map(|(hamlet, name)| {
            link(name, move |world| open_hamlet(world, hamlet))
        })
    };
    let mut parts = Vec::new();
    match place {
        Place::Lead(found) => {
            parts.push(words("lead of"));
            parts.extend(hamlet(found));
        }
        Place::Under {
            hamlet: found,
            parent,
        } => {
            parts.push(words("in"));
            parts.extend(hamlet(found));
            if let Some((parent, name)) = parent.clone() {
                parts.push(words("under"));
                parts.push(link(name, move |world| {
                    talk_to(world, parent);
                }));
            }
        }
    }
    row(parts).gap(4.0).align(AlignItems::Center).boxed()
}

/// `name` as words that `act` follows when pressed.
fn link(
    name: String,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(label(name))
        .padding(UiRect::all(px(0.0)))
        .variant(Variant::Text)
        .on_activate(act)
        .boxed()
}

/// Its agents in their section, each with its version.
fn agents(shown: &Shown) -> AnyView {
    if shown.agents.is_empty() {
        return section_line(
            Glyph::HamletAgent,
            "Agents",
            dim("Nobody works as it now.").boxed(),
        );
    }
    let mut parts = Vec::new();
    let pins = shown.allowed.contains(&"pin");
    let mut names = vec!["main".to_string()];
    names.extend(
        shown.profile.versions.iter().map(|v| v.name.clone()),
    );
    // `main` is what the profile knows now.
    let words = names
        .iter()
        .map(|name| match name.as_str() {
            "main" => "Latest".to_string(),
            version => version.to_string(),
        })
        .collect::<Vec<_>>();
    for shown_agent in &shown.agents {
        let agent = shown_agent.entity;
        let address = shown_agent.address.clone();
        let choices = names.clone();
        // The version it follows now, main when it is not pinned.
        let now = shown_agent
            .pin
            .as_ref()
            .and_then(|pin| choices.iter().position(|v| v == pin))
            .unwrap_or(0);
        let mut line = vec![
            button(label(shown_agent.called.clone()).bold(true))
                .padding(UiRect::all(px(0.0)))
                .variant(Variant::Text)
                .tooltip({
                    let address = address.clone();
                    move || note(address.clone())
                })
                .on_activate(move |world| talk_to(world, agent))
                .boxed(),
        ];
        if shown_agent.learned {
            let words = learned_words(&shown.profile.name);
            line.push(
                row((learned_mark(),))
                    .tooltip(move || note(words.clone()))
                    .boxed(),
            );
        }
        line.push(place_view(&shown_agent.place));
        line.push(frame().grow(1.0).boxed());
        if pins {
            let picked = choices.clone();
            line.push(
                label("Version")
                    .tone(Tone::Faint)
                    .tooltip(|| {
                        note(
                            "The version of what the profile knows \
                             that this agent works from",
                        )
                    })
                    .boxed(),
            );
            line.push(
                dropdown(
                    words.clone(),
                    Binding::new(now, move |world, chosen: usize| {
                        if chosen != now
                            && let Some(to) = picked.get(chosen)
                        {
                            tell_hand(
                                world,
                                format!("/pin {address} {to}"),
                            );
                        }
                    }),
                    Glyph::Chevron.image(ICON),
                )
                .boxed(),
            );
        }
        parts.push(
            row(line).gap(8.0).align(AlignItems::Center).boxed(),
        );
    }
    section(
        Glyph::HamletAgent,
        "Agents",
        None,
        column(parts).gap(4.0),
    )
}

/// Its character, memory and skills, each in its section.
fn knowledge(hub: Entity, shown: &Shown) -> [AnyView; 3] {
    [
        memory::character_section(hub),
        section(
            Glyph::Notes,
            "Memory",
            None,
            column((
                memory::view(hub, dream(shown), file_menu(shown)),
                form_view(
                    hub,
                    shown.entity,
                    profile_address(shown.profile.id),
                    shown.profile.name.clone(),
                    is_of_memory,
                ),
            ))
            .gap(8.0),
        ),
        memory::skills_section(hub),
    ]
}

/// The dream button, while the profile may have one.
fn dream(
    shown: &Shown,
) -> impl Fn() -> Option<AnyView> + Send + Sync + 'static {
    let may = shown.profile.left.is_none()
        && shown.allowed.contains(&"dream");
    let address = profile_address(shown.profile.id);
    move || {
        let address = address.clone();
        may.then(|| {
            action(
                Glyph::Rename,
                "Note What Its Agents Learned",
                move |world| {
                    tell_hand(world, format!("/dream {address}"));
                },
            )
            .tooltip(|| {
                note(
                    "Brings home what its agents learned, and \
                     tidies its memory",
                )
            })
            .boxed()
        })
    }
}

/// A memory file's menu, while one may be purged.
fn file_menu(shown: &Shown) -> memory::FileMenu {
    let may = shown.profile.left.is_none()
        && shown.allowed.contains(&"purge");
    may.then_some(erase_menu)
}

/// The menu that purges `path` from the history of the profile `hub`
/// shows.
fn erase_menu(hub: Entity, path: &str) -> AnyView {
    action_menu(
        hub,
        "What can be done to this file",
        vec![(
            "Erase from Every Version".to_string(),
            // The memory's paths are inside its folder.
            Act::Purge(format!("memory/{path}")),
        )],
    )
}

/// What changes the profile for good, at the foot of its page.
fn careful(hub: Entity, shown: &Shown) -> AnyView {
    let name = shown.profile.name.clone();
    let address = profile_address(shown.profile.id);
    let can = |name| shown.allowed.contains(&name);
    let living = shown.profile.left.is_none();
    let mut acts = Vec::new();
    if living
        && shown.profile.arrival == protocol::Arrival::Housed
        && can("turn-away")
    {
        let address = address.clone();
        acts.push(
            action(
                Glyph::Anchor,
                "Send to the Harbor",
                move |world| {
                    tell_hand(world, format!("/turn-away {address}"));
                },
            )
            .tooltip(|| {
                note(
                    "Takes its house out of the city. It waits at \
                     the harbor until it is welcomed back",
                )
            })
            .boxed(),
        );
    }
    if living && can("remove-profile") {
        acts.push(bad_action(Glyph::Gone, "Remove", move |world| {
            ask(world, hub, Act::Remove);
        }));
    }
    if !living && can("erase-profile") {
        acts.push(bad_action(
            Glyph::Delete,
            "Erase for Good",
            move |world| {
                ask(world, hub, Act::Erase);
            },
        ));
    }
    if acts.is_empty() {
        return frame().boxed();
    }
    let form =
        form_view(hub, shown.entity, address, name, is_of_foot);
    section(
        Glyph::Stop,
        "Careful",
        None,
        column((buttons(acts), form)).gap(4.0),
    )
}

/// The profile's page in `hub`.
fn body(hub: Entity, shown: &Shown) -> AnyView {
    let profile = shown.entity;
    let name = shown.profile.name.clone();
    let address = profile_address(shown.profile.id);
    let (stands, tone) = standing(&shown.profile);
    let standing = label(stands).tone(tone);
    let parts = (
        arrival(hub, shown),
        agents(shown),
        knowledge(hub, shown),
        versions(hub, shown),
        careful(hub, shown),
    );
    with_cx(move |cx| {
        let (
            head,
            agents_part,
            [character_part, memory_part, skills_part],
            versions_part,
            careful_part,
        ) = parts;
        let section = cx.theme().space.section;
        // The head stays at the top while the rest scrolls under it.
        column((
            // The path names it; where it stands is beside the path.
            beside((
                crumbs(vec![
                    crumb("Realm", move |world| {
                        show(world, hub, Showing::Realm);
                    })
                    .boxed(),
                    crumb_slash().boxed(),
                    crumb("City", move |world| {
                        show(world, hub, Showing::Realm);
                        // The city is the rail's tab that lists it.
                        rail::turn(world, hub, rail::Tab::City);
                    })
                    .boxed(),
                    crumb_slash().boxed(),
                    crate::naming::name_crumb(profile, name.clone()),
                ]),
                standing,
            )),
            scroll_area((column((
                head,
                form_view(
                    hub,
                    profile,
                    address,
                    name.clone(),
                    is_of_header,
                ),
                column((
                    agents_part,
                    character_part,
                    memory_part,
                    skills_part,
                    versions_part,
                    careful_part,
                ))
                .gap(0.0),
            ))
            .gap(section),)),
        ))
        .gap(section)
        .fills_height()
    })
    .boxed()
}

/// Does the action the form of the profile `hub` shows asks for.
pub(super) fn submit(world: &mut World, hub: Entity) {
    let Some(Showing::Profile(profile)) = super::showing(world, hub)
    else {
        return;
    };
    let Some(act) = world
        .get::<Form>(hub)
        .and_then(|form| form.act_on(profile))
        .cloned()
    else {
        return;
    };
    let Some(shown) = shown(world, profile) else {
        return;
    };
    let address = profile_address(shown.profile.id);
    run(world, hub, &address, &shown.profile.name, &act);
}

/// Whether an action of the profile `hub` shows waits for its words.
pub(super) fn asking(world: &World, hub: Entity) -> bool {
    let Some(Showing::Profile(profile)) = super::showing(world, hub)
    else {
        return false;
    };
    world
        .get::<Form>(hub)
        .is_some_and(|form| form.act_on(profile).is_some())
}

/// The page of `profile` in `hub`.
pub(super) fn page(hub: Entity, profile: Entity) -> impl View {
    keyed(
        derived(move |world: &World| shown(world, profile))
            .when(every_frame()),
        move |shown| match shown {
            Some(shown) => body(hub, shown),
            None => dim("That profile is not in the city.").boxed(),
        },
    )
    .within(filling())
}

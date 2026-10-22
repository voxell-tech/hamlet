//! Every hamlet, with a search over them, the way to found one, and
//! those closed down.

use agents_ui::{Confirm, ask, confirm, either, moved};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, ClosedHamlet, Roster, RosterHamlet, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, World, button, column, derived, field,
    heading, icon, icon_button, keyed, label, list, menu_item,
    pressable, resource, row, scroll,
};

use crate::hamlet::{HOME, close_down};
use crate::{select, with};

/// The words typed to search the hamlets.
#[derive(Default)]
struct Search(String);

/// The form a hamlet is founded with, while it is open.
#[derive(Default, Clone, PartialEq)]
struct Founding {
    open: bool,
    name: String,
    mayor: String,
    purpose: String,
}

impl Founding {
    /// The chief's command that founds it. None until all three are
    /// given.
    fn command(&self) -> Option<String> {
        // A name is lowercase letters and hyphens.
        let named = |text: &str| {
            text.trim()
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
        };
        let (name, mayor) = (named(&self.name), named(&self.mayor));
        let purpose = self.purpose.trim();
        (!name.is_empty() && !mayor.is_empty() && !purpose.is_empty())
            .then(|| format!("/found {name} {mayor} {purpose}"))
    }
}

pub(crate) fn prepare(world: &mut World) {
    world.init_resource::<Search>();
    world.init_resource::<Founding>();
}

/// Sends `text` to the open conversation.
pub(crate) fn tell(world: &mut World, text: String) {
    if let Some(to) = with(world) {
        ask(world, AgentsRequest::Send { to, text });
    }
}

/// The icon, the words and the tone of a hamlet's state.
pub(crate) fn standing(
    hamlet: &RosterHamlet,
) -> (&'static str, Tone) {
    match (hamlet.waiting, hamlet.working) {
        (0, 0) => ("moon", Tone::Dim),
        (0, _) => ("bolt", Tone::Accent),
        _ => ("bell", Tone::Critical),
    }
}

/// A row that can be pressed: a title with its state beside it, over
/// dim lines.
pub(crate) fn pressed_card(
    title: String,
    beside: (&'static str, String, Tone),
    lines: Vec<String>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let lines = lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .map(|line| {
            label(line).size(Size::Small).tone(Tone::Dim).boxed()
        })
        .collect::<Vec<_>>();
    pressable((column((
        row((
            label(title).bold(true).face(Face::Display),
            icon(beside.0).tone(beside.2),
            label(beside.1).size(Size::Small).tone(beside.2),
        ))
        .space(Space::Half),
        column(lines).space(Space::None),
    ))
    .space(Space::Half),))
    .on_click(act)
    .boxed()
}

fn hamlet_card(hamlet: &RosterHamlet) -> AnyView {
    let (mark, tone) = standing(hamlet);
    let mut lines = vec![
        format!("Mayor: {}", hamlet.mayor),
        hamlet.account.clone(),
    ];
    if hamlet.gone > 0 {
        lines.push(format!("{} gone", hamlet.gone));
    }
    let (id, closed) = (hamlet.id.clone(), hamlet.id.clone());
    // The first hamlet is the home itself and stays.
    let mut items = Vec::new();
    if hamlet.id != HOME {
        items.push(
            menu_item(
                format!("Close Down {}...", hamlet.title),
                move |world| close_down(world, &closed),
            )
            .critical(),
        );
    }
    pressed_card(
        hamlet.title.to_uppercase(),
        (mark, hamlet.state.clone(), tone),
        lines,
        move |world| select(world, Selected::Hamlet(id.clone())),
    )
    .menu(items)
    .boxed()
}

/// The hamlets the search finds.
fn found(world: &World) -> Vec<RosterHamlet> {
    let search = world.resource::<Search>().0.trim().to_lowercase();
    world
        .resource::<Roster>()
        .hamlets
        .iter()
        .filter(|hamlet| {
            search.is_empty()
                || hamlet.title.to_lowercase().contains(&search)
                || hamlet.id.contains(&search)
                || hamlet.mayor.contains(&search)
        })
        .cloned()
        .collect()
}

/// The hamlets that were closed down, each to be brought back.
fn closed(names: &[ClosedHamlet]) -> AnyView {
    if names.is_empty() {
        return column(()).boxed();
    }
    let mut parts = vec![
        label(format!("CLOSED DOWN ({})", names.len()))
            .bold(true)
            .face(Face::Display)
            .tone(Tone::Faint)
            .boxed(),
    ];
    parts.extend(names.iter().map(|closed| {
        let (back, gone) = (closed.name.clone(), closed.name.clone());
        row((
            label(closed.title.to_uppercase())
                .bold(true)
                .face(Face::Display),
            icon_button("restore", "RESTORE")
                .style(ButtonStyle::Primary)
                .on_click(move |world| {
                    tell(world, format!("/restore-hamlet {back}"))
                }),
            icon_button("bin", "DELETE")
                .style(ButtonStyle::Critical)
                .on_click(move |world| erase(world, &gone)),
        ))
        .space(Space::Gap)
        .boxed()
    }));
    column(parts).space(Space::Half).boxed()
}

/// Asks before everything archived of the hamlet `name` is deleted.
fn erase(world: &mut World, name: &str) {
    let Some(to) = with(world) else {
        return;
    };
    confirm(
        world,
        Confirm {
            question: format!(
                "DELETE {} FOR GOOD?",
                name.to_uppercase()
            ),
            outcome: format!(
                "Everything archived of {name} is deleted from \
                 disk. This cannot be undone."
            ),
            working: Vec::new(),
            verb: "DELETE".into(),
            then: vec![AgentsRequest::Send {
                to,
                text: format!("/erase-hamlet {name}"),
            }],
        },
    );
}

/// One line of the founding form: its question, and its field.
fn asks(
    what: &'static str,
    read: fn(&Founding) -> &String,
    write: fn(&mut Founding, String),
) -> AnyView {
    column((
        label(what).size(Size::Small).tone(Tone::Dim),
        field(resource::<Founding, _>(move |founding| {
            read(founding).clone()
        }))
        .on_commit(move |world, text| {
            write(world.resource_mut::<Founding>(), text)
        }),
    ))
    .space(Space::None)
    .boxed()
}

/// The form a hamlet is founded with, while it is open.
fn founding() -> AnyView {
    keyed(resource::<Founding, _>(|founding| founding.open), |open| {
        if !*open {
            return column(()).boxed();
        }
        column((
            label("FOUND A HAMLET").bold(true).face(Face::Display),
            label(
                "It is built on the next free ground and gets its \
                 own mayor.",
            )
            .size(Size::Small)
            .tone(Tone::Dim),
            asks("Its name", |f| &f.name, |f, text| f.name = text),
            asks(
                "Its mayor's name",
                |f| &f.mayor,
                |f, text| f.mayor = text,
            ),
            asks(
                "What it is for",
                |f| &f.purpose,
                |f, text| f.purpose = text,
            ),
            label(resource::<Founding, _>(|founding| match founding
                .command()
            {
                Some(_) => "Names are kept as lowercase letters and \
                            hyphens."
                    .to_string(),
                None => "All three are needed.".to_string(),
            }))
            .size(Size::Small)
            .tone(Tone::Dim),
            row((
                button("FOUND").style(ButtonStyle::Primary).on_click(
                    |world| {
                        let Some(command) =
                            world.resource::<Founding>().command()
                        else {
                            return;
                        };
                        world.insert(Founding::default());
                        tell(world, command);
                    },
                ),
                button("CANCEL").style(ButtonStyle::Quiet).on_click(
                    |world| {
                        world.insert(Founding::default());
                    },
                ),
            ))
            .space(Space::Half),
        ))
        .space(Space::Half)
        .boxed()
    })
    .boxed()
}

/// Every hamlet, with a search over them.
pub(crate) fn overview() -> AnyView {
    let shown = keyed(
        derived(found)
            .when(either(moved::<Roster>(), moved::<Search>())),
        |hamlets| {
            if hamlets.is_empty() {
                return label("No hamlet by that name.")
                    .tone(Tone::Dim)
                    .boxed();
            }
            list(hamlets.iter().map(hamlet_card).collect::<Vec<_>>())
                .boxed()
        },
    );
    let gone = keyed(
        resource::<Roster, _>(|roster| roster.closed.clone()),
        |names| closed(names),
    );
    column((
        row((
            heading("HAMLETS"),
            label("Click a hamlet to see who lives there.")
                .tone(Tone::Dim),
        ))
        .space(Space::Gap),
        row((
            field(resource::<Search, _>(|search| search.0.clone()))
                .on_commit(|world, text| {
                    world.resource_mut::<Search>().0 = text
                }),
            icon_button("plus", "FOUND A HAMLET").on_click(|world| {
                world.resource_mut::<Founding>().open = true;
            }),
        ))
        .space(Space::Half),
        label("Search by a hamlet's name or its mayor's.")
            .size(Size::Small)
            .tone(Tone::Dim),
        founding(),
        scroll((shown, gone)),
    ))
    .space(Space::Gap)
    .boxed()
}

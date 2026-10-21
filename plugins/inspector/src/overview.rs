//! Every hamlet, with a search over them, the way to found one, and
//! those closed down.

use agents_ui::{AgentsWorldExt, Confirm, Focused, either, moved};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, ClosedHamlet, Roster, RosterHamlet, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, World, column, derived, field, heading,
    icon, icon_button, keyed, label, list, menu_item, pressable,
    resource, row, scroll,
};

use crate::hamlet::{HOME, close_down};
use crate::{founding, select};

/// The words typed to search the hamlets.
#[derive(Default)]
pub(crate) struct Search(pub(crate) String);

pub(crate) fn prepare(world: &mut World) {
    world.init_resource::<Search>();
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
                    world.tell(format!("/restore-hamlet {back}"))
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
    let Some(to) = world.resource::<Focused>().with() else {
        return;
    };
    world.confirm(Confirm {
        question: format!("DELETE {} FOR GOOD?", name.to_uppercase()),
        outcome: format!(
            "Everything archived of {name} is deleted from disk. \
             This cannot be undone."
        ),
        working: Vec::new(),
        verb: "DELETE".into(),
        then: vec![AgentsRequest::Send {
            to,
            text: format!("/erase-hamlet {name}"),
        }],
    });
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
            icon_button("plus", "FOUND A HAMLET")
                .on_click(|_| founding::open()),
        ))
        .space(Space::Half),
        label("Search by a hamlet's name or its mayor's.")
            .size(Size::Small)
            .tone(Tone::Dim),
        scroll((shown, gone)),
        row((
            icon_button("flame", "YOUR HEARTH")
                .on_click(|world| select(world, Selected::Hearth)),
            label("Where you live, and how you look.")
                .tone(Tone::Dim),
        ))
        .space(Space::Gap),
    ))
    .space(Space::Gap)
    .boxed()
}

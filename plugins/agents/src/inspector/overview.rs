//! Every hamlet, with a search over them, the way to found one, and
//! those closed down.

use hamlet_engine_bevy::api::agents::{
    AgentsRequest, ClosedHamlet, RosterHamlet, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, column, derived, description, field,
    heading, icon, icon_button, keyed, label, menu_item, pressable,
    resource, row, scroll,
};
use serde::{Deserialize, Serialize};

use crate::inspector::hamlet::{HOME, close_down};
use crate::inspector::{HAMLETS, founding, select};
use crate::shared::{
    AgentsWorldExt, Confirm, Focused, Rostered, either, link_row,
    link_rows, moved,
};

/// The search is kept across a reload.
pub(crate) fn plugin(app: &mut App) {
    app.init_saved_resource::<Search>();
}

/// The words typed to search the hamlets.
#[derive(Resource, Default, Serialize, Deserialize)]
pub(crate) struct Search(pub(crate) String);

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

/// A row of a list that leads somewhere: a title with its state
/// beside it, over dim lines.
pub(crate) fn pressed_row(
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
    link_row(
        column((
            row((
                label(title).bold(true).face(Face::Display),
                icon(beside.0).tone(beside.2),
                label(beside.1).size(Size::Small).tone(beside.2),
            ))
            .space(Space::Half),
            column(lines).space(Space::None),
        ))
        .space(Space::Half)
        .boxed(),
    )
    .on_click(act)
    .boxed()
}

fn hamlet_row(hamlet: &RosterHamlet) -> AnyView {
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
    pressed_row(
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
        .resource::<Rostered>()
        .0
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
    let rows = names
        .iter()
        .map(|closed| {
            let (back, gone) =
                (closed.name.clone(), closed.name.clone());
            // A row as the others are, its buttons at its end.
            pressable((
                label(closed.title.to_uppercase())
                    .bold(true)
                    .face(Face::Display),
                row((
                    icon_button("restore", "RESTORE")
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| {
                            world.tell(format!(
                                "/restore-hamlet {back}"
                            ))
                        }),
                    icon_button("bin", "DELETE")
                        .style(ButtonStyle::Critical)
                        .on_click(move |world| erase(world, &gone)),
                ))
                .space(Space::Half),
            ))
            .style(ButtonStyle::Row)
            .boxed()
        })
        .collect::<Vec<_>>();
    parts.push(link_rows(rows));
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
            .when(either(moved::<Rostered>(), moved::<Search>())),
        |hamlets| {
            if hamlets.is_empty() {
                return label("No hamlet by that name.")
                    .tone(Tone::Dim)
                    .boxed();
            }
            link_rows(hamlets.iter().map(hamlet_row).collect())
        },
    );
    let gone = keyed(
        resource::<Rostered, _>(|roster| roster.0.closed.clone()),
        |names| closed(names),
    );
    column((
        row((
            heading(HAMLETS),
            description("Click a hamlet to see who lives there."),
        ))
        .space(Space::Gap),
        row((icon_button("plus", "FOUND A HAMLET")
            .on_click(|_| founding::open()),)),
        field(resource::<Search, _>(|search| search.0.clone()))
            .placeholder("Search hamlets")
            .on_commit(|world, text| {
                world.resource_mut::<Search>().0 = text
            }),
        description("Search by a hamlet's name or its mayor's."),
        scroll((shown, gone)),
        row((
            icon_button("flame", "YOUR HEARTH")
                .on_click(|world| select(world, Selected::Hearth)),
            description("Where you live, and how you look."),
        ))
        .space(Space::Gap),
    ))
    .space(Space::Gap)
    .boxed()
}

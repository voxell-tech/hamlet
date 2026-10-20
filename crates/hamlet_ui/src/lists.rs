//! The library's skills, and the card a row of a list is.

use bevy::ecs::resource::Resource;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{
    ScopedExt, StateExt, ViewExt, each, keyed, resource,
};
use hamlet_client::realm::Skills;

use crate::dock::show_tab;
use crate::icons::Glyph;
use crate::looks::{Fills, action, away, bad_action};
use crate::platform::DisplayText;
use crate::world_menu::{self, Deed};
use crate::{AnyView, Cx, View};

#[derive(Clone, PartialEq)]
pub(crate) struct SkillRow {
    name: String,
    description: String,
}

fn skill_rows(skills: &Skills) -> Vec<SkillRow> {
    skills
        .0
        .iter()
        .map(|s| SkillRow {
            name: s.name.clone(),
            description: s.description.clone(),
        })
        .collect()
}

/// The words of a list while it is empty, and a way to the chat.
fn empty_state<R: Resource>(
    is_empty: fn(&R) -> bool,
    hint: &'static str,
) -> impl View {
    keyed(resource::<R, _>(is_empty), move |empty| {
        if *empty {
            column((
                label(hint).tone(Tone::Dim),
                action(Glyph::Talk, "OPEN CHAT", |w| {
                    show_tab(w, "agent")
                }),
            ))
            .gap(12.0)
            .padding(UiRect::bottom(px(12.0)))
            .align(AlignItems::FlexStart)
            .boxed()
        } else {
            frame().boxed()
        }
    })
    .within(column(()))
}

/// The library's skills, each with the press that takes it out.
/// Whoever shows them scrolls them.
pub(crate) fn skills() -> impl View {
    column((
        empty_state(
            |l: &Skills| l.0.is_empty(),
            "The library is empty. Agents offer skills and the \
             librarian decides which to keep.",
        ),
        each(
            resource::<Skills, _>(skill_rows),
            |r: &SkillRow| format!("{}|{}", r.name, r.description),
            |r| {
                // The press that takes it out is in its card,
                // beside its name, so that what it does has the
                // card's whole width.
                let gone = r.name.clone();
                let out = bad_action(
                    Glyph::Delete,
                    "TAKE OUT",
                    move |world| {
                        world_menu::ask(
                            world,
                            Deed::Unshelve(gone.clone()),
                        )
                    },
                );
                card(
                    r.name.clone(),
                    r.description.clone(),
                    Tone::Body,
                    Some(out),
                )
            },
        )
        .within(column(()).gap(8.0)),
    ))
    .gap(0.0)
}

/// One card: a title over a dim line.
pub fn item(title: String, detail: String, tone: Tone) -> AnyView {
    card(title, detail, tone, None)
}

/// One card: a title, with `beside` it at the card's right when
/// there is something to press, over a dim line the card's whole
/// width.
pub fn card(
    title: String,
    detail: String,
    tone: Tone,
    beside: Option<AnyView>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let name = row((label(title)
            .size(13.0)
            .bold(true)
            .tone(tone)
            .tagged(DisplayText),))
        .fills_width()
        .boxed();
        let head =
            std::iter::once(name).chain(beside).collect::<Vec<_>>();
        cx.build(
            column((
                row(head).gap(8.0).align(AlignItems::Center),
                label(detail).size(13.0).tone(Tone::Dim),
            ))
            .gap(4.0)
            .padding(UiRect::axes(px(14.0), px(10.0)))
            .border(1.0)
            .border_color(cx.theme().palette.brass_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .appear(away)
            .transition(Motion::Expand),
        )
    })
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::v0;
    use hamlet_realm::pick::{Selected, Thing};

    use super::*;
    use crate::me::tests::{panel_app, words};

    #[test]
    fn the_library_shows_the_clients_empty_shelf_and_received_skills()
    {
        let mut app = panel_app();
        app.insert_resource(Selected(Some(Thing::Library)));
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            crate::inspector::screen(),
        );
        app.update();
        assert!(words(&mut app).iter().any(|text| text.starts_with("The library is empty.")));
        app.world_mut().resource_mut::<Skills>().0.push(v0::Skill {
            name: "orchard".into(),
            description: "Tends the trees".into(),
        });
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "orchard"));
        assert!(shown.iter().any(|text| text == "Tends the trees"));
        assert!(!shown.iter().any(|text| text.starts_with("The library is empty.")));
    }
}

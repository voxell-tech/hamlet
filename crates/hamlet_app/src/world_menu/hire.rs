//! The form a scholar of the city is hired with, under a lead: a
//! search over the scholars, the one chosen, and what it is hired to
//! do.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, dropdown, frame, label,
    popup, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{self as agents, Deeds, Scholar};
use hamlet_protocol::client as protocol;
use hamlet_realm::orders;
use hamlet_ui::styles::{
    self, ICON, choice, description, lettered, primary_button,
    scroll_area, section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{View, built};

use super::close;

/// How tall the list of scholars grows before it scrolls.
const LIST: f32 = 220.0;

/// What is typed and chosen in the form.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Hiring {
    search: String,
    chosen: Option<protocol::ScholarId>,
    /// The version it is pinned to; its newest work when None.
    version: Option<String>,
    purpose: String,
}

/// A scholar that can be hired: its id, its name and how many hires
/// it has.
#[derive(Clone, PartialEq)]
struct Choice {
    id: protocol::ScholarId,
    name: String,
    hires: usize,
}

/// The scholars that are not archived whose names hold `search`, by
/// name.
fn choices(world: &World, search: &str) -> Vec<Choice> {
    let Some(mut scholars) = world.try_query::<&Scholar>() else {
        return Vec::new();
    };
    let search = search.trim().to_lowercase();
    let mut found = scholars
        .iter(world)
        .filter(|scholar| scholar.0.left.is_none())
        .filter(|scholar| {
            scholar.0.name.to_lowercase().contains(&search)
        })
        .map(|scholar| Choice {
            id: scholar.0.id,
            name: scholar.0.name.clone(),
            hires: scholar.0.hires.len(),
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| {
        a.name.cmp(&b.name).then(a.id.0.cmp(&b.id.0))
    });
    found
}

/// One scholar of the list, highlighted while it is the one chosen.
fn row_of(choice_of: &Choice, on: bool) -> impl View + use<> {
    let (id, name) = (choice_of.id, choice_of.name.clone());
    let hires = match choice_of.hires {
        0 => "No hires".to_string(),
        1 => "1 hire".to_string(),
        n => format!("{n} hires"),
    };
    button(
        column((
            label(name.clone()).bold(true).wrap(false),
            label(hires).tone(Tone::Dim).wrap(false),
        ))
        .gap(0.0)
        .padding(UiRect::axes(px(0.0), px(4.0)))
        .align(AlignItems::FlexStart),
    )
    .width(percent(100.0))
    .justify(JustifyContent::FlexStart)
    .rules(choice(on))
    .on_activate(move |world| {
        let mut hiring = world.resource_mut::<Hiring>();
        if hiring.chosen != Some(id) {
            hiring.chosen = Some(id);
            hiring.version = None;
        }
    })
}

/// The versions of `scholar`, newest first.
fn versions_of(
    world: &World,
    scholar: protocol::ScholarId,
) -> Vec<String> {
    let Some(mut scholars) = world.try_query::<&Scholar>() else {
        return Vec::new();
    };
    scholars
        .iter(world)
        .find(|s| s.0.id == scholar)
        .map(|s| {
            s.0.versions
                .iter()
                .rev()
                .map(|v| v.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The version the chosen scholar is hired at, while it has named
/// versions to pin.
fn version() -> impl View {
    keyed(
        derived(|world: &World| {
            let hiring = world.resource::<Hiring>();
            let versions = hiring
                .chosen
                .map(|scholar| versions_of(world, scholar))
                .unwrap_or_default();
            (versions, hiring.version.clone())
        })
        .when(every_frame()),
        |(versions, pinned)| {
            if versions.is_empty() {
                return frame().boxed();
            }
            let at = pinned
                .as_ref()
                .and_then(|pinned| {
                    versions.iter().position(|v| v == pinned)
                })
                .map_or(0, |at| at + 1);
            let shown =
                std::iter::once("Its newest work".to_string())
                    .chain(versions.iter().cloned())
                    .collect::<Vec<_>>();
            let versions = versions.clone();
            styles::close((
                section("At version"),
                dropdown(
                    shown,
                    at,
                    Glyph::Chevron.image(ICON),
                    move |world, chosen| {
                        world.resource_mut::<Hiring>().version =
                            chosen
                                .checked_sub(1)
                                .and_then(|at| versions.get(at))
                                .cloned();
                    },
                ),
            ))
            .boxed()
        },
    )
    .within(column(()))
}

/// The list of scholars that match the search.
fn list() -> impl View {
    keyed(
        derived(|world: &World| {
            let hiring = world.resource::<Hiring>();
            (choices(world, &hiring.search), hiring.chosen)
        })
        .when(every_frame()),
        |(found, chosen)| {
            if found.is_empty() {
                return description("No scholar has that name.")
                    .boxed();
            }
            column(
                found
                    .iter()
                    .map(|c| row_of(c, *chosen == Some(c.id)).boxed())
                    .collect::<Vec<_>>(),
            )
            .gap(2.0)
            .boxed()
        },
    )
    .within(column(()))
}

/// The chief's command that hires the one chosen under `lead`, with
/// its purpose if one is given. None until a scholar is chosen.
fn command(world: &World, lead: Entity) -> Option<String> {
    let hiring = world.resource::<Hiring>();
    let scholar = agents::scholar_address(hiring.chosen?);
    let pinned = hiring
        .version
        .as_ref()
        .map(|version| format!("@{version}"))
        .unwrap_or_default();
    let under = agents::name_of(world, lead);
    let command = format!(
        "/hire {scholar}{pinned} under {under} {}",
        hiring.purpose.trim()
    );
    Some(command.trim_end().to_string())
}

/// The form a scholar is hired under `lead` with.
pub(super) fn hire(lead: Entity, at: Vec2) -> impl View {
    built(move |cx| {
        if !Deeds::is_allowed(cx.world, "hire")
            || cx.world.get_entity(lead).is_err()
        {
            return cx.build(frame());
        }
        let under = agents::shown_of(cx.world, lead);
        let (gap, between, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.gap,
                theme.space.section,
                theme.space.overlay_padding,
                theme.text.heading,
            )
        };
        let body = column((
            label(format!("HIRE UNDER {}", under.to_uppercase()))
                .size(heading)
                .bold(true)
                .tagged(DisplayText),
            super::search::search(
                "Search scholars",
                |world| world.resource::<Hiring>().search.clone(),
                |world, text| {
                    world.resource_mut::<Hiring>().search = text;
                },
            ),
            column((scroll_area((list(),)),)).max_height(px(LIST)),
            version(),
            styles::close((
                // Left empty, its first message says it.
                section("What it is hired to do (optional)"),
                text_field(
                    bevy_fynix::resource::<Hiring, _>(|h| {
                        h.purpose.clone()
                    }),
                    |world, text| {
                        world.resource_mut::<Hiring>().purpose = text;
                    },
                )
                .rules(text_input),
            )),
            row((
                lettered("HIRE")
                    .rules(primary_button)
                    .tagged(Variant::Primary)
                    .on_activate(move |world| {
                        if !Deeds::is_allowed(world, "hire") {
                            return;
                        }
                        let Some(command) = command(world, lead)
                        else {
                            return;
                        };
                        close(world);
                        *world.resource_mut::<Hiring>() =
                            Hiring::default();
                        orders::decree(world, lead, command);
                    }),
                lettered("CANCEL")
                    .rules(styles::button)
                    .on_activate(close),
            ))
            .gap(gap),
        ))
        .gap(between)
        .width(px(320.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(UiRect::all(px(padding))),
        )
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;
    use hamlet_testing::realm;

    use super::*;
    use crate::testing::{panel_app, press};

    #[test]
    fn a_scholar_chosen_from_the_list_is_hired_under_the_lead() {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .add_observer(crate::chat::decreed)
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>();
        let mut state = realm::state();
        state.deeds = realm::deeds(&["hire"]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let ada = agents::named(app.world_mut(), "home#0").unwrap();
        let (id, scholar) = app
            .world_mut()
            .query::<&Scholar>()
            .iter(app.world())
            .find(|s| s.0.left.is_none())
            .map(|s| (s.0.id, s.0.name.clone()))
            .expect("a living scholar");
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            hire(ada, Vec2::ZERO),
        );
        app.update();
        press(&mut app, &scholar);
        assert_eq!(app.world().resource::<Hiring>().chosen, Some(id));
        app.world_mut().resource_mut::<Hiring>().purpose =
            "Mend the fences".into();
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        press(&mut app, "HIRE");
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one hire: {sent:?}");
        };
        assert_eq!(
            say.text,
            format!(
                "/hire scholar#{} under home#0 Mend the fences",
                id.0
            )
        );
    }
}

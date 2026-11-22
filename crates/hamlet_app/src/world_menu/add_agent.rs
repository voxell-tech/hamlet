//! The form an agent is added with, under a parent: a blank agent or
//! a profile of the city found by a search, and folded under them the
//! version and what it is added to do, which may be left out.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, popup, row, text_field,
};
use bevy_fynix::{
    Binding, ScopedExt, ViewExt, derived, every_frame, keyed,
};
use hamlet_client::realm::{self as agents, Actions, Profile};
use hamlet_core::{orders, words};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    self, ICON, Looks, choice, description, lettered, note,
    scroll_area, section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, built};

use super::{TakesFocus, close, optional};

/// How tall the list of profiles grows before it scrolls.
const LIST: f32 = 220.0;

/// What is typed and chosen in the form.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct AgentForm {
    search: String,
    /// The profile chosen; a blank agent while None.
    chosen: Option<protocol::ProfileId>,
    /// The version it is pinned to; its newest work when None.
    version: Option<String>,
    purpose: String,
    /// Whether what may be left out is shown.
    more: bool,
}

/// A profile an agent can be added of: its id, its name and how many
/// agents it has.
#[derive(Clone, PartialEq)]
struct Choice {
    id: protocol::ProfileId,
    name: String,
    agents: usize,
}

/// The profiles of the city that are not archived whose names hold
/// `search`, by name.
fn choices(world: &World, search: &str) -> Vec<Choice> {
    let Some(mut profiles) = world.try_query::<&Profile>() else {
        return Vec::new();
    };
    let search = search.trim().to_lowercase();
    let mut found = profiles
        .iter(world)
        .filter(|profile| {
            profile.0.left.is_none()
                && profile.0.arrival != protocol::Arrival::Apart
        })
        .filter(|profile| {
            profile.0.name.to_lowercase().contains(&search)
        })
        .map(|profile| Choice {
            id: profile.0.id,
            name: profile.0.name.clone(),
            agents: profile.0.agents.len(),
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| {
        a.name.cmp(&b.name).then(a.id.0.cmp(&b.id.0))
    });
    found
}

/// A row of the list: its name over a line under it, highlighted
/// while it is the one chosen, and pressed to choose `id`.
fn row_of(
    name: String,
    under: String,
    id: Option<protocol::ProfileId>,
    on: bool,
) -> AnyView {
    // What it is to add a blank agent or an agent of a profile.
    let tip = match id {
        Some(_) => words::EXISTING_PROFILE,
        None => words::BLANK,
    };
    button(
        column((
            label(name).bold(true).wrap(false),
            label(under).tone(Tone::Dim).wrap(false),
        ))
        .gap(0.0)
        .padding(UiRect::axes(px(0.0), px(4.0)))
        .align(AlignItems::FlexStart),
    )
    .width(percent(100.0))
    .justify(JustifyContent::FlexStart)
    .rules(choice(on))
    .tooltip(move || note(tip))
    .on_activate(move |world| {
        let mut form = world.resource_mut::<AgentForm>();
        if form.chosen != id {
            form.chosen = id;
            form.version = None;
        }
    })
    .boxed()
}

/// One profile of the list.
fn profile_row(choice_of: &Choice, on: bool) -> AnyView {
    let agents = match choice_of.agents {
        0 => "No agents".to_string(),
        1 => "1 agent".to_string(),
        n => format!("{n} agents"),
    };
    row_of(choice_of.name.clone(), agents, Some(choice_of.id), on)
}

/// The versions of `profile`, newest first.
fn versions_of(
    world: &World,
    profile: protocol::ProfileId,
) -> Vec<String> {
    let Some(mut profiles) = world.try_query::<&Profile>() else {
        return Vec::new();
    };
    profiles
        .iter(world)
        .find(|s| s.0.id == profile)
        .map(|s| {
            s.0.versions
                .iter()
                .rev()
                .map(|v| v.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The version the chosen profile is added at, while it has named
/// versions to pin.
fn version() -> impl View {
    keyed(
        derived(|world: &World| {
            let form = world.resource::<AgentForm>();
            let versions = form
                .chosen
                .map(|profile| versions_of(world, profile))
                .unwrap_or_default();
            (versions, form.version.clone())
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
                    Binding::new(at, move |world, chosen: usize| {
                        world.resource_mut::<AgentForm>().version =
                            chosen
                                .checked_sub(1)
                                .and_then(|at| versions.get(at))
                                .cloned();
                    }),
                    Glyph::Chevron.image(ICON),
                ),
            ))
            .boxed()
        },
    )
    .within(column(()))
}

/// A blank agent, then the profiles that match the search.
fn list() -> impl View {
    keyed(
        derived(|world: &World| {
            let form = world.resource::<AgentForm>();
            (choices(world, &form.search), form.chosen)
        })
        .when(every_frame()),
        |(found, chosen)| {
            let mut rows = vec![row_of(
                "A blank agent".into(),
                "Starts fresh, for this job alone".into(),
                None,
                chosen.is_none(),
            )];
            rows.extend(
                found
                    .iter()
                    .map(|c| profile_row(c, *chosen == Some(c.id))),
            );
            column(rows).gap(2.0).boxed()
        },
    )
    .within(column(()))
}

/// The chief's command that adds an agent of the one chosen under
/// `parent`, a blank agent while none is, with its purpose if one is
/// given.
fn command(world: &World, parent: Entity) -> String {
    let form = world.resource::<AgentForm>();
    let profile = match form.chosen {
        Some(profile) => {
            let pinned = form
                .version
                .as_ref()
                .map(|version| format!("@{version}"))
                .unwrap_or_default();
            format!("{}{pinned}", agents::profile_address(profile))
        }
        None => "blank".into(),
    };
    let under = agents::name_of(world, parent);
    let command = format!(
        "/add-agent {profile} under {under} {}",
        form.purpose.trim()
    );
    command.trim_end().to_string()
}

/// Adds an agent of the one the form chose under `parent`.
pub(super) fn submit(world: &mut World, parent: Entity) {
    if !Actions::is_allowed(world, "add-agent")
        || world.get_entity(parent).is_err()
    {
        return;
    }
    let command = command(world, parent);
    close(world);
    *world.resource_mut::<AgentForm>() = AgentForm::default();
    orders::decree(world, parent, command);
}

/// The form an agent is added under `parent` with.
pub(super) fn form(parent: Entity, at: Vec2) -> impl View {
    built(move |cx| {
        if !Actions::is_allowed(cx.world, "add-agent")
            || cx.world.get_entity(parent).is_err()
        {
            return cx.build(frame());
        }
        let under = agents::shown_of(cx.world, parent);
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
            label(format!("Add Agent Under {}", under))
                .size(heading)
                .bold(true),
            hamlet_ui::widgets::search::search(
                "Search profiles",
                String::new(),
                |world, text| {
                    world.resource_mut::<AgentForm>().search = text;
                },
            ),
            column((scroll_area((list(),)),)).max_height(px(LIST)),
            optional::<AgentForm>(
                |f| f.more,
                |f| f.more = !f.more,
                || {
                    column((
                        version(),
                        styles::close((
                            // Left empty, its first message says it.
                            section("What it is added to do"),
                            text_field(Binding::new(
                                bevy_fynix::resource::<AgentForm, _>(
                                    |f| f.purpose.clone(),
                                ),
                                |world, text| {
                                    world
                                        .resource_mut::<AgentForm>()
                                        .purpose = text;
                                },
                            ))
                            .rules(text_input)
                            .tagged(TakesFocus),
                        )),
                        description(
                            "Left empty, its first message says it.",
                        ),
                    ))
                    .gap(8.0)
                    .boxed()
                },
            ),
            row((
                lettered("Add Agent")
                    .variant(Variant::Primary)
                    .on_activate(move |world| submit(world, parent)),
                lettered("Cancel")
                    .rules(styles::button)
                    .on_activate(close),
                super::send_keys(),
            ))
            .gap(gap)
            .align(AlignItems::Center),
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
    fn a_profile_chosen_from_the_list_is_added_under_the_parent() {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .add_observer(crate::chat::decreed)
            .init_resource::<hamlet_core::pick::Asked>()
            .init_resource::<hamlet_core::pick::AskedGround>();
        let mut state = realm::state();
        state.actions = realm::actions(&["add-agent"]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let ada = agents::named(app.world_mut(), "agent#2").unwrap();
        let (id, profile) = app
            .world_mut()
            .query::<&Profile>()
            .iter(app.world())
            .find(|s| s.0.left.is_none())
            .map(|s| (s.0.id, s.0.name.clone()))
            .expect("a living profile");
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            form(ada, Vec2::ZERO),
        );
        app.update();
        press(&mut app, &profile);
        assert_eq!(
            app.world().resource::<AgentForm>().chosen,
            Some(id)
        );
        app.world_mut().resource_mut::<AgentForm>().purpose =
            "Mend the fences".into();
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        press(&mut app, "Add Agent");
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one agent added: {sent:?}");
        };
        assert_eq!(
            say.text,
            format!(
                "/add-agent profile#{} under agent#2 Mend the fences",
                id.0
            )
        );
    }
}

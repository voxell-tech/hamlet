//! The form a hamlet is founded with: its name and the folder it
//! works on, and folded under them its mayor and what it is for,
//! which may be left out. Once founded, its mayor's chat opens.

use std::path::PathBuf;

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::{FromWorld, World};
use bevy::math::{Rect, Vec2};
use bevy::time::Time;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, frame, label, popup,
    row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{
    self, Deeds, HamletId, Scholar, scholar_address,
};
use hamlet_desktop::folder::{ChooseFolder, FolderChosen};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, Looks, action, button, description, lettered, section,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{View, built};

use super::{asks, asks_first, close, optional};
use crate::hub::{Showing, show};
use crate::{chat, words};

/// How long a founding is waited for before its folder and chat are
/// let go, in seconds.
const WAIT: f64 = 60.0;

/// The name of a hamlet being founded, the folder it works on, the
/// scholar that is its mayor, and its purpose.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Founding {
    name: String,
    folder: Option<PathBuf>,
    /// A blank agent while None.
    mayor: Option<protocol::ScholarId>,
    purpose: String,
    /// Whether what may be left out is shown.
    more: bool,
}

/// What the folder dialog of the form answers to.
#[derive(Resource)]
pub(crate) struct FolderAsker(Entity);

impl FromWorld for FolderAsker {
    fn from_world(world: &mut World) -> Self {
        Self(world.spawn_empty().id())
    }
}

/// A hamlet founded from the form and not yet seen: its name, the
/// hamlets that had that name already, the folder it is to work on,
/// and when it is let go.
#[derive(Resource, Default)]
pub(crate) struct Awaited(
    Option<(String, Vec<Entity>, Option<PathBuf>, f64)>,
);

impl Founding {
    /// The name it is founded under: what was typed, on one line,
    /// with no colon, since a colon starts the purpose.
    fn name(&self) -> String {
        self.name
            .replace(':', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The chief's command that founds it. None until its name is
    /// given.
    fn command(&self) -> Option<String> {
        let name = self.name();
        let purpose = self.purpose.trim();
        // A blank mayor is said, so a name that starts with a word
        // the command reads as a mayor stays whole.
        let mayor = self
            .mayor
            .map_or_else(|| "blank".to_string(), scholar_address);
        (!name.is_empty()).then(|| match purpose {
            "" => format!("/create-hamlet {mayor} {name}"),
            _ => format!("/create-hamlet {mayor} {name}: {purpose}"),
        })
    }
}

/// The scholars of the city that can be the mayor: living ones, by
/// name.
fn candidates(world: &World) -> Vec<(protocol::ScholarId, String)> {
    let Some(mut scholars) = world.try_query::<&Scholar>() else {
        return Vec::new();
    };
    let mut found = scholars
        .iter(world)
        .filter(|scholar| {
            scholar.0.left.is_none()
                && scholar.0.arrival != protocol::Arrival::Apart
        })
        .map(|scholar| (scholar.0.id, scholar.0.name.clone()))
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.0.cmp(&b.0.0)));
    found
}

/// Who is to be the mayor: a blank agent, or one of the scholars.
fn mayor() -> impl View {
    keyed(
        derived(|world: &World| {
            (candidates(world), world.resource::<Founding>().mayor)
        })
        .when(every_frame()),
        |(found, chosen)| {
            let shown = std::iter::once("A blank agent".to_string())
                .chain(found.iter().map(|(_, name)| name.clone()))
                .collect::<Vec<_>>();
            let at = chosen
                .and_then(|chosen| {
                    found.iter().position(|(id, _)| *id == chosen)
                })
                .map_or(0, |at| at + 1);
            let found = found.clone();
            // What it is to have a blank agent or a scholar as mayor.
            let meaning = match chosen {
                Some(_) => words::HIRED_SCHOLAR,
                None => words::BLANK,
            };
            hamlet_ui::styles::close((
                section("Its mayor"),
                description(meaning),
                dropdown(
                    shown,
                    at,
                    Glyph::Chevron.image(ICON),
                    move |world, picked| {
                        world.resource_mut::<Founding>().mayor =
                            picked
                                .checked_sub(1)
                                .and_then(|at| found.get(at))
                                .map(|(id, _)| *id);
                    },
                ),
            ))
            .boxed()
        },
    )
    .within(column(()))
}

/// Founds the hamlet the form holds, once it is named.
pub(super) fn submit(world: &mut World) {
    if !Deeds::is_allowed(world, "create-hamlet") {
        return;
    }
    let founding = world.resource::<Founding>().clone();
    let Some(command) = founding.command() else {
        return;
    };
    close(world);
    *world.resource_mut::<Founding>() = Founding::default();
    // Names may repeat, so the new hamlet is the one of its name that
    // was not there before.
    let name = founding.name();
    let before = named_hamlets(world, &name)
        .into_iter()
        .map(|(hamlet, _)| hamlet)
        .collect();
    let until = now(world) + WAIT;
    world.resource_mut::<Awaited>().0 =
        Some((name, before, founding.folder, until));
    chat::command(world, command);
}

/// The hamlets named `name`, each with its id.
fn named_hamlets(world: &World, name: &str) -> Vec<(Entity, String)> {
    let Some(mut hamlets) =
        world.try_query::<(Entity, &HamletId, &Name)>()
    else {
        return Vec::new();
    };
    hamlets
        .iter(world)
        .filter(|(_, _, shown)| shown.as_str() == name)
        .map(|(hamlet, id, _)| (hamlet, id.0.clone()))
        .collect()
}

/// The seconds since the window started.
fn now(world: &World) -> f64 {
    world
        .get_resource::<Time>()
        .map_or(0.0, Time::elapsed_secs_f64)
}

/// Puts the folder chosen in the form's dialog in the form.
pub(crate) fn chosen(
    chosen: On<FolderChosen>,
    mut commands: Commands,
) {
    let FolderChosen { asker, folder } = chosen.event().clone();
    commands.queue(move |world: &mut World| {
        if world.resource::<FolderAsker>().0 == asker {
            world.resource_mut::<Founding>().folder = Some(folder);
        }
    });
}

/// Once the hamlet founded from the form is there, points it at its
/// folder and opens its mayor's chat.
pub(crate) fn open_founded(world: &mut World) {
    let Some((name, before, _, until)) =
        &world.resource::<Awaited>().0
    else {
        return;
    };
    if now(world) > *until {
        world.resource_mut::<Awaited>().0 = None;
        return;
    }
    let Some((_, id)) = named_hamlets(world, name)
        .into_iter()
        .find(|(hamlet, _)| !before.contains(hamlet))
    else {
        return;
    };
    let Some(mayor) = realm::mayor_of(world, &id) else {
        return;
    };
    let Some((_, _, folder, _)) =
        world.resource_mut::<Awaited>().0.take()
    else {
        return;
    };
    if let Some(folder) = folder {
        chat::decree(
            world,
            &id,
            format!("/folder {}", folder.display()),
        );
    }
    crate::convo::seat(world, mayor);
    show(world, Showing::Thread);
}

/// The folder the hamlet is to work on, and the way to choose it.
fn folder() -> impl View {
    keyed(resource::<Founding, _>(|f| f.folder.clone()), |folder| {
        let choose = action(
            Glyph::Folder,
            match folder {
                Some(_) => "Choose another",
                None => "Choose a folder",
            },
            |world| {
                let asker = world.resource::<FolderAsker>().0;
                world.trigger(ChooseFolder {
                    asker,
                    title: "Choose the folder this hamlet works on"
                        .into(),
                });
            },
        );
        let chosen = match folder {
            Some(folder) => label(folder.display().to_string()),
            None => label("None yet").tone(Tone::Dim),
        };
        hamlet_ui::styles::close((
            section("The folder it works on"),
            row((
                choose,
                row((chosen,)).grow(1.0).min_width(px(0.0)),
            ))
            .gap(8.0)
            .align(AlignItems::Center),
        ))
        .boxed()
    })
    .within(column(()))
}

/// The form a hamlet is founded with.
pub(super) fn founding(at: Vec2) -> impl View {
    built(move |cx| {
        if !Deeds::is_allowed(cx.world, "create-hamlet") {
            return cx.build(frame());
        }
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
            label("Create a hamlet").size(heading).bold(true),
            description(
                "It is built on the next free ground and gets its \
                 own mayor.",
            ),
            asks_first::<Founding>(
                "Its name",
                |f| &f.name,
                |f, text| f.name = text,
            ),
            description(resource::<Founding, _>(|f| {
                match f.command() {
                    Some(_) => String::new(),
                    None => "Its name is needed.".to_string(),
                }
            })),
            folder(),
            optional::<Founding>(
                |f| f.more,
                |f| f.more = !f.more,
                || {
                    column((
                        mayor(),
                        asks::<Founding>(
                            "What it is for",
                            |f| &f.purpose,
                            |f, text| f.purpose = text,
                        ),
                    ))
                    .gap(8.0)
                    .boxed()
                },
            ),
            row((
                lettered("Create")
                    .variant(Variant::Primary)
                    .on_activate(submit),
                lettered("Cancel").rules(button).on_activate(close),
                super::send_keys(),
            ))
            .gap(gap)
            .align(AlignItems::Center),
        ))
        .gap(between)
        .width(px(300.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(bevy::ui::UiRect::all(px(padding))),
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
    fn founding_goes_to_the_hand_even_from_another_chat() {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>();
        let mut state = realm::state();
        state.deeds = realm::deeds(&["create-hamlet"]);
        state.agents.push(realm::agent(3, "home", false));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let scout =
            hamlet_client::realm::named(app.world_mut(), "hire#3")
                .unwrap();
        crate::convo::talk_to(app.world_mut(), scout);
        app.insert_resource(Founding {
            name: "mill".into(),
            folder: None,
            mayor: Some(protocol::ScholarId(2)),
            purpose: "Grinds flour".into(),
            more: false,
        });
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            founding(Vec2::ZERO),
        );
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        press(&mut app, "Create");
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one founding: {sent:?}");
        };
        assert_eq!(
            say.text,
            "/create-hamlet scholar#2 mill: Grinds flour"
        );
        assert_eq!(say.to, protocol::AgentId(1));
    }
}

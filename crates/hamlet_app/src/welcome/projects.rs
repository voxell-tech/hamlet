//! The projects of other programs brought in (C35): each a hamlet
//! working on its folder, with a hire for each of its conversations.
//! Offered once, when first-run setup ends on a realm with no hamlet,
//! and from Settings.

use std::collections::BTreeSet;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::{FromWorld, World};
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, checkbox, column, frame, label, overlay,
    row,
};
use bevy_fynix::{
    ViewExt, derived, each, every_frame, keyed, resource,
};
use hamlet_client::ViewFolder;
use hamlet_client::imports::{BringIn, FindProjects, Imports};
use hamlet_client::realm::HamletId;
use hamlet_desktop::choose::{Choose, Chosen, What};
use hamlet_import::Project;
use hamlet_ui::styles::{
    self, Looks, description, good_action, lettered, note,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::Welcome;
use super::pages::{footed, over, sheet};

/// The file in the window's folder that says the projects were
/// offered on this machine.
const OFFERED: &str = "projects-offered";
/// The hires a hamlet holds at its first tier, its mayor among them.
const ROOM: usize = 49;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Projects>()
        .init_resource::<Asker>()
        .add_observer(chosen)
        .add_systems(Update, (offer, pick_suggested));
}

/// The card of projects and what is picked on it.
#[derive(Resource, Default)]
pub struct Projects {
    pub open: bool,
    /// The projects picked, by their key.
    picked: BTreeSet<String>,
    /// Every project is listed, not only the suggested ones.
    all: bool,
    /// The suggested ones were picked for the projects found.
    seeded: bool,
}

/// Whom the dialog an export is chosen in answers.
#[derive(Resource)]
struct Asker(Entity);

impl FromWorld for Asker {
    fn from_world(world: &mut World) -> Self {
        Self(world.spawn_empty().id())
    }
}

/// A project as a row shows it.
#[derive(Clone, PartialEq)]
struct Line {
    key: String,
    name: String,
    detail: String,
    picked: bool,
}

/// What names a project among those found.
fn key(project: &Project) -> String {
    project
        .folder
        .clone()
        .unwrap_or_else(|| project.name.clone())
}

/// Opens the card on the projects of this machine.
pub fn open(world: &mut World) {
    *world.resource_mut::<Projects>() = Projects {
        open: true,
        ..Projects::default()
    };
    world.trigger(FindProjects { file: None });
}

fn close(world: &mut World) {
    world.resource_mut::<Projects>().open = false;
    // Offered once: closed, it opens again only from Settings.
    if let Some(folder) = world.get_resource::<ViewFolder>() {
        let _ = std::fs::write(folder.0.join(OFFERED), "");
    }
}

/// Opens the card when first-run setup ends on a realm with no
/// hamlet, unless it was offered on this machine before.
fn offer(
    welcome: Res<Welcome>,
    folder: Option<Res<ViewFolder>>,
    hamlets: Query<(), With<HamletId>>,
    mut was_open: Local<bool>,
    mut commands: Commands,
) {
    let ended = *was_open && !welcome.open && !welcome.adding;
    *was_open = welcome.open && !welcome.adding;
    let offered =
        folder.is_none_or(|folder| folder.0.join(OFFERED).exists());
    if ended && hamlets.is_empty() && !offered {
        commands.queue(open);
    }
}

/// Picks the suggested projects once they are found.
fn pick_suggested(
    imports: Res<Imports>,
    mut projects: ResMut<Projects>,
) {
    if !projects.open || projects.seeded || imports.looking {
        return;
    }
    projects.seeded = true;
    projects.picked = imports
        .projects
        .iter()
        .filter(|project| project.is_suggested())
        .map(key)
        .collect();
}

fn choose(world: &mut World) {
    let asker = world.resource::<Asker>().0;
    world.trigger(Choose {
        asker,
        what: What::Files,
    });
}

fn chosen(
    chosen: On<Chosen>,
    asker: Res<Asker>,
    mut projects: ResMut<Projects>,
    mut commands: Commands,
) {
    if chosen.asker != asker.0 {
        return;
    }
    if let Some(file) = chosen.paths.first() {
        projects.seeded = false;
        projects.all = true;
        commands.trigger(FindProjects {
            file: Some(file.clone()),
        });
    }
}

fn toggle(world: &mut World, key: &str, on: bool) {
    let mut projects = world.resource_mut::<Projects>();
    if on {
        projects.picked.insert(key.to_owned());
    } else {
        projects.picked.remove(key);
    }
}

fn bring(world: &mut World) {
    let picked = world.resource::<Projects>().picked.clone();
    let projects = world
        .resource::<Imports>()
        .projects
        .iter()
        .filter(|project| picked.contains(&key(project)))
        .cloned()
        .collect::<Vec<_>>();
    if !projects.is_empty() {
        world.trigger(BringIn { projects });
    }
}

fn lines(world: &World) -> Vec<Line> {
    let projects = world.resource::<Projects>();
    let home = std::env::home_dir();
    world
        .resource::<Imports>()
        .projects
        .iter()
        .filter(|project| projects.all || project.is_suggested())
        .map(|project| {
            let count = project.conversations.len();
            let mut detail = match count {
                1 => "1 conversation".to_owned(),
                count if count > ROOM => {
                    format!(
                        "The newest {ROOM} of {count} conversations"
                    )
                }
                count => format!("{count} conversations"),
            };
            if let Some(folder) = &project.folder {
                let shown = home
                    .as_deref()
                    .and_then(|home| {
                        std::path::Path::new(folder)
                            .strip_prefix(home)
                            .ok()
                    })
                    .map_or_else(
                        || folder.clone(),
                        |under| format!("~/{}", under.display()),
                    );
                detail.push_str(&format!(" · {shown}"));
            }
            let key = key(project);
            Line {
                picked: projects.picked.contains(&key),
                key,
                name: project.name.clone(),
                detail,
            }
        })
        .collect()
}

fn line_view(line: &Line) -> AnyView {
    let key = line.key.clone();
    let (name, detail) = (line.name.clone(), line.detail.clone());
    row((
        checkbox(line.picked).on_change(move |world, on| {
            toggle(world, &key, on);
        }),
        column((label(name).bold(true).wrap(false), note(detail)))
            .gap(2.0)
            .grow(1.0)
            .min_width(px(0.0)),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
    .boxed()
}

/// What the card says over the list: what it does, or what was
/// brought in.
fn status() -> AnyView {
    keyed(
        derived(|world: &World| {
            let imports = world.resource::<Imports>();
            let shown = lines(world).len();
            let all = imports.projects.len();
            let found = match (&imports.unread, imports.looking) {
                (_, true) => "Looking…".to_owned(),
                (Some(why), false) => why.clone(),
                (None, false) if all == 0 => "No projects found \
                                              from the last 30 days."
                    .to_owned(),
                (None, false) => format!("{shown} of {all} projects"),
            };
            let busy = imports
                .bringing
                .as_ref()
                .map(|name| format!("Bringing in {name}…"));
            let said = imports
                .outcomes
                .iter()
                .map(|outcome| match &outcome.said {
                    Ok(words) => (
                        Tone::Success,
                        format!("{}: {words}", outcome.name),
                    ),
                    Err(why) => (
                        Tone::Critical,
                        format!("{}: {why}", outcome.name),
                    ),
                })
                .collect::<Vec<_>>();
            (found, busy, said)
        })
        .when(every_frame()),
        |(found, busy, said)| {
            let mut shown = vec![note(found.clone()).boxed()];
            if let Some(busy) = busy {
                shown.push(
                    label(busy.clone()).tone(Tone::Dim).boxed(),
                );
            }
            for (tone, words) in said {
                shown.push(label(words.clone()).tone(*tone).boxed());
            }
            column(shown).gap(4.0).boxed()
        },
    )
    .within(column(()))
    .boxed()
}

/// The way between the suggested projects and every one.
fn review() -> AnyView {
    keyed(resource::<Projects, _>(|projects| projects.all), |all| {
        let text = if *all { "Show suggested" } else { "Review all" };
        lettered(text)
            .variant(Variant::Text)
            .on_activate(|world| {
                let mut projects = world.resource_mut::<Projects>();
                projects.all = !projects.all;
            })
            .boxed()
    })
    .within(footed())
    .boxed()
}

fn page() -> AnyView {
    let head = column((
        styles::title("Bring in your projects"),
        description(
            "Each project Claude Code, Codex or pi worked on \
             becomes a hamlet working on its folder, and each of \
             its conversations from the last 30 days a hire that \
             carries it on.",
        ),
    ))
    .gap(4.0);
    let body = column((
        status(),
        each(
            derived(lines).when(every_frame()),
            |line: &Line| line.clone(),
            line_view,
        )
        .within(column(()).gap(12.0)),
    ))
    .gap(12.0);
    let bring = keyed(
        derived(|world: &World| {
            (
                world.resource::<Projects>().picked.len(),
                world.resource::<Imports>().is_busy(),
            )
        })
        .when(every_frame()),
        |(picked, busy)| match (*picked, *busy) {
            (_, true) | (0, _) => frame().boxed(),
            (1, false) => {
                good_action(Glyph::Plus, "Bring in 1 project", bring)
            }
            (_, false) => {
                good_action(Glyph::Plus, "Bring in projects", bring)
            }
        },
    )
    .within(footed())
    .boxed();
    sheet(
        head,
        body,
        vec![
            review(),
            lettered("From an export file")
                .variant(Variant::Text)
                .on_activate(choose)
                .boxed(),
            lettered("Close")
                .variant(Variant::Text)
                .on_activate(close)
                .boxed(),
            bring,
        ],
    )
}

/// The card, over the window while it is open.
pub fn view() -> impl View {
    keyed(resource::<Projects, _>(|projects| projects.open), |open| {
        if !*open {
            return frame().boxed();
        }
        with_cx(|cx| over(cx, page())).boxed()
    })
    .within(overlay(()))
}

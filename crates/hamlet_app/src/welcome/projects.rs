//! The projects of other programs brought in (C35): each a hamlet
//! working on its folder, with a hire for each conversation picked.
//! Offered once, when first-run setup ends on a realm with no hamlet,
//! and from Settings.

use std::collections::BTreeSet;
use std::time::SystemTime;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::{FromWorld, World};
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, checkbox, column, frame, label,
    overlay, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, each, every_frame, keyed, resource,
};
use hamlet_client::ViewFolder;
use hamlet_client::imports::{BringIn, FindProjects, Imports};
use hamlet_client::realm::HamletId;
use hamlet_desktop::choose::{Choose, Chosen, What};
use hamlet_import::{Found, Project};
use hamlet_ui::styles::{
    self, ICON, Looks, description, good_action, lettered, link_row,
    note, note_rules,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::Welcome;
use super::pages::{footed, over, sheet};

/// The file in the window's folder that says the projects were
/// offered on this machine.
const OFFERED: &str = "projects-offered";
/// The hires a hamlet holds at its first tier, its mayor among them.
const ROOM: usize = 49;
/// How far in a project's conversations stand under it.
const INDENT: f32 = 32.0;

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
    /// The conversations picked, by their key.
    picked: BTreeSet<String>,
    /// The projects whose conversations are listed, by their key.
    unfolded: BTreeSet<String>,
    /// Every project is listed, not only the suggested ones and
    /// those with a conversation picked.
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

/// A row of the list: a project, or one of its conversations under
/// it.
#[derive(Clone, PartialEq)]
enum Line {
    Project {
        key: String,
        name: String,
        detail: String,
        picked: bool,
        unfolded: bool,
    },
    Conversation {
        key: String,
        title: String,
        detail: String,
        picked: bool,
    },
}

/// What names a project among those found.
fn project_key(project: &Project) -> String {
    project
        .folder
        .clone()
        .unwrap_or_else(|| project.name.clone())
}

/// What names a conversation among those found.
fn key(found: &Found) -> String {
    format!("{}:{}", found.source.word(), found.session)
}

/// The newest conversations of `project` a hamlet has room for.
fn newest(project: &Project) -> impl Iterator<Item = String> + '_ {
    project.conversations.iter().take(ROOM).map(key)
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

/// Picks the newest conversations of the suggested projects once they
/// are found.
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
        .flat_map(newest)
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

/// Picks the newest conversations of the project `key` there is room
/// for, or none of them.
fn pick_project(world: &mut World, project: &str, on: bool) {
    let keys = world
        .resource::<Imports>()
        .projects
        .iter()
        .find(|found| project_key(found) == project)
        .map(|found| {
            let all = found
                .conversations
                .iter()
                .map(key)
                .collect::<Vec<_>>();
            let newest = newest(found).collect::<Vec<_>>();
            (all, newest)
        });
    let Some((all, newest)) = keys else {
        return;
    };
    let mut projects = world.resource_mut::<Projects>();
    if on {
        projects.picked.extend(newest);
    } else {
        for key in all {
            projects.picked.remove::<String>(&key);
        }
    }
}

fn pick(world: &mut World, key: &str, on: bool) {
    let mut projects = world.resource_mut::<Projects>();
    if on {
        projects.picked.insert(key.to_owned());
    } else {
        projects.picked.remove(key);
    }
}

fn unfold(world: &mut World, project: &str) {
    let mut projects = world.resource_mut::<Projects>();
    if !projects.unfolded.remove(project) {
        projects.unfolded.insert(project.to_owned());
    }
}

/// Brings in each project with a conversation picked, with those
/// picked alone.
fn bring(world: &mut World) {
    let picked = world.resource::<Projects>().picked.clone();
    let projects = world
        .resource::<Imports>()
        .projects
        .iter()
        .filter_map(|project| {
            let conversations = project
                .conversations
                .iter()
                .filter(|found| picked.contains(&key(found)))
                .cloned()
                .collect::<Vec<_>>();
            (!conversations.is_empty()).then(|| Project {
                conversations,
                ..project.clone()
            })
        })
        .collect::<Vec<_>>();
    if !projects.is_empty() {
        world.trigger(BringIn { projects });
    }
}

/// When `at` was, as a row of a conversation says it.
fn when(at: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%-d %b, %H:%M")
        .to_string()
}

fn lines(world: &World) -> Vec<Line> {
    let projects = world.resource::<Projects>();
    let home = std::env::home_dir();
    let mut lines = Vec::new();
    for project in &world.resource::<Imports>().projects {
        let project_key = project_key(project);
        let picked = project
            .conversations
            .iter()
            .filter(|found| projects.picked.contains(&key(found)))
            .count();
        if !projects.all && !project.is_suggested() && picked == 0 {
            continue;
        }
        let count = project.conversations.len();
        let mut detail = match (picked, count) {
            (_, 1) => format!("{picked} of 1 conversation"),
            (_, count) => {
                format!("{picked} of {count} conversations")
            }
        };
        if picked > ROOM {
            detail.push_str(&format!(", {ROOM} fit"));
        }
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
        let unfolded = projects.unfolded.contains(&project_key);
        lines.push(Line::Project {
            key: project_key,
            name: project.name.clone(),
            detail,
            picked: picked > 0,
            unfolded,
        });
        if !unfolded {
            continue;
        }
        for found in &project.conversations {
            let key = key(found);
            lines.push(Line::Conversation {
                picked: projects.picked.contains(&key),
                key,
                title: found.title.clone(),
                detail: format!(
                    "{} · {}",
                    found.source.name(),
                    when(found.modified)
                ),
            });
        }
    }
    lines
}

fn line_view(line: &Line) -> AnyView {
    match line.clone() {
        Line::Project {
            key,
            name,
            detail,
            picked,
            unfolded,
        } => {
            let picking = key.clone();
            let folding = key;
            // The chevron points up unturned: down while the
            // project's conversations are listed, else left.
            let turn = if unfolded { 180.0 } else { 270.0 };
            row((
                checkbox(picked).on_change(move |world, on| {
                    pick_project(world, &picking, on);
                }),
                button(
                    row((
                        column((
                            one_line(label(name).bold(true)),
                            column((one_line(label(detail)),))
                                .rules(note_rules),
                        ))
                        .gap(2.0)
                        .grow(1.0)
                        .min_width(px(0.0)),
                        glyph(Glyph::Chevron, ICON)
                            .tone(Tone::Dim)
                            .rotation(turn),
                    ))
                    .gap(8.0)
                    .align(AlignItems::Center)
                    .grow(1.0),
                )
                .grow(1.0)
                .min_width(px(0.0))
                .padding(UiRect::all(px(4.0)))
                .rules(link_row)
                .tagged(Variant::Default)
                .on_activate(move |world| unfold(world, &folding)),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .boxed()
        }
        Line::Conversation {
            key,
            title,
            detail,
            picked,
        } => row((
            checkbox(picked).on_change(move |world, on| {
                pick(world, &key, on);
            }),
            column((
                one_line(label(title)),
                column((one_line(label(detail)),)).rules(note_rules),
            ))
            .gap(2.0)
            .grow(1.0)
            .min_width(px(0.0)),
        ))
        .gap(12.0)
        .padding(UiRect::left(px(INDENT)))
        .align(AlignItems::Center)
        .boxed(),
    }
}

/// What the card says over the list: what it does, or what was
/// brought in.
fn status() -> AnyView {
    keyed(
        derived(|world: &World| {
            let imports = world.resource::<Imports>();
            let all = imports.projects.len();
            let picked = world.resource::<Projects>().picked.len();
            let found = match (&imports.unread, imports.looking) {
                (_, true) => "Looking…".to_owned(),
                (Some(why), false) => why.clone(),
                (None, false) if all == 0 => "No projects found \
                                              from the last 30 days."
                    .to_owned(),
                (None, false) => format!(
                    "{picked} conversations picked from {all} \
                     projects"
                ),
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
             becomes a hamlet working on its folder, and each \
             conversation picked a hire that carries it on. Open a \
             project to pick its conversations.",
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
        .within(column(()).gap(8.0)),
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
            (_, false) => good_action(Glyph::Plus, "Bring in", bring),
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

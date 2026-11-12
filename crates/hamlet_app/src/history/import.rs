//! The sessions page's list of conversations of other programs, on
//! this machine or in an export, each brought in as a session of the
//! chat (C35).

use std::path::Path;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::{FromWorld, World};
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, each, every_frame, keyed, resource,
    style,
};
use hamlet_client::imports::{
    BringIn, BroughtIn, FindImports, Imports, Step,
};
use hamlet_client::realm::id_of;
use hamlet_desktop::choose::{Choose, Chosen, What};
use hamlet_ui::styles::{
    Fills, back, command, link_row, note, scroll_area,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::cut;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::{History, TITLE_SHOWN};
use crate::convo::Convo;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Importing>()
        .init_resource::<Asker>()
        .add_observer(chosen)
        .add_observer(brought_in);
}

/// The sessions page shows conversations to bring in.
#[derive(Resource, Default)]
pub(super) struct Importing(pub bool);

/// Whom the dialog an export is chosen in answers.
#[derive(Resource)]
struct Asker(Entity);

impl FromWorld for Asker {
    fn from_world(world: &mut World) -> Self {
        Self(world.spawn_empty().id())
    }
}

/// A conversation as a row shows it.
#[derive(Clone, PartialEq)]
struct Line {
    index: usize,
    title: String,
    detail: String,
}

/// Shows the conversations found on this machine.
pub(super) fn open(world: &mut World) {
    world.resource_mut::<Importing>().0 = true;
    world.trigger(FindImports { file: None });
}

fn leave(world: &mut World) {
    world.resource_mut::<Importing>().0 = false;
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
    mut commands: Commands,
) {
    if chosen.asker != asker.0 {
        return;
    }
    if let Some(file) = chosen.paths.first() {
        commands.trigger(FindImports {
            file: Some(file.clone()),
        });
    }
}

fn brought_in(_: On<BroughtIn>, mut history: ResMut<History>) {
    history.fresh = false;
}

fn bring(world: &mut World, index: usize) {
    let found = world.resource::<Imports>().found.get(index).cloned();
    let to = id_of(world, world.resource::<Convo>().agent);
    if let (Some(found), Some(to)) = (found, to) {
        world.trigger(BringIn { found, to });
    }
}

fn lines(world: &World) -> Vec<Line> {
    let home = std::env::home_dir();
    world
        .resource::<Imports>()
        .found
        .iter()
        .enumerate()
        .map(|(index, found)| {
            let day = chrono::DateTime::<chrono::Local>::from(
                found.modified,
            )
            .format("%-d %b %Y");
            let mut detail =
                format!("{} · {day}", found.source.name());
            if let Some(folder) = &found.folder {
                let folder = Path::new(folder);
                let shown = home
                    .as_deref()
                    .and_then(|home| folder.strip_prefix(home).ok())
                    .map_or_else(
                        || folder.display().to_string(),
                        |under| format!("~/{}", under.display()),
                    );
                detail.push_str(&format!(" · {shown}"));
            }
            Line {
                index,
                title: cut(&found.title, TITLE_SHOWN),
                detail,
            }
        })
        .collect()
}

/// What the page says over the list: what it is doing, or how the
/// last conversation brought in went.
fn status() -> impl View {
    keyed(
        resource::<Imports, _>(|imports| {
            let words = match &imports.step {
                Step::Reading | Step::Sending => {
                    Some((Tone::Dim, "Bringing it in…".to_owned()))
                }
                Step::Done(text) => {
                    Some((Tone::Success, text.clone()))
                }
                Step::Failed(why) => {
                    Some((Tone::Critical, why.clone()))
                }
                Step::Idle => None,
            };
            let found = match (
                imports.looking,
                &imports.unread,
                imports.found.len(),
            ) {
                (true, ..) => "Looking…".to_owned(),
                (false, Some(why), _) => why.clone(),
                (false, None, 0) => {
                    "No conversations found.".to_owned()
                }
                (false, None, 1) => "1 conversation".to_owned(),
                (false, None, count) => {
                    format!("{count} conversations")
                }
            };
            (found, words)
        }),
        |(found, words)| {
            let mut shown = vec![note(found.clone()).boxed()];
            if let Some((tone, text)) = words {
                shown.push(label(text.clone()).tone(*tone).boxed());
            }
            column(shown).gap(4.0).boxed()
        },
    )
    .within(column(()))
}

fn line_view(line: &Line) -> AnyView {
    let index = line.index;
    let (title, detail) = (line.title.clone(), line.detail.clone());
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let name = label(title.clone()).rules(|cx: &mut Cx| {
            style::<HamletTheme>()
                .label(|l, t| l.size(t.text.heading).bold(true))
                .apply(cx);
        });
        button(
            row((column((name, note(detail.clone())))
                .gap(space.gap / 2.0)
                .grow(1.0)
                .min_width(px(0.0)),))
            .fills_width(),
        )
        .width(percent(100.0))
        .padding(UiRect::axes(
            px(space.card_padding_x),
            px(space.card_padding_y),
        ))
        .rules(link_row)
        .tagged(Variant::Default)
        .on_activate(move |world| bring(world, index))
    })
    .boxed()
}

/// The page of conversations to bring in.
pub(super) fn screen() -> AnyView {
    with_cx(|cx| {
        let space = &cx.theme().space;
        column((
            row((
                back("Sessions", leave),
                command("From an export file", choose),
            ))
            .gap(space.section)
            .align(AlignItems::Center),
            status(),
            scroll_area((each(
                derived(lines).when(every_frame()),
                |line: &Line| {
                    (
                        line.index,
                        line.title.clone(),
                        line.detail.clone(),
                    )
                },
                line_view,
            )
            .within(column(()).gap(0.0)),)),
        ))
        .fills_height()
        .gap(space.section)
    })
    .boxed()
}

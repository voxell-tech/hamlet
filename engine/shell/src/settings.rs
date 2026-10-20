//! Settings pages, each an entity, as plugins add them, and the
//! Settings panel that shows them: a list of their titles beside the
//! page chosen.

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::Changed;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, ghost, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_api::id::ENGINE;

use crate::dock::{Panel, panel};
use crate::{AnyView, Cx};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Pages>()
        .add_systems(Startup, open)
        .add_systems(Update, follow);
}

/// A page of settings.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct SettingsPage {
    pub id: PanelId,
    pub title: String,
    /// Builds its content, given the page's entity.
    pub content: fn(Entity) -> AnyView,
}

/// The pages as the Settings panel lists them, by title, and the one
/// chosen.
#[derive(Resource, Clone, PartialEq, Default)]
struct Pages {
    list: Vec<(Entity, String)>,
    chosen: Option<Entity>,
}

fn open(mut commands: Commands) {
    commands.spawn(Panel {
        id: PanelId::new(ENGINE, "settings").expect("a valid id"),
        title: "Settings".into(),
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    panel(keyed(resource::<Pages, _>(Clone::clone), |pages| {
        if pages.list.is_empty() {
            return label("No settings.").tone(Tone::Dim).boxed();
        }
        let titles = pages
            .list
            .iter()
            .map(|(page, title)| {
                let page = *page;
                let tone = if pages.chosen == Some(page) {
                    Tone::Body
                } else {
                    Tone::Dim
                };
                button(label(title.clone()).tone(tone))
                    .rules(ghost)
                    .on_activate(move |world| {
                        world.resource_mut::<Pages>().chosen =
                            Some(page);
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        let shown = match pages.chosen {
            Some(page) => page_view(page),
            None => column(()).boxed(),
        };
        row((
            column(titles).gap(2.0).min_width(px(140.0)),
            column((shown,)).grow(1.0).min_width(px(0.0)),
        ))
        .align(AlignItems::Start)
        .grow(1.0)
        .boxed()
    }))
}

/// The content of the page `page`, as its owner builds it.
fn page_view(page: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        match cx
            .world
            .get::<SettingsPage>(page)
            .map(|page| page.content)
        {
            Some(content) => cx.build(content(page)),
            None => cx.build(column(())),
        }
    })
}

/// Lists the pages again when one comes, changes or goes.
fn follow(
    changed: Query<(), Changed<SettingsPage>>,
    mut gone: RemovedComponents<SettingsPage>,
    all: Query<(Entity, &SettingsPage)>,
    mut pages: ResMut<Pages>,
) {
    if changed.is_empty() && gone.read().count() == 0 {
        return;
    }
    let mut list = all
        .iter()
        .map(|(page, settings)| (page, settings.title.clone()))
        .collect::<Vec<_>>();
    list.sort_by(|a, b| a.1.cmp(&b.1));
    let chosen = pages
        .chosen
        .filter(|chosen| list.iter().any(|(page, _)| page == chosen))
        .or_else(|| list.first().map(|(page, _)| *page));
    let now = Pages { list, chosen };
    if *pages != now {
        *pages = now;
    }
}

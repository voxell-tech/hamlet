//! Settings pages, each an entity, as plugins add them, and the
//! Settings panel that shows them: a link row to each, and the page
//! gone through under a head that leads back.

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::Changed;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ui::px;
use bevy_fynix::views::{BehaviorExt, FrameProps, column};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_api::id::ENGINE;

use crate::dock::{Panel, PanelIcon};
use crate::icons::Glyph;
use crate::style::{
    StyleExt, link_row, link_rows, note, page_head, scroll_area,
};
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
    commands.spawn((
        Panel {
            id: PanelId::new(ENGINE, "settings").expect("a valid id"),
            title: "Settings".into(),
            content: view,
        },
        PanelIcon(Glyph::Sliders),
    ));
}

/// The pages as a list of link rows, or the page chosen under its
/// head.
fn view(_: Entity) -> AnyView {
    let page = keyed(resource::<Pages, _>(Clone::clone), |pages| {
        if pages.list.is_empty() {
            return note("No settings.");
        }
        let chosen = pages.chosen.and_then(|chosen| {
            pages.list.iter().find(|(page, _)| *page == chosen)
        });
        if let Some((page, title)) = chosen {
            return AnyView::new({
                let (page, title) = (*page, title.to_uppercase());
                move |cx: &mut Cx| {
                    let gap = cx.theme().space.panel;
                    cx.build(
                        column((
                            page_head(
                                "SETTINGS",
                                |world| {
                                    world
                                        .resource_mut::<Pages>()
                                        .chosen = None;
                                },
                                Glyph::Sliders,
                                title,
                            ),
                            page_view(page),
                        ))
                        .gap(gap),
                    )
                }
            });
        }
        link_rows(
            pages
                .list
                .iter()
                .map(|(page, title)| {
                    let page = *page;
                    link_row(
                        Glyph::Sliders,
                        title.to_uppercase(),
                        None,
                    )
                    .on_activate(move |world| {
                        world.resource_mut::<Pages>().chosen =
                            Some(page);
                    })
                    .boxed()
                })
                .collect(),
        )
    })
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let width = cx.theme().space.page_width;
        cx.build(scroll_area((column((page,))
            .gap(0.0)
            .max_width(px(width)),)))
    })
    .in_panel()
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
        .filter(|chosen| list.iter().any(|(page, _)| page == chosen));
    let now = Pages { list, chosen };
    if *pages != now {
        *pages = now;
    }
}

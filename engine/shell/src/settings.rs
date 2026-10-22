//! Settings pages, each an entity, as plugins add them, and the
//! Settings panel that shows them: a link row to each in the order
//! they came, with the footers under them, and the page gone through
//! under a head that leads back.

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::{Add, RemovedComponents};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ui::{UiRect, px};
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

/// The name of the Settings panel.
pub const SETTINGS: &str = "settings";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Pages>()
        .init_resource::<Placed>()
        .add_observer(place)
        .add_systems(Startup, open)
        .add_systems(Update, follow);
}

/// A page of settings.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct SettingsPage {
    pub id: PanelId,
    pub title: String,
    /// What it holds, in a line under its title.
    pub about: String,
    pub icon: Glyph,
    /// Builds its content, given the page's entity.
    pub content: fn(Entity) -> AnyView,
}

/// What Settings shows under the link rows to its pages.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct SettingsFooter {
    pub id: PanelId,
    /// Builds its content, given the footer's entity.
    pub content: fn(Entity) -> AnyView,
}

/// On a settings page or footer: when it came, among all of them.
#[derive(Component, Clone, Copy)]
struct Order(u64);

/// How many settings pages and footers have come.
#[derive(Resource, Default)]
struct Placed(u64);

/// The pages as the Settings panel lists them, the footers, and the
/// page chosen.
#[derive(Resource, Clone, PartialEq, Default)]
pub(crate) struct Pages {
    list: Vec<Listed>,
    footers: Vec<Entity>,
    pub(crate) chosen: Option<Entity>,
}

/// A page as its link row shows it.
#[derive(Clone, PartialEq)]
struct Listed {
    page: Entity,
    title: String,
    about: String,
    icon: Glyph,
}

fn open(mut commands: Commands) {
    commands.spawn((
        Panel {
            id: PanelId::new(ENGINE, SETTINGS).expect("a valid id"),
            title: "Settings".into(),
            content: view,
        },
        PanelIcon(Glyph::Sliders),
    ));
}

/// The pages as a list of link rows over the footers, or the page
/// chosen under its head.
fn view(_: Entity) -> AnyView {
    let page = keyed(resource::<Pages, _>(Clone::clone), |pages| {
        let chosen = pages.chosen.and_then(|chosen| {
            pages.list.iter().find(|listed| listed.page == chosen)
        });
        if let Some(listed) = chosen {
            let (page, icon, title) = (
                listed.page,
                listed.icon,
                listed.title.to_uppercase(),
            );
            return AnyView::new(move |cx: &mut Cx| {
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
                            icon,
                            title,
                        ),
                        content_of(page),
                    ))
                    .gap(gap),
                )
            });
        }
        if pages.list.is_empty() && pages.footers.is_empty() {
            return note("No settings.");
        }
        let rows = link_rows(
            pages
                .list
                .iter()
                .map(|listed| {
                    let page = listed.page;
                    let about = (!listed.about.is_empty())
                        .then(|| listed.about.clone());
                    link_row(
                        listed.icon,
                        listed.title.to_uppercase(),
                        about,
                    )
                    .on_activate(move |world| {
                        world.resource_mut::<Pages>().chosen =
                            Some(page);
                    })
                    .boxed()
                })
                .collect(),
        );
        let footers = pages.footers.clone();
        AnyView::new(move |cx: &mut Cx| {
            let gap = cx.theme().space.gap;
            let mut parts = vec![rows];
            parts.extend(footers.iter().map(|&footer| {
                column((content_of(footer),))
                    .gap(0.0)
                    .padding(UiRect::top(px(gap)))
                    .boxed()
            }));
            cx.build(column(parts).gap(0.0))
        })
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

/// The content of the page or footer `at`, as its owner builds it.
fn content_of(at: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let content = cx
            .world
            .get::<SettingsPage>(at)
            .map(|page| page.content)
            .or_else(|| {
                cx.world
                    .get::<SettingsFooter>(at)
                    .map(|footer| footer.content)
            });
        match content {
            Some(content) => cx.build(content(at)),
            None => cx.build(column(())),
        }
    })
}

fn place(
    added: On<Add, (SettingsPage, SettingsFooter)>,
    mut placed: ResMut<Placed>,
    mut commands: Commands,
) {
    placed.0 += 1;
    commands.entity(added.entity).insert(Order(placed.0));
}

/// Lists the pages and footers again when one comes, changes or
/// goes.
fn follow(
    changed: Query<
        (),
        Or<(
            Changed<SettingsPage>,
            Changed<SettingsFooter>,
            Changed<Order>,
        )>,
    >,
    mut gone: RemovedComponents<SettingsPage>,
    mut gone_footers: RemovedComponents<SettingsFooter>,
    all: Query<(Entity, &SettingsPage, &Order)>,
    footers: Query<(Entity, &Order), With<SettingsFooter>>,
    mut pages: ResMut<Pages>,
) {
    let went = gone.read().count() + gone_footers.read().count() > 0;
    if changed.is_empty() && !went {
        return;
    }
    let mut list = all.iter().collect::<Vec<_>>();
    list.sort_by_key(|(.., order)| order.0);
    let list = list
        .into_iter()
        .map(|(page, settings, _)| Listed {
            page,
            title: settings.title.clone(),
            about: settings.about.clone(),
            icon: settings.icon,
        })
        .collect::<Vec<_>>();
    let mut below = footers.iter().collect::<Vec<_>>();
    below.sort_by_key(|(_, order)| order.0);
    let chosen = pages.chosen.filter(|chosen| {
        list.iter().any(|listed| listed.page == *chosen)
    });
    let now = Pages {
        list,
        footers: below
            .into_iter()
            .map(|(footer, _)| footer)
            .collect(),
        chosen,
    };
    if *pages != now {
        *pages = now;
    }
}

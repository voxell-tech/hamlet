//! The pages of the Web panels: one webview each, drawn in its
//! panel while the panel is in sight and put away while it is not.

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::asset::Assets;
use bevy::camera::visibility::Visibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use bevy::ui::widget::Text;
use bevy::ui::{Node, PositionType, percent};
use bevy::ui_render::prelude::MaterialNode;
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_cef::prelude::{
    AddressChanged, BeginFrameInterval, CefPlugin, RequestNavigate,
    WebviewHidden, WebviewSource, WebviewTitle, WebviewUiMaterial,
};
use bevy_fynix::dock::{DockTab, DockTree};
use hamlet_client::ViewFolder;

use super::panel::{PanelId, Surface, is_panel, tab_name};
use super::{Addresses, engine};
use crate::dock::on_a_desk;

/// The file name of the engine's helper, `src/web_helper.rs`.
const HELPER: &str = "hamlet_web_helper";

/// The least time between two frames of a page. Shorter than any
/// frame of the window, so a page draws once for each of the
/// window's frames and each of its frames shows for as long.
const FRAME_GAP: Duration = Duration::from_millis(1);

/// The frames the window draws each second while it is not in front
/// and a page is in sight.
const FRAMES_BEHIND: u64 = 30;

/// How long a page is out of sight before it sleeps: it stops drawing
/// and its timers slow, and it shows its last frame when it is back.
const SLEEP_AFTER: Duration = Duration::from_secs(30);

pub(super) fn plugin(app: &mut App) {
    // One session for every panel, kept with the window's view.
    let cache =
        app.world().get_resource::<ViewFolder>().map(|view| {
            view.0.join("web").to_string_lossy().into_owned()
        });
    app.add_plugins(CefPlugin {
        root_cache_path: cache,
        cef_path: engine::located(),
        subprocess_path: Some(helper()),
        ..CefPlugin::default()
    })
    .insert_resource(BeginFrameInterval(FRAME_GAP))
    .init_resource::<InSight>()
    .add_observer(follow_address)
    .add_systems(
        Update,
        (
            (open, place, sleep).chain().after(crate::desk::capture),
            pace.after(crate::world_view::pace),
            name_tabs,
        ),
    );
}

/// The helper the engine starts for each page's processes: where
/// `CEF_SUBPROCESS_PATH` says, else beside the window.
pub(super) fn helper() -> PathBuf {
    std::env::var_os("CEF_SUBPROCESS_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            let window = std::env::current_exe().ok()?;
            Some(window.parent()?.join(HELPER))
        })
        .unwrap_or_else(|| PathBuf::from(HELPER))
}

/// The webview of a Web panel.
#[derive(Component)]
pub(super) struct Page(pub PanelId);

/// Whether any Web panel's page is in sight.
#[derive(Resource, Default)]
pub(super) struct InSight(pub bool);

/// The time a page went out of sight.
#[derive(Component)]
struct OutOfSight(Duration);

/// The node pages out of sight are kept under, undrawn.
#[derive(Component)]
struct Away;

/// `panel`'s page, once it has one.
pub(super) fn of(
    world: &mut World,
    panel: &PanelId,
) -> Option<Entity> {
    world
        .query::<(Entity, &Page)>()
        .iter(world)
        .find(|(_, page)| page.0 == *panel)
        .map(|(entity, _)| entity)
}

/// Loads `address` in `panel`'s page; a panel with none gets one at
/// it by [`open`].
pub(super) fn navigate(
    world: &mut World,
    panel: &PanelId,
    address: String,
) {
    if let Some(page) = of(world, panel) {
        world.trigger(RequestNavigate {
            webview: page,
            url: address,
        });
    }
}

/// The Web panels the dock holds, with whether each is the tab in
/// front of its area.
fn docked(tree: &DockTree) -> Vec<(PanelId, bool)> {
    let Some(root) = tree.root else {
        return Vec::new();
    };
    tree.leaves_under(root)
        .into_iter()
        .flat_map(|(leaf, area)| {
            let front = tree.active(leaf);
            area.tabs()
                .filter(|(window, _)| is_panel(window))
                .map(move |(window, tab)| {
                    (PanelId(window.to_owned()), front == Some(tab))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Gives each Web panel with an address a page, and closes the page
/// and forgets the address of each panel neither the dock nor a desk
/// holds any more.
fn open(world: &mut World) {
    let docked = docked(world.resource::<DockTree>());
    let kept = |world: &World, panel: &str| {
        docked.iter().any(|(id, _)| id.0 == panel)
            || on_a_desk(world, panel)
    };
    let closed = world
        .query::<(Entity, &Page)>()
        .iter(world)
        .filter(|(_, page)| !kept(world, &page.0.0))
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    for page in closed {
        world.despawn(page);
    }
    let gone = world
        .resource::<Addresses>()
        .0
        .keys()
        .filter(|panel| !kept(world, panel))
        .cloned()
        .collect::<Vec<_>>();
    if !gone.is_empty() {
        let mut addresses = world.resource_mut::<Addresses>();
        for panel in gone {
            addresses.0.remove(&panel);
        }
    }
    let wanted = world
        .resource::<Addresses>()
        .0
        .iter()
        .map(|(panel, address)| {
            (PanelId(panel.clone()), address.clone())
        })
        .collect::<Vec<_>>();
    for (panel, address) in wanted {
        if of(world, &panel).is_some() {
            continue;
        }
        let material = world
            .resource_mut::<Assets<WebviewUiMaterial>>()
            .add(WebviewUiMaterial::default());
        world.spawn((
            Page(panel),
            WebviewSource::new(address),
            Node {
                width: percent(100.0),
                height: percent(100.0),
                ..Node::default()
            },
            MaterialNode(material),
        ));
    }
}

/// Hangs each page off its panel while the panel is the tab in
/// front, and off a node that is not drawn otherwise. The page lives
/// apart from the panel's nodes, which the dock builds again as it
/// changes.
fn place(
    tree: Res<DockTree>,
    pages: Query<(Entity, &Page, Option<&ChildOf>, Has<OutOfSight>)>,
    surfaces: Query<(Entity, &Surface)>,
    away: Query<Entity, With<Away>>,
    time: Res<Time<Real>>,
    mut in_sight: ResMut<InSight>,
    mut commands: Commands,
    mut made: Local<Option<Entity>>,
) {
    let docked = docked(&tree);
    let away = away.iter().next().unwrap_or_else(|| {
        *made.get_or_insert_with(|| {
            // Laid out at the window's size, so a page put away keeps
            // a size to draw at.
            commands
                .spawn((
                    Away,
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100.0),
                        height: percent(100.0),
                        ..Node::default()
                    },
                    Visibility::Hidden,
                ))
                .id()
        })
    });
    let mut any = false;
    for (page, Page(panel), parent, out) in &pages {
        let front =
            docked.iter().any(|(id, front)| id == panel && *front);
        let surface = surfaces
            .iter()
            .find(|(_, surface)| surface.0 == *panel)
            .map(|(surface, _)| surface)
            .filter(|_| front);
        any |= surface.is_some();
        let under = surface.unwrap_or(away);
        if parent.map(ChildOf::parent) != Some(under) {
            commands.entity(page).try_insert(ChildOf(under));
        }
        match (surface.is_some(), out) {
            (true, true) => {
                commands.entity(page).try_remove::<OutOfSight>();
            }
            (false, false) => {
                commands
                    .entity(page)
                    .try_insert(OutOfSight(time.elapsed()));
            }
            _ => {}
        }
    }
    if in_sight.0 != any {
        in_sight.0 = any;
    }
}

/// Puts to sleep each page out of sight for [`SLEEP_AFTER`], and
/// wakes each that is back.
fn sleep(
    time: Res<Time<Real>>,
    pages: Query<
        (Entity, Option<&OutOfSight>, Option<&WebviewHidden>),
        With<Page>,
    >,
    mut commands: Commands,
) {
    for (page, out, hidden) in &pages {
        let asleep = out
            .is_some_and(|out| time.elapsed() - out.0 >= SLEEP_AFTER);
        if hidden.is_some_and(|hidden| hidden.0) != asleep {
            commands.entity(page).try_insert(WebviewHidden(asleep));
        }
    }
}

/// Keeps a panel's address as its page goes from one to another.
fn follow_address(
    moved: On<AddressChanged>,
    pages: Query<&Page>,
    mut addresses: ResMut<Addresses>,
) {
    let Ok(Page(panel)) = pages.get(moved.webview) else {
        return;
    };
    if addresses.0.get(&panel.0) != Some(&moved.url) {
        addresses.0.insert(panel.0.clone(), moved.url.clone());
    }
}

/// Names each Web panel's tab after its page's title.
fn name_tabs(
    tabs: Query<(Entity, &DockTab)>,
    tree: Res<DockTree>,
    pages: Query<(&Page, &WebviewTitle)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    for (node, tab) in &tabs {
        let Some(window) = tree.leaf(tab.leaf).and_then(|leaf| {
            leaf.tabs()
                .find(|(_, id)| *id == tab.tab)
                .map(|(window, _)| window)
        }) else {
            continue;
        };
        if !is_panel(window) {
            continue;
        }
        let title = pages
            .iter()
            .find(|(Page(panel), _)| panel.0 == window)
            .map(|(_, title)| title.0.as_str())
            .unwrap_or_default();
        let name = tab_name(title);
        let label = children
            .iter_descendants_depth_first(node)
            .find(|kid| texts.contains(*kid));
        if let Some(label) = label
            && let Ok(mut text) = texts.get_mut(label)
            && text.0 != name
        {
            text.0 = name;
        }
    }
}

/// Draws every frame while a page is in sight and the window is in
/// front, which the window's slow tick would starve, and at
/// [`FRAMES_BEHIND`] while it is behind.
fn pace(
    in_sight: Res<InSight>,
    mut winit: ResMut<WinitSettings>,
    mut was: Local<bool>,
) {
    if in_sight.0 {
        if winit.focused_mode != UpdateMode::Continuous {
            winit.focused_mode = UpdateMode::Continuous;
        }
        let behind = UpdateMode::reactive(Duration::from_millis(
            1000 / FRAMES_BEHIND,
        ));
        if winit.unfocused_mode != behind {
            winit.unfocused_mode = behind;
        }
    } else if *was {
        *winit = crate::window::winit_settings();
    }
    *was = in_sight.0;
}

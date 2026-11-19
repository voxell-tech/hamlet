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
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use bevy::ui::widget::Text;
use bevy::ui::{Node, PositionType, percent};
use bevy::ui_render::prelude::MaterialNode;
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_cef::prelude::{
    AddressChanged, BeginFrameInterval, CefPlugin, CommandLineConfig,
    PopupRequested, PreloadScripts, RequestNavigate, TitleChanged,
    WebviewClipboard, WebviewHidden, WebviewProfile, WebviewSource,
    WebviewTitle, WebviewUiMaterial,
};
use bevy_fynix::dock::{
    DockTab, DockTree, DropTarget, NodeId, TabId,
};
use hamlet_client::ViewFolder;

use super::agents::{AgentNames, Waits};
use super::panel::{PanelId, Surface, has_page, is_panel, tab_name};
use super::{
    Addresses, AgentTabs, FILES, PANEL, Titles, bridge, engine,
};
use crate::dock::{on_a_desk, open_numbered};

/// The name of the chief's browsing session, which every Web panel of
/// the chief's shares and the view folder keeps.
const CHIEF: &str = "chief";

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
        // Chromium's own log, which no one reading the window's
        // needs.
        command_line_config: CommandLineConfig::default()
            .with_switch_value("log-severity", "disable"),
        root_cache_path: cache,
        cef_path: engine::located(),
        subprocess_path: Some(helper()),
        ..CefPlugin::default()
    })
    .insert_resource(BeginFrameInterval(FRAME_GAP))
    .init_resource::<InSight>()
    .add_observer(follow_address)
    .add_observer(keep_title)
    .add_observer(open_popup)
    .add_systems(
        Update,
        (
            (open, place, sleep)
                .chain()
                .in_set(Opening)
                .after(crate::desk::capture),
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
            Some(
                window
                    .parent()?
                    .join(HELPER)
                    .with_extension(std::env::consts::EXE_EXTENSION),
            )
        })
        .unwrap_or_else(|| PathBuf::from(HELPER))
}

/// Where the pages of the Web panels are opened, put in place and put
/// to sleep.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct Opening;

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

/// Wakes `panel`'s page, if it sleeps, and keeps it awake for another
/// [`SLEEP_AFTER`], for an agent acting on it.
pub(super) fn wake(world: &mut World, panel: &PanelId) {
    let Some(page) = of(world, panel) else {
        return;
    };
    let now = world.resource::<Time<Real>>().elapsed();
    let mut page = world.entity_mut(page);
    if page.contains::<OutOfSight>() {
        page.insert(OutOfSight(now));
    }
    if page.get::<WebviewHidden>().is_some_and(|hidden| hidden.0) {
        page.insert(WebviewHidden(false));
    }
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

/// The panels with pages the dock holds, with whether each is the
/// tab in front of its area.
fn docked(tree: &DockTree) -> Vec<(PanelId, bool)> {
    let Some(root) = tree.root else {
        return Vec::new();
    };
    tree.leaves_under(root)
        .into_iter()
        .flat_map(|(leaf, area)| {
            let front = tree.active(leaf);
            area.tabs()
                .filter(|(window, _)| has_page(window))
                .map(move |(window, tab)| {
                    (PanelId(window.to_owned()), front == Some(tab))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Gives each Web panel with an address a page once its tab is in
/// front, and closes the page
/// and forgets the address of each panel neither the dock nor a desk
/// holds any more. The Files panel's page is kept when it closes, put
/// away like a page behind another tab, so that the web IDE opens
/// again as it was rather than loading anew.
fn open(world: &mut World) {
    let docked = docked(world.resource::<DockTree>());
    let kept = |world: &World, panel: &str| {
        panel == FILES
            || docked.iter().any(|(id, _)| id.0 == panel)
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
    for panel in gone {
        world.resource_mut::<Addresses>().0.remove(&panel);
        world.resource_mut::<AgentTabs>().0.remove(&panel);
        world.resource_mut::<Titles>().0.remove(&panel);
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
        let agent =
            world.resource::<AgentTabs>().0.contains_key(&panel.0);
        // A page loads the first time its tab is in front, so the
        // tabs kept from before, and those opened behind, cost
        // nothing until they are looked at. An agent acts on its page
        // unseen.
        let front =
            docked.iter().any(|(id, front)| *id == panel && *front);
        if !front && !agent {
            continue;
        }
        // The web IDE, the window's own, pastes what the chief
        // copied.
        let files = panel.0 == FILES;
        let material = world
            .resource_mut::<Assets<WebviewUiMaterial>>()
            .add(WebviewUiMaterial::default());
        let mut page = world.spawn((
            Page(panel),
            WebviewSource::new(address),
            Node {
                width: percent(100.0),
                height: percent(100.0),
                ..Node::default()
            },
            MaterialNode(material),
        ));
        if files {
            page.insert(WebviewClipboard);
        }
        // A page with no profile, as an agent's is, has a session of
        // its own, kept in memory.
        if agent {
            page.insert(PreloadScripts(vec![
                bridge::SCRIPT.to_owned(),
            ]));
        } else if let Ok(profile) = WebviewProfile::new(CHIEF) {
            page.insert(profile);
        }
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
    pages: Query<(
        Entity,
        &Page,
        Option<&OutOfSight>,
        Option<&WebviewHidden>,
    )>,
    waits: Option<Res<Waits>>,
    mut commands: Commands,
) {
    for (page, Page(panel), out, hidden) in &pages {
        // An agent's page it waits on stays awake.
        let waited = waits.as_ref().is_some_and(|w| w.has(&panel.0));
        let asleep = !waited
            && out.is_some_and(|out| {
                time.elapsed() - out.0 >= SLEEP_AFTER
            });
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

/// Keeps the title of each Web panel's page for its tab, for when the
/// page is not loaded.
fn keep_title(
    changed: On<TitleChanged>,
    pages: Query<&Page>,
    mut titles: ResMut<Titles>,
) {
    let Ok(Page(panel)) = pages.get(changed.webview) else {
        return;
    };
    let title = changed.title.trim();
    if !title.is_empty()
        && titles.0.get(&panel.0).map(String::as_str) != Some(title)
    {
        titles.0.insert(panel.0.clone(), title.to_owned());
    }
}

/// Opens the page a page asked for in a new window in a Web tab just
/// after its own, in front unless it was opened behind. An agent's
/// page has a session of its own, so the page it asks for loads in
/// its place.
fn open_popup(popup: On<PopupRequested>, mut commands: Commands) {
    let (webview, url, foreground) =
        (popup.webview, popup.url.clone(), popup.foreground);
    commands.queue(move |world: &mut World| {
        let Some(panel) =
            world.get::<Page>(webview).map(|page| page.0.clone())
        else {
            return;
        };
        if world.resource::<AgentTabs>().0.contains_key(&panel.0) {
            navigate(world, &panel, url);
            return;
        }
        let Some((leaf, index, front)) = beside(world, &panel) else {
            return;
        };
        let Some(id) = open_numbered(
            world,
            PANEL,
            DropTarget::Tabs { leaf, index },
        ) else {
            return;
        };
        world.resource_mut::<Addresses>().0.insert(id, url);
        if !foreground && let Some(front) = front {
            world.resource_mut::<DockTree>().set_active(leaf, front);
        }
    });
}

/// Where a tab opened from `panel`'s page goes: the area of `panel`'s
/// tab, else the first with a Web panel, else the first; the slot
/// after `panel`'s; and the tab in front of that area now.
fn beside(
    world: &World,
    panel: &PanelId,
) -> Option<(NodeId, usize, Option<TabId>)> {
    let tree = world.resource::<DockTree>();
    let leaves = tree.leaves_under(tree.root?);
    let (leaf, area) = leaves
        .iter()
        .find(|(_, area)| area.tabs().any(|(id, _)| id == panel.0))
        .or_else(|| {
            leaves.iter().find(|(_, area)| {
                area.tabs().any(|(id, _)| is_panel(id))
            })
        })
        .or_else(|| leaves.first())?;
    let index = area
        .tabs()
        .position(|(id, _)| id == panel.0)
        .map_or_else(|| area.tabs().count(), |at| at + 1);
    Some((*leaf, index, tree.active(*leaf)))
}

/// Names each Web panel's tab after its page's title.
fn name_tabs(
    tabs: Query<(Entity, &DockTab)>,
    tree: Res<DockTree>,
    agents: Option<Res<AgentNames>>,
    addresses: Res<Addresses>,
    titles: Res<Titles>,
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
        let agent = agents
            .as_ref()
            .and_then(|names| names.0.get(window))
            .map(String::as_str);
        let name = tab_name(
            title,
            titles.0.get(window).map(String::as_str),
            addresses.0.get(window).map(String::as_str),
            agent,
        );
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

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetApp, AssetPlugin};
    use bevy::ecs::system::RunSystemOnce;
    use bevy_fynix::dock::{DockAreaStyle, DockLeaf};

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default())
            .init_asset::<WebviewUiMaterial>()
            .init_resource::<AgentTabs>()
            .init_resource::<Titles>()
            .insert_resource(Addresses(
                [
                    (
                        FILES.to_owned(),
                        "http://127.0.0.1:1/".to_owned(),
                    ),
                    (
                        "web".to_owned(),
                        "https://example.com".to_owned(),
                    ),
                ]
                .into(),
            ));
        let mut tree = DockTree::new();
        tree.set_root_leaf(
            DockLeaf::new("area", DockAreaStyle::TabBar)
                .with_windows(vec![
                    FILES.to_owned(),
                    "web".to_owned(),
                ]),
        );
        app.insert_resource(tree);
        app
    }

    fn pages(app: &mut App) -> Vec<String> {
        app.world_mut().run_system_once(open).unwrap();
        let mut pages = app
            .world_mut()
            .query::<&Page>()
            .iter(app.world())
            .map(|page| page.0.0.clone())
            .collect::<Vec<_>>();
        pages.sort();
        pages
    }

    fn show(app: &mut App, window: &str) {
        let mut tree = app.world_mut().resource_mut::<DockTree>();
        let leaf = tree.find_leaf_with_window(window).unwrap();
        let tab = tree
            .leaf(leaf)
            .unwrap()
            .tabs()
            .find(|(id, _)| *id == window)
            .unwrap()
            .1;
        tree.set_active(leaf, tab);
    }

    #[test]
    fn a_tab_behind_loads_its_page_only_once_it_is_in_front() {
        let mut app = app();
        show(&mut app, FILES);
        assert_eq!(pages(&mut app), [FILES]);
        show(&mut app, "web");
        assert_eq!(pages(&mut app), [FILES, "web"]);
        // Behind again, it keeps its page.
        show(&mut app, FILES);
        assert_eq!(pages(&mut app), [FILES, "web"]);
    }

    #[test]
    fn the_files_panels_page_outlives_its_tab_and_a_web_panels_does_not()
     {
        let mut app = app();
        show(&mut app, "web");
        pages(&mut app);
        show(&mut app, FILES);
        assert_eq!(pages(&mut app), [FILES, "web"]);
        // Both tabs closed.
        app.insert_resource(DockTree::new());
        assert_eq!(pages(&mut app), [FILES]);
        let kept = app.world().resource::<Addresses>().0.clone();
        assert_eq!(kept.keys().collect::<Vec<_>>(), [FILES]);
    }
}

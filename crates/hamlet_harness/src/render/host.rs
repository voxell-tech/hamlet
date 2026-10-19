//! What a picture shows around a panel: the dock alone, the page of
//! the Inspector or of the Settings it fills, or the menu of a thing
//! of the world.

use bevy::ecs::resource::Resource;
use bevy::prelude::*;
use bevy::ui::px;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockRegistry, DockTree, DockWindowKind,
};
use bevy_fynix::views::{FrameProps, column, frame};
use hamlet_realm_api::pick::{Also, Selected, Thing};
use hamlet_sdk::command::{Subject, kind};
use hamlet_sdk::context::ContextItems;
use hamlet_sdk::dock::{Panel, panel};
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::looks::{ICON, note, page_head, title};
use hamlet_sdk::theme::HamletTheme;
use hamlet_sdk::tree::view;
use hamlet_sdk::{AnyView, Cx};
use hamlet_ui::tabs;
use plugin_inspector::Section;
use plugin_menus::world_menu;

/// The id of the one window of the dock a hosted picture draws.
const HOST: &str = "host";

/// What a picture shows around the plugin panel `panel`, by its id in
/// the dock.
#[derive(Resource, Clone)]
pub enum Host {
    /// The panel alone, as a window of the dock.
    Panel(Panel),
    /// The tab the panel fills in the Inspector's page of an agent.
    Inspector {
        panel: String,
        title: String,
        icon: Glyph,
        /// The agent the page is of.
        agent: String,
    },
    /// The page the panel fills in the Settings.
    Settings {
        panel: String,
        title: String,
        icon: Glyph,
    },
    /// The menu of a thing of the world, with these items.
    Context {
        subject: Subject,
        items: ContextItems,
    },
}

impl Host {
    /// The id in the dock of the plugin panel that is drawn, if one
    /// is.
    pub fn panel(&self) -> Option<&str> {
        match self {
            Host::Panel(panel) => Some(&panel.id),
            Host::Inspector { panel, .. }
            | Host::Settings { panel, .. } => Some(panel),
            Host::Context { .. } => None,
        }
    }

    /// Sets the window of the dock up, and gives the app what the
    /// view of the host reads.
    pub(super) fn arrange(self, app: &mut App) {
        let id = match &self {
            Host::Panel(panel) => {
                let id = panel.id.clone();
                app.world_mut().spawn(panel.clone());
                id
            }
            _ => {
                let (name, icon) = self.window();
                app.world_mut()
                    .resource_mut::<DockRegistry<HamletTheme>>()
                    .register(
                        HOST,
                        DockWindowKind::new(name, hosted)
                            .icon(icon.image(ICON)),
                    );
                HOST.to_string()
            }
        };
        if let Host::Inspector { title, icon, .. } = &self {
            let tab = Section::Contributed(
                app.world_mut().spawn_empty().id(),
            );
            app.insert_resource(tab);
            app.add_systems(Update, tabs::mark::<Section>);
            app.insert_resource(Tab {
                title: title.clone(),
                icon: *icon,
                section: tab,
            });
        }
        if let Host::Context { subject, items } = &self {
            app.insert_resource(items.clone());
            app.insert_resource(Selected::default());
            app.insert_resource(Also::default());
            let name = subject.name.clone();
            let thing = app.world_mut().spawn(Name::new(name)).id();
            let at = Vec2::new(32.0, 64.0);
            let world = app.world_mut();
            match subject.kind.as_str() {
                kind::GROUND => world_menu::show_ground(world, at),
                other => world_menu::show(
                    world,
                    thing_of(other, thing),
                    at,
                ),
            }
        }
        app.insert_resource(self);
        app.world_mut().resource_mut::<DockTree>().set_root_leaf(
            DockLeaf::new("centre", DockAreaStyle::TabBar)
                .with_windows(vec![id]),
        );
    }

    /// The title and the icon of the window the host is drawn in.
    fn window(&self) -> (&'static str, Glyph) {
        match self {
            Host::Inspector { .. } => ("INSPECTOR", Glyph::Lens),
            Host::Settings { .. } => ("SETTINGS", Glyph::Sliders),
            _ => ("WORLD", Glyph::Globe),
        }
    }

    /// The menu of a thing laid over the window, for a host that is
    /// one.
    pub(super) fn overlay(&self) -> AnyView {
        match self {
            Host::Context { .. } => world_menu::view(),
            _ => AnyView::new(|cx: &mut Cx| cx.build(frame())),
        }
    }
}

/// The thing of the world that stands for a kind, with the entity
/// that carries its name.
fn thing_of(of: &str, entity: Entity) -> Thing {
    match of {
        kind::HOUSE => Thing::ResidentHouse(entity),
        kind::RUIN => Thing::Ruin(entity),
        kind::HAMLET => Thing::Common(entity),
        kind::CHIEF => Thing::Chief(entity),
        kind::HEARTH => Thing::ChiefPlace(entity),
        kind::LIBRARY => Thing::Library,
        kind::MAYOR => Thing::Keeper,
        _ => Thing::Resident(entity),
    }
}

/// The tab of the Inspector that the picture is turned to.
#[derive(Resource, Clone)]
struct Tab {
    title: String,
    icon: Glyph,
    section: Section,
}

/// The window of the dock a hosted picture draws.
fn hosted() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let host = cx.world.resource::<Host>().clone();
        match host {
            Host::Panel(_) | Host::Context { .. } => {
                cx.build(panel(column(())))
            }
            Host::Inspector {
                panel: id, agent, ..
            } => {
                let tab = cx.world.resource::<Tab>().clone();
                let tabs = [
                    (
                        Section::Conversation,
                        Glyph::Talk,
                        "CHAT".into(),
                    ),
                    (Section::Notes, Glyph::Notes, "NOTES".into()),
                    (
                        Section::Character,
                        Glyph::Robot,
                        "CHARACTER".into(),
                    ),
                    (
                        Section::History,
                        Glyph::History,
                        "ARCHIVE".into(),
                    ),
                    (tab.section, tab.icon, tab.title),
                ];
                cx.build(panel(
                    column((
                        title(agent.to_uppercase()),
                        note("Selected in the world."),
                        tabs::strip::<Section, String>(
                            &tabs,
                            |_, _| {},
                        ),
                        view::content(&id),
                    ))
                    .gap(12.0),
                ))
            }
            Host::Settings {
                panel: id,
                title: name,
                icon,
            } => cx.build(panel(
                column((
                    page_head("SETTINGS", |_| {}, icon, name),
                    view::content(&id),
                ))
                .gap(16.0)
                .max_width(px(640.0)),
            )),
        }
    })
}

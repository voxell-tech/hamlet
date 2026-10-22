//! What a plugin adds to the engine, declared on its [`App`] as it is
//! built, and the resources kept across a reload.

use std::collections::HashMap;

use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_ecs::system::SystemId;
use hamlet_engine_api::{
    Contribution, Layout, PanelPadding, contribution, decode, encode,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ui::{AnyView, Space};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Declared>()
        .init_resource::<Views>()
        .init_resource::<CommandSystems>()
        .init_resource::<Services>()
        .init_resource::<Subscriptions>()
        .init_resource::<SavedResources>()
        .init_schedule(Declare)
        .add_systems(Declare, subscribe);
}

/// The schedule run once as the engine starts the plugin and asks
/// what it adds, before it is on: for the requests it makes as it
/// starts, such as a watch. Its scenes cannot be made yet. `Startup`
/// runs later, on the first call after it.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Declare;

/// A call of one of the plugin's services: the system serving it is
/// given it as its input, and gives the result's bytes or why it
/// failed. The argument and the result are bytes of the types its
/// manifest names.
pub struct ServiceCall {
    pub service: String,
    /// The id of the plugin that called.
    pub caller: String,
    pub argument: Vec<u8>,
}

/// One of the plugin's commands, run from a menu or a shortcut.
/// Triggered before the system [`HamletAppExt::add_command`] gave
/// it runs, so observers can take it too.
#[derive(Event, Clone, Debug)]
pub struct RunCommand {
    pub name: String,
}

/// What a plugin declares it adds, as methods of its [`App`]. A name
/// is the plugin's own. A command is named by its name, for one of
/// the plugin's own, or by its full id for another's.
pub trait HamletAppExt {
    /// A panel for the dock, titled `title`, its view built by the
    /// system `view` each time it is shown.
    fn add_panel<M>(
        &mut self,
        name: &str,
        title: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self;

    /// Gives the panel added last the icon `icon` on its tab, as
    /// `Glyph::name` has it.
    fn panel_icon(&mut self, icon: &str) -> &mut Self;

    /// Pads the content of the panel added last by `space`, in place
    /// of the theme's panel padding. `Space::None` lets it reach the
    /// panel's edges.
    fn panel_padding(&mut self, space: Space) -> &mut Self;

    /// Pads the content of the panel added last by `pixels`, logical
    /// ones, in place of the theme's panel padding.
    fn panel_padding_pixels(&mut self, pixels: f32) -> &mut Self;

    /// A section titled `title` of the engine's panel `panel`
    /// (`engine/name`), one that takes sections, shown while
    /// [`ui::set_section`](crate::ui::set_section) says it holds
    /// something, and built as a panel is.
    fn add_panel_section<M>(
        &mut self,
        panel: &str,
        name: &str,
        title: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self;

    /// A page of settings titled `title`, built as a panel is. Its
    /// link row in Settings shows the icon `icon`, as `Glyph::name`
    /// has it, and what it holds, `about`.
    fn add_settings_page<M>(
        &mut self,
        name: &str,
        title: &str,
        about: &str,
        icon: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self;

    /// What Settings shows under the link rows to its pages, built as
    /// a panel is.
    fn add_settings_footer<M>(
        &mut self,
        name: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self;

    /// A card over the dock, on a backdrop that takes every press,
    /// shown by [`ui::show_overlay`](crate::ui::show_overlay) and
    /// built as a panel is.
    fn add_overlay<M>(
        &mut self,
        name: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self;

    /// A command a menu calls `label`, running the system `run` after
    /// a [`RunCommand`] is triggered for it.
    fn add_command<M>(
        &mut self,
        name: &str,
        label: &str,
        run: impl IntoSystem<(), (), M> + 'static,
    ) -> &mut Self;

    /// The command `command` in the bar's menu titled `menu`.
    fn add_menu_entry(
        &mut self,
        menu: &str,
        command: &str,
    ) -> &mut Self;

    /// The keys `keys` bound to the command `command`, written as
    /// `Primary+Shift+H`: Primary is Command on macOS and Control
    /// elsewhere.
    fn add_shortcut(
        &mut self,
        keys: &str,
        command: &str,
    ) -> &mut Self;

    /// A workspace the person can take, titled `title`.
    fn add_workspace(
        &mut self,
        name: &str,
        title: &str,
        layout: Layout,
    ) -> &mut Self;

    /// An item of the status line, after those that came before it,
    /// hidden until [`ui::set_status`](crate::ui::set_status) gives
    /// it words.
    fn add_status_item(&mut self, name: &str) -> &mut Self;

    /// An item of the status line that takes the room left on it,
    /// words or none, and clips its words.
    fn add_growing_status_item(&mut self, name: &str) -> &mut Self;

    /// The service `name` of the manifest, served by the system
    /// `serve`.
    fn add_service<M>(
        &mut self,
        name: &str,
        serve: impl IntoSystem<
            In<ServiceCall>,
            Result<Vec<u8>, String>,
            M,
        > + 'static,
    ) -> &mut Self;

    /// Subscribes to the event `event` (`plugin.id/name`) of another
    /// plugin as the app starts: each comes as an
    /// [`EventReceived`](crate::EventReceived).
    fn add_subscription(&mut self, event: &str) -> &mut Self;

    /// Puts in the default of `R`, kept across a reload: saved by the
    /// running version and restored into the next.
    fn init_saved_resource<R>(&mut self) -> &mut Self
    where
        R: Resource + Default + Serialize + DeserializeOwned;

    /// Declares `contribution` as it is.
    fn declare(&mut self, contribution: Contribution) -> &mut Self;

    /// The system building the view of the panel, settings page,
    /// footer or overlay `name`.
    fn add_view<M>(
        &mut self,
        name: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self;
}

impl HamletAppExt for App {
    fn add_panel<M>(
        &mut self,
        name: &str,
        title: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self {
        self.add_view(name, view).declare(Contribution::Panel {
            name: name.into(),
            title: title.into(),
            icon: None,
            padding: None,
        })
    }

    fn panel_icon(&mut self, icon: &str) -> &mut Self {
        if let Some(Contribution::Panel { icon: set, .. }) =
            self.world_mut().resource_mut::<Declared>().0.last_mut()
        {
            *set = Some(icon.into());
        }
        self
    }

    fn panel_padding(&mut self, space: Space) -> &mut Self {
        let space = match space {
            Space::None => contribution::Space::None,
            Space::Half => contribution::Space::Half,
            Space::Gap => contribution::Space::Gap,
            Space::Double => contribution::Space::Double,
        };
        self.world_mut()
            .resource_mut::<Declared>()
            .pad(PanelPadding::Space(space));
        self
    }

    fn panel_padding_pixels(&mut self, pixels: f32) -> &mut Self {
        self.world_mut()
            .resource_mut::<Declared>()
            .pad(PanelPadding::Pixels(pixels));
        self
    }

    fn add_panel_section<M>(
        &mut self,
        panel: &str,
        name: &str,
        title: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self {
        self.add_view(name, view).declare(
            Contribution::PanelSection {
                panel: panel.into(),
                name: name.into(),
                title: title.into(),
            },
        )
    }

    fn add_settings_page<M>(
        &mut self,
        name: &str,
        title: &str,
        about: &str,
        icon: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self {
        self.add_view(name, view).declare(
            Contribution::SettingsPage {
                name: name.into(),
                title: title.into(),
                about: about.into(),
                icon: icon.into(),
            },
        )
    }

    fn add_settings_footer<M>(
        &mut self,
        name: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self {
        self.add_view(name, view).declare(
            Contribution::SettingsFooter { name: name.into() },
        )
    }

    fn add_overlay<M>(
        &mut self,
        name: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self {
        self.add_view(name, view)
            .declare(Contribution::Overlay { name: name.into() })
    }

    fn add_command<M>(
        &mut self,
        name: &str,
        label: &str,
        run: impl IntoSystem<(), (), M> + 'static,
    ) -> &mut Self {
        let id = self.register_system(run);
        self.world_mut()
            .get_resource_or_init::<CommandSystems>()
            .0
            .insert(name.into(), id);
        self.declare(Contribution::Command {
            name: name.into(),
            label: label.into(),
        })
    }

    fn add_menu_entry(
        &mut self,
        menu: &str,
        command: &str,
    ) -> &mut Self {
        self.declare(Contribution::MenuEntry {
            menu: menu.into(),
            command: command.into(),
        })
    }

    fn add_shortcut(
        &mut self,
        keys: &str,
        command: &str,
    ) -> &mut Self {
        self.declare(Contribution::Shortcut {
            keys: keys.into(),
            command: command.into(),
        })
    }

    fn add_workspace(
        &mut self,
        name: &str,
        title: &str,
        layout: Layout,
    ) -> &mut Self {
        self.declare(Contribution::Workspace {
            name: name.into(),
            title: title.into(),
            layout,
        })
    }

    fn add_status_item(&mut self, name: &str) -> &mut Self {
        self.declare(Contribution::StatusItem {
            name: name.into(),
            grow: false,
        })
    }

    fn add_growing_status_item(&mut self, name: &str) -> &mut Self {
        self.declare(Contribution::StatusItem {
            name: name.into(),
            grow: true,
        })
    }

    fn add_service<M>(
        &mut self,
        name: &str,
        serve: impl IntoSystem<
            In<ServiceCall>,
            Result<Vec<u8>, String>,
            M,
        > + 'static,
    ) -> &mut Self {
        let id = self.register_system(serve);
        self.world_mut()
            .get_resource_or_init::<Services>()
            .0
            .insert(name.into(), id);
        self
    }

    fn add_subscription(&mut self, event: &str) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<Subscriptions>()
            .0
            .push(event.into());
        self
    }

    fn init_saved_resource<R>(&mut self) -> &mut Self
    where
        R: Resource + Default + Serialize + DeserializeOwned,
    {
        self.init_resource::<R>();
        self.world_mut()
            .get_resource_or_init::<SavedResources>()
            .0
            .push(SavedResource {
                name: core::any::type_name::<R>(),
                save: save::<R>,
                restore: restore::<R>,
            });
        self
    }

    fn declare(&mut self, contribution: Contribution) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<Declared>()
            .0
            .push(contribution);
        self
    }

    fn add_view<M>(
        &mut self,
        name: &str,
        view: impl IntoSystem<(), AnyView, M> + 'static,
    ) -> &mut Self {
        let id = self.register_system(view);
        self.world_mut()
            .get_resource_or_init::<Views>()
            .0
            .insert(name.into(), id);
        self
    }
}

/// What the plugin adds, in the order it was declared.
#[derive(Resource, Default)]
pub(crate) struct Declared(pub Vec<Contribution>);

impl Declared {
    /// Sets the padding of the last thing declared, if it is a panel.
    fn pad(&mut self, to: PanelPadding) {
        if let Some(Contribution::Panel { padding, .. }) =
            self.0.last_mut()
        {
            *padding = Some(to);
        }
    }
}

/// The system building each panel, settings page, footer and overlay,
/// by name.
#[derive(Resource, Default)]
pub(crate) struct Views(pub HashMap<String, SystemId<(), AnyView>>);

#[derive(Resource, Default)]
pub(crate) struct CommandSystems(pub HashMap<String, SystemId>);

#[derive(Resource, Default)]
pub(crate) struct Services(
    pub  HashMap<
        String,
        SystemId<In<ServiceCall>, Result<Vec<u8>, String>>,
    >,
);

#[derive(Resource, Default)]
struct Subscriptions(Vec<String>);

/// A resource kept across a reload, by its type's name.
struct SavedResource {
    name: &'static str,
    save: fn(&World) -> Option<Vec<u8>>,
    restore: fn(&mut World, &[u8]) -> Result<(), String>,
}

#[derive(Resource, Default)]
pub(crate) struct SavedResources(Vec<SavedResource>);

impl SavedResources {
    /// The bytes of every saved resource, by name.
    pub(crate) fn save(world: &World) -> Vec<u8> {
        let saved = world
            .get_resource::<Self>()
            .into_iter()
            .flat_map(|saved| &saved.0)
            .filter_map(|saved| {
                Some((saved.name, (saved.save)(world)?))
            })
            .collect::<Vec<_>>();
        encode(&saved).unwrap_or_else(|why| {
            crate::error(&format!(
                "the state cannot be saved: {why}"
            ));
            Vec::new()
        })
    }

    /// Puts back each resource `state` holds bytes of.
    pub(crate) fn restore(world: &mut World, state: &[u8]) {
        if state.is_empty() {
            return;
        }
        let state = match decode::<Vec<(String, Vec<u8>)>>(state) {
            Ok(state) => state,
            Err(why) => {
                crate::warn(&format!(
                    "the state is not taken: {why}"
                ));
                return;
            }
        };
        let restores = world
            .get_resource::<Self>()
            .into_iter()
            .flat_map(|saved| &saved.0)
            .map(|saved| (saved.name, saved.restore))
            .collect::<Vec<_>>();
        for (name, bytes) in state {
            let Some((_, restore)) =
                restores.iter().find(|(saved, _)| *saved == name)
            else {
                continue;
            };
            if let Err(why) = restore(world, &bytes) {
                crate::warn(&format!(
                    "{name} is not restored: {why}"
                ));
            }
        }
    }
}

fn save<R: Resource + Serialize>(world: &World) -> Option<Vec<u8>> {
    encode(world.get_resource::<R>()?).ok()
}

fn restore<R: Resource + DeserializeOwned>(
    world: &mut World,
    bytes: &[u8],
) -> Result<(), String> {
    let value = decode::<R>(bytes).map_err(|why| why.to_string())?;
    world.insert_resource(value);
    Ok(())
}

fn subscribe(subscriptions: Res<Subscriptions>) {
    for event in &subscriptions.0 {
        crate::bindings::hamlet::engine::host::subscribe(event);
    }
}

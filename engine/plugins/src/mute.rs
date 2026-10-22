//! Muting a plugin's sounds: SOUND on its page, a switch kept in the
//! person's sound settings by its id, and [`Muted`] on its entity
//! while it is muted. Uninstalling it forgets it.

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Has};
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy_fynix::resource;
use bevy_fynix::views::{FrameProps, column};
use hamlet_engine_audio::{Muted, SoundSettings};
use hamlet_engine_shell::style::{labelled_switch, section};
use hamlet_engine_shell::{AnyView, Cx};

use crate::list::{PluginPageSection, Uninstall};
use crate::{PluginInfo, PluginKind};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(forget)
        .add_systems(Startup, add)
        .add_systems(Update, mark);
}

fn add(mut commands: Commands) {
    commands.spawn(PluginPageSection { content: view });
}

/// SOUND on the page of `plugin`, over the switch that mutes it.
fn view(plugin: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(id) = cx
            .world
            .get::<PluginInfo>(plugin)
            .map(|info| info.id.clone())
        else {
            return cx.build(column(()));
        };
        let read = id.clone();
        let muted = labelled_switch(
            "Mute its sounds",
            resource::<SoundSettings, _>(move |settings| {
                settings.muted.contains(&read)
            }),
            move |world, on| {
                let mut settings =
                    world.resource_mut::<SoundSettings>();
                if on {
                    settings.muted.insert(id.clone());
                } else {
                    settings.muted.remove(&id);
                }
            },
        );
        let gap = cx.theme().space.gap / 2.0;
        cx.build(column((section("SOUND"), muted)).gap(gap))
    })
}

/// Marks each plugin the person muted, and only those.
fn mark(
    settings: Res<SoundSettings>,
    plugins: Query<(Entity, &PluginInfo, Has<Muted>)>,
    added: Query<(), Added<PluginInfo>>,
    mut commands: Commands,
) {
    if !settings.is_changed() && added.is_empty() {
        return;
    }
    for (plugin, info, marked) in &plugins {
        match (settings.muted.contains(&info.id), marked) {
            (true, false) => {
                commands.entity(plugin).insert(Muted);
            }
            (false, true) => {
                commands.entity(plugin).remove::<Muted>();
            }
            _ => {}
        }
    }
}

/// Takes a plugin being uninstalled off the muted.
fn forget(
    event: On<Uninstall>,
    plugins: Query<&PluginInfo>,
    mut settings: ResMut<SoundSettings>,
) {
    if let Ok(info) = plugins.get(event.entity)
        && info.kind == PluginKind::Installed
        && settings.muted.contains(&info.id)
    {
        settings.muted.remove(&info.id);
    }
}

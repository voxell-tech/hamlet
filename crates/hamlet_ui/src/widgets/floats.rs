//! Every popup, menu and tooltip comes in the same way, in light and
//! dark alike: on the card's fill, in its place at its full size,
//! fading in quickly, and casting the theme's shadow of what floats,
//! which fades in with it.
//!
//! The fade is laid on where the popup is drawn, as its parts are
//! handed to the renderer: each part's alpha is lowered by how far
//! the popup has come in. While it fades, its parts are handed over
//! anew every frame, so the fade is laid on afresh and never twice.

use bevy::app::{App, Plugin, PostUpdate};
use bevy::color::Alpha;
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::{ContainsEntity, Entity};
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::render::{Extract, ExtractSchedule, RenderApp};
use bevy::time::Time;
use bevy::ui::{BackgroundColor, BoxShadow, ComputedNode, UiSystems};
use bevy::ui_render::{
    ExtractedUiItem, ExtractedUiNodes, RenderUiSystems,
};
use bevy::ui_widgets::popover::Popover;

use super::effects::EffectSettings;
use crate::theme::Themed;

/// Brings popups in and casts their shadows.
pub struct FloatsPlugin;

impl Plugin for FloatsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                fill_card.before(UiSystems::Layout),
                arrive.after(UiSystems::PostLayout),
            ),
        );
    }

    // Once every plugin is built, so the render world is there; only
    // a window that draws has one.
    fn finish(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                ExtractSchedule,
                fade.after(RenderUiSystems::ExtractBackgrounds)
                    .after(RenderUiSystems::ExtractImages)
                    .after(RenderUiSystems::ExtractTextureSlice)
                    .after(RenderUiSystems::ExtractBorders)
                    .after(RenderUiSystems::ExtractTextBackgrounds)
                    .after(RenderUiSystems::ExtractTextShadows)
                    .after(RenderUiSystems::ExtractText)
                    .after(RenderUiSystems::ExtractCursor)
                    .after(RenderUiSystems::ExtractInlineImages),
            );
        }
    }
}

/// On a popup: how long it has been coming in, in seconds.
#[derive(Component, Default)]
struct Arrival {
    age: f32,
    /// Whether it has come all the way in.
    done: bool,
}

/// On each node of a popup coming in: how much of it is shown, 0 to
/// 1.
#[derive(Component, Clone, Copy)]
struct Fading(f32);

/// Gives a popup on the panel's fill the card's, a shade off the
/// panel under it in light and dark alike. The fill keeps the
/// opacity it is given, so a popup that fades out of its own accord
/// still does.
fn fill_card(
    theme: Themed,
    mut floats: Query<&mut BackgroundColor, With<Popover>>,
) {
    let palette = &theme.palette;
    for mut fill in &mut floats {
        let opacity = fill.0.alpha();
        if fill.0.with_alpha(1.0) == palette.surface.with_alpha(1.0) {
            fill.0 = palette.card.with_alpha(opacity);
        }
    }
}

/// Brings each popup in, on every node of it, and casts its shadow
/// as strongly as the popup shows: as far as it has come in, and as
/// opaque as its own fill is while it is held out of sight or fades
/// out.
fn arrive(
    time: Res<Time>,
    set: Res<EffectSettings>,
    theme: Themed,
    mut floats: Query<
        (
            Entity,
            Option<&mut Arrival>,
            Option<&BoxShadow>,
            &BackgroundColor,
        ),
        With<Popover>,
    >,
    children: Query<&Children>,
    mut nodes: Query<&mut ComputedNode>,
    mut commands: Commands,
) {
    let full = theme.shadow();
    for (float, arrival, cast, fill) in &mut floats {
        let (shown, coming) = match arrival {
            None => {
                commands.entity(float).try_insert(Arrival::default());
                (0.0, true)
            }
            Some(mut arrival) if !arrival.done => {
                arrival.age += time.delta_secs();
                let t = (arrival.age / set.popup_in).min(1.0);
                arrival.done = t >= 1.0;
                (1.0 - (1.0 - t).powi(3), true)
            }
            Some(_) => (1.0, false),
        };
        let strength = shown * fill.0.alpha();
        let mut shadow = full.clone();
        for style in &mut shadow.0 {
            style.color = style
                .color
                .with_alpha(style.color.alpha() * strength);
        }
        if cast != Some(&shadow) {
            commands.entity(float).try_insert(shadow);
        }
        if !coming {
            continue;
        }
        // Handed to the renderer anew, faded or at last whole.
        for node in core::iter::once(float)
            .chain(children.iter_descendants(float))
        {
            if shown < 1.0 {
                commands.entity(node).try_insert(Fading(shown));
            } else {
                commands.entity(node).try_remove::<Fading>();
            }
            if let Ok(mut computed) = nodes.get_mut(node) {
                computed.set_changed();
            }
        }
    }
}

/// Lowers the alpha of every part of a popup coming in, as it is
/// handed to the renderer.
fn fade(
    mut extracted: ResMut<ExtractedUiNodes>,
    fading: Extract<Query<&Fading>>,
) {
    let ExtractedUiNodes { uinodes, changed } = &mut *extracted;
    for main in changed.iter() {
        let Ok(Fading(shown)) = fading.get(main.entity()) else {
            continue;
        };
        let Some((_, items)) = uinodes.get_mut(main) else {
            continue;
        };
        for node in items.values_mut() {
            match &mut node.item {
                ExtractedUiItem::Node { color, .. } => {
                    color.alpha *= shown;
                }
                ExtractedUiItem::Glyphs { glyphs } => {
                    for glyph in glyphs.iter_mut() {
                        glyph.color.alpha *= shown;
                    }
                }
            }
        }
    }
}

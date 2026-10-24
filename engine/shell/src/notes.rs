//! What a panel tells the person without coming to the front: a
//! teal light across its top when what it shows changed, and while
//! something in it needs the person, a lamp over it and a glowing
//! badge on its tab.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::{
    BackgroundGradient, ColorStop, LinearGradient, PositionType,
    UiRect, Val, percent, px,
};
use bevy_fynix::ViewExt;
use bevy_fynix::dock::{DockTab, DockTree, TabBadge};
use bevy_fynix::views::{BehaviorExt, FrameProps, column, frame};
use hamlet_engine_api::PanelId;

use crate::AnyView;
use crate::theme::EngineTheme;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Notes>().add_systems(
        Update,
        (fade, light_panels, light_tabs).chain(),
    );
}

/// What a panel is told.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelNote {
    /// What it shows changed.
    Updated,
    /// Something in it needs the person, until it is told `None`.
    Attention,
    None,
}

/// How long a flash takes to die away, in seconds.
const FLASH: f32 = 1.6;

/// How far a panel's light reaches down from its top, in pixels.
const REACH: f32 = 90.0;

/// The rate of the lamp's and the badge's breathing, per second, and
/// the share of their light that breathes.
const BREATH: f32 = 2.4;
const BREATHING: f32 = 0.35;

/// The most of a panel's light, as an alpha, a flash and a lamp
/// throw, and the most of a tab's.
const FLASH_LIGHT: f32 = 0.34;
const LAMP_LIGHT: f32 = 0.2;
const TAB_LIGHT: f32 = 0.4;

/// What each panel was told and has not lost yet, by its id.
#[derive(Resource, Default)]
pub(crate) struct Notes(HashMap<String, Noted>);

/// The light of one panel.
#[derive(Clone, Copy, Default)]
struct Noted {
    /// Something in it needs the person.
    lamp: bool,
    /// What is left of the flash, from 1 to 0.
    flash: f32,
}

impl Notes {
    pub(crate) fn tell(&mut self, panel: &PanelId, note: PanelNote) {
        let noted =
            self.0.entry(panel.as_str().to_string()).or_default();
        match note {
            PanelNote::Updated => noted.flash = 1.0,
            PanelNote::Attention => noted.lamp = true,
            PanelNote::None => noted.lamp = false,
        }
    }

    fn of(&self, panel: &str) -> Noted {
        self.0.get(panel).copied().unwrap_or_default()
    }
}

/// The light over the top of a panel, of the panel `0`.
#[derive(Component, Clone, Debug)]
pub(crate) struct PanelGlow(String);

/// `content`, the content of the panel `id`, with the light that
/// falls into it over its top.
pub(crate) fn glowing(id: String, content: AnyView) -> AnyView {
    column((
        content,
        frame()
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(0.0),
                right: px(0.0),
                top: px(0.0),
                bottom: Val::Auto,
            })
            .height(px(REACH))
            .tagged(PanelGlow(id))
            .tagged(Pickable::IGNORE),
    ))
    .grow(1.0)
    .min_width(px(0.0))
    .min_height(px(0.0))
    .gap(0.0)
    .boxed()
}

/// Lets each flash die away, and forgets a panel with no light.
fn fade(time: Res<Time>, mut notes: ResMut<Notes>) {
    if notes.0.is_empty() {
        return;
    }
    let dt = time.delta_secs() / FLASH;
    notes.0.retain(|_, noted| {
        noted.flash = (noted.flash - dt).max(0.0);
        noted.lamp || noted.flash > 0.0
    });
}

/// The breath of the lamp, from `1 - BREATHING` to 1.
fn breath(time: &Time) -> f32 {
    1.0 - BREATHING
        * (0.5 + 0.5 * (time.elapsed_secs() * BREATH).sin())
}

/// A light of `color` that is `alpha` at its start and none at its
/// end, toward the end of its node that `down` has it.
fn gradient(
    color: Color,
    alpha: f32,
    down: bool,
) -> BackgroundGradient {
    let stops = vec![
        ColorStop::new(color.with_alpha(alpha), percent(0.0)),
        ColorStop::new(color.with_alpha(0.0), percent(100.0)),
    ];
    BackgroundGradient::from(if down {
        LinearGradient::to_bottom(stops)
    } else {
        LinearGradient::to_top(stops)
    })
}

/// The light of `noted`: its colour and its alpha, the teal of a
/// flash while there is one, and the lamp's amber otherwise.
fn light_of(
    theme: &EngineTheme,
    noted: Noted,
    time: &Time,
    (flash, lamp): (f32, f32),
) -> Option<(Color, f32)> {
    let colors = &theme.colors;
    if noted.flash > 0.0 {
        Some((*colors.info, noted.flash * flash))
    } else if noted.lamp {
        Some((*colors.active, lamp * breath(time)))
    } else {
        None
    }
}

/// Lights the top of each panel that was told something.
fn light_panels(
    time: Res<Time>,
    theme: Res<EngineTheme>,
    notes: Res<Notes>,
    glows: Query<(Entity, &PanelGlow, Option<&BackgroundGradient>)>,
    mut commands: Commands,
) {
    for (node, glow, now) in &glows {
        let light = light_of(
            &theme,
            notes.of(&glow.0),
            &time,
            (FLASH_LIGHT, LAMP_LIGHT),
        );
        match light {
            Some((color, alpha)) => {
                let want = gradient(color, alpha, true);
                if now != Some(&want) {
                    commands.entity(node).try_insert(want);
                }
            }
            None if now.is_some() => {
                commands
                    .entity(node)
                    .try_remove::<BackgroundGradient>();
            }
            None => {}
        }
    }
}

/// Lights each tab behind another that was told something, and the
/// badge of one that needs the person.
fn light_tabs(
    time: Res<Time>,
    theme: Res<EngineTheme>,
    notes: Res<Notes>,
    tree: Res<DockTree>,
    tabs: Query<(Entity, &DockTab, Option<&BackgroundGradient>)>,
    parents: Query<&ChildOf>,
    mut badges: Query<
        (Entity, &mut Visibility, &mut BackgroundColor),
        With<TabBadge>,
    >,
    mut commands: Commands,
) {
    // What each tab was told, unless its area shows it.
    let told = |tab: &DockTab| {
        let leaf = tree.leaf(tab.leaf)?;
        let entry = leaf.windows.iter().find(|e| e.id == tab.tab)?;
        (leaf.active != Some(tab.tab))
            .then(|| notes.of(&entry.window_id))
    };
    for (node, tab, now) in &tabs {
        let noted = told(tab).unwrap_or_default();
        let light =
            light_of(&theme, noted, &time, (TAB_LIGHT, TAB_LIGHT));
        match light {
            Some((color, alpha)) => {
                let want = gradient(color, alpha, false);
                if now != Some(&want) {
                    commands.entity(node).try_insert(want);
                }
            }
            None if now.is_some() => {
                commands
                    .entity(node)
                    .try_remove::<BackgroundGradient>();
            }
            None => {}
        }
    }
    for (badge, mut visibility, mut fill) in &mut badges {
        let noted = parents
            .iter_ancestors(badge)
            .find_map(|ancestor| tabs.get(ancestor).ok())
            .and_then(|(_, tab, _)| told(tab))
            .unwrap_or_default();
        let (shown, color) = if noted.lamp {
            (
                Visibility::Inherited,
                theme.colors.active.with_alpha(breath(&time)),
            )
        } else {
            (Visibility::Hidden, Color::NONE)
        };
        visibility.set_if_neq(shown);
        fill.set_if_neq(BackgroundColor(color));
    }
}

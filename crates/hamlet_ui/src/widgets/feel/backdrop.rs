//! The light that falls into a panel: a lamp while something waits
//! for the chief, and a flash when the agent it shows changes.

use bevy::asset::{Asset, Assets};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{With, Without};
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::math::Vec4;
use bevy::reflect::TypePath;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::time::Time;
use bevy::ui::ComputedNode;
use bevy::ui_render::prelude::{MaterialNode, UiMaterial};
use hamlet_client::waits::Waiting;

use super::{FeelSettings, MagicColors, ease, linear};
use crate::me::Me;
use crate::theme::Themed;

/// The light that falls into a panel.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct Backdrop {
    /// rgb: the lamplight. a: the amount it is turned up.
    #[uniform(0)]
    alarm: Vec4,
    /// rgb: the colour of the flash. a: the share of it that shows.
    #[uniform(1)]
    flash: Vec4,
    /// rgb: the heart of the flame.
    #[uniform(2)]
    heart: Vec4,
    /// x: the share of its light the flame wavers by. w: physical
    /// pixels to a logical one.
    #[uniform(3)]
    shape: Vec4,
}

impl UiMaterial for Backdrop {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_ui/widgets/feel/backdrop.wgsl".into()
    }
}

/// A panel that light falls into.
#[derive(Component, Clone, Copy, Default)]
pub struct Lit {
    /// A lamp is lit over it while something waits for the chief.
    pub alarm: bool,
    /// It flashes when the agent it shows changes.
    pub flash: bool,
}

pub(super) fn lay(
    mut commands: Commands,
    set: Res<FeelSettings>,
    theme: Themed,
    mut backdrops: ResMut<Assets<Backdrop>>,
    panels: Query<(Entity, &Lit), Without<MaterialNode<Backdrop>>>,
) {
    let colors = MagicColors::of(&theme);
    for (panel, lit) in &panels {
        // A panel no light falls into has nothing to draw.
        if !lit.alarm && !lit.flash {
            continue;
        }
        let backdrop = Backdrop {
            alarm: linear(colors.alarm).extend(0.0),
            flash: linear(colors.flash).extend(0.0),
            heart: linear(colors.flame).extend(1.0),
            shape: Vec4::new(set.waver, 0.0, 0.0, 1.0),
        };
        commands
            .entity(panel)
            .try_insert(MaterialNode(backdrops.add(backdrop)));
    }
}

/// Carries a change of the colours to every backdrop.
pub(super) fn tint(
    theme: Themed,
    mut backdrops: ResMut<Assets<Backdrop>>,
) {
    let colors = MagicColors::of(&theme);
    for (_, backdrop) in backdrops.iter_mut() {
        backdrop.alarm =
            linear(colors.alarm).extend(backdrop.alarm.w);
        backdrop.flash =
            linear(colors.flash).extend(backdrop.flash.w);
        backdrop.heart = linear(colors.flame).extend(1.0);
    }
}

/// Lights a panel's lamp while something waits, turns it up for a
/// moment as something new comes, and flashes a panel when the agent
/// it shows changes.
pub(super) fn light(
    time: Res<Time>,
    set: Res<FeelSettings>,
    me: Res<Me>,
    waiting: Query<(), With<Waiting>>,
    panels: Query<(&MaterialNode<Backdrop>, &Lit, &ComputedNode)>,
    mut backdrops: ResMut<Assets<Backdrop>>,
    mut shown: Local<Option<Entity>>,
    mut counted: Local<usize>,
) {
    let dt = time.delta_secs();
    let changed = shown.replace(me.agent) != Some(me.agent);
    let count = waiting.iter().count();
    let rung = count > *counted;
    *counted = count;
    let alarm = f32::from(count > 0);
    for (node, lit, computed) in &panels {
        let scale = computed.inverse_scale_factor.recip();
        let Some(now) = backdrops.get(&node.0) else {
            continue;
        };
        let raised = if !lit.alarm {
            0.0
        } else if rung {
            now.alarm.w.max(alarm) + set.ring
        } else {
            ease(now.alarm.w, alarm, set.settle, dt)
        };
        let flash = if lit.flash && changed {
            1.0
        } else {
            ease(now.flash.w, 0.0, set.settle, dt)
        };
        if raised == now.alarm.w
            && flash == now.flash.w
            && scale == now.shape.w
        {
            continue;
        }
        if let Some(mut backdrop) = backdrops.get_mut(&node.0) {
            backdrop.alarm.w = raised;
            backdrop.flash.w = flash;
            backdrop.shape.w = scale;
        }
    }
}

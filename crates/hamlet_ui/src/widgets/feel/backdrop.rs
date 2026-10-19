//! The light that falls into a panel: a lamp while something waits
//! for the chief, and a flash when the agent it shows changes.

use bevy::asset::{Asset, Assets};
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
use hamlet_agents_api::convo::Me;
use hamlet_core::oversight::Waiting;
use hamlet_sdk::feel::Lit;

use super::{FeelSettings, MagicColors, ease, linear};
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
#[allow(clippy::too_many_arguments)]
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

#[cfg(test)]
mod tests {
    use bevy::app::{App, Update};
    use bevy::ecs::schedule::IntoScheduleConfigs;
    use bevy_fynix::Theme;
    use hamlet_assets::Tuning;

    use super::super::tests::step;
    use super::*;
    use crate::theme::HamletTheme;

    /// A world with one panel of each kind, and their backdrops.
    fn panels(app: &mut App) -> (Entity, Entity, Entity) {
        app.init_resource::<FeelSettings>()
            .insert_resource(Theme(HamletTheme::designed()))
            .init_resource::<Time>()
            .init_resource::<Assets<Backdrop>>()
            .add_systems(Update, (lay, light).chain());
        let agent = app.world_mut().spawn_empty().id();
        app.world_mut()
            .insert_resource(Me::of(agent, agent, "", None));
        let node = ComputedNode {
            inverse_scale_factor: 1.0,
            ..ComputedNode::default()
        };
        let mut panel = |alarm, flash| {
            app.world_mut().spawn((Lit { alarm, flash }, node)).id()
        };
        (panel(true, false), panel(false, true), panel(false, false))
    }

    fn lamp_and_flash(app: &App, panel: Entity) -> (f32, f32) {
        let node =
            app.world().get::<MaterialNode<Backdrop>>(panel).unwrap();
        let backdrops = app.world().resource::<Assets<Backdrop>>();
        let now = backdrops.get(&node.0).unwrap();
        (now.alarm.w, now.flash.w)
    }

    fn wait(app: &mut App) -> Entity {
        app.world_mut()
            .spawn(Waiting {
                id: String::new(),
                kind: hamlet_core::oversight::Kind::Question,
                by: String::new(),
                about: String::new(),
                since: std::time::Instant::now(),
                order: 0,
                asked: 1,
                answers: Vec::new(),
                session: None,
            })
            .id()
    }

    #[test]
    fn a_lamp_is_lit_while_something_waits_and_rung_as_it_comes() {
        let mut app = App::new();
        let (needs, selection, plain) = panels(&mut app);
        for _ in 0..3 {
            step(&mut app);
        }
        assert_eq!(lamp_and_flash(&app, needs).0, 0.0);
        // A panel no light falls into has nothing to draw.
        assert!(
            app.world()
                .get::<MaterialNode<Backdrop>>(plain)
                .is_none()
        );

        let need = wait(&mut app);
        step(&mut app);
        let set = FeelSettings::default();
        let rung = lamp_and_flash(&app, needs).0;
        assert_eq!(rung, 1.0 + set.ring, "turned up past its rest");
        assert_eq!(lamp_and_flash(&app, selection).0, 0.0);
        for _ in 0..300 {
            step(&mut app);
        }
        assert_eq!(lamp_and_flash(&app, needs).0, 1.0, "then steady");

        app.world_mut().entity_mut(need).despawn();
        for _ in 0..300 {
            step(&mut app);
        }
        assert_eq!(lamp_and_flash(&app, needs).0, 0.0, "and put out");
    }

    #[test]
    fn a_change_of_agent_flashes_the_selection_alone() {
        let mut app = App::new();
        let (needs, selection, _) = panels(&mut app);
        for _ in 0..300 {
            step(&mut app);
        }
        assert_eq!(lamp_and_flash(&app, selection).1, 0.0);

        let other = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<Me>().agent = other;
        step(&mut app);
        assert_eq!(lamp_and_flash(&app, selection).1, 1.0);
        assert_eq!(lamp_and_flash(&app, needs).1, 0.0);
        let colors = MagicColors::of(&HamletTheme::designed());
        let node = app
            .world()
            .get::<MaterialNode<Backdrop>>(selection)
            .unwrap();
        let backdrops = app.world().resource::<Assets<Backdrop>>();
        assert_eq!(
            backdrops.get(&node.0).unwrap().flash.truncate(),
            linear(colors.flash)
        );
        for _ in 0..300 {
            step(&mut app);
        }
        assert_eq!(lamp_and_flash(&app, selection).1, 0.0);
    }
}

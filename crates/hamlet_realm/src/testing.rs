//! Helpers the tests of several modules share.

use bevy::prelude::*;
use hamlet_client::realm::Seen;
use hamlet_client::receive;
use hamlet_protocol::client as protocol;

/// Receives `state` as the server's whole state.
pub(crate) fn receive_state(app: &mut App, state: protocol::State) {
    receive(app.world_mut(), protocol::ServerMessage::State(state));
}

/// Receives `update` as the next revision after the one seen.
pub(crate) fn receive_change(
    app: &mut App,
    update: protocol::Update,
) {
    let seen = app.world().resource::<Seen>();
    let frame = protocol::ServerMessage::Change(protocol::Changes {
        epoch: seen.epoch,
        revision: seen.revision + 1,
        changes: vec![update],
    });
    receive(app.world_mut(), frame);
}

/// A world camera at `eye` looking at `target`, with the projection
/// and target size a renderer would compute, which a test has none
/// of.
pub(crate) fn camera(app: &mut App, eye: Vec3, target: Vec3) {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};

    let at =
        Transform::from_translation(eye).looking_at(target, Vec3::Y);
    app.world_mut().spawn((
        crate::WorldCamera,
        Camera {
            computed: ComputedCameraValues {
                clip_from_view: Mat4::perspective_infinite_reverse_rh(
                    std::f32::consts::FRAC_PI_4,
                    4.0 / 3.0,
                    0.1,
                ),
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(800, 600),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        at,
        GlobalTransform::from(at),
    ));
}

//! Telling the chief that a command did not go as asked.

use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Res};
use bevy::ecs::world::World;
use hamlet_core::session::{Reply, Transcript};

use crate::convo::Me;

/// Words for the chief, put in the open chat as a notice.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Notice(pub String);

pub(super) fn notice(
    notice: On<Notice>,
    me: Option<Res<Me>>,
    mut commands: Commands,
) {
    let Some(me) = me else {
        return;
    };
    let (session, text) = (me.session, notice.0.clone());
    commands.queue(move |world: &mut World| {
        if let Some(mut said) = world.get_mut::<Transcript>(session) {
            said.apply(&Reply::Notice(text));
        }
    });
}

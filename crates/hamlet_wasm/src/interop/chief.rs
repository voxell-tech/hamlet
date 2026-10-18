//! A command the chief runs from a menu or a key: it goes to the
//! plugin that owns it, as a call from the sender `chief`.

use bevy::prelude::*;
use hamlet_sdk::command::Forward;
use hamlet_sdk::plugin::{PluginInfo, Status};

use super::calls::Calls;
use super::mail::send;
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    CommandCall, Incoming,
};
use crate::limits::Limits;

/// The sender the plugin sees on a command the chief ran.
pub const CHIEF: &str = "chief";

pub(super) fn forward(
    forward: On<Forward>,
    plugins: Query<&PluginInfo>,
    mut inboxes: Query<(&Status, &mut Inbox)>,
    mut calls: ResMut<Calls>,
    limits: Res<Limits>,
) {
    let Ok(info) = plugins.get(forward.plugin) else {
        return;
    };
    let id = calls.open_for_chief(
        &forward.command,
        (forward.plugin, &info.id),
        limits.call_frames,
    );
    let call = CommandCall {
        call_id: id,
        sender: CHIEF.into(),
        command: forward.command.clone(),
        args: Vec::new(),
    };
    if send(&mut inboxes, forward.plugin, Incoming::Command(call)) {
        info!("the chief runs {}/{}", info.id, forward.command);
    } else {
        calls.close(id);
    }
}

//! What a plugin asks for that it does not yet hold.

use bevy::log::warn;
use hamlet_sdk::capability::{Capability, Verdict};

use super::State;
use crate::interface::hamlet::plugin::host::RequestResult;

/// A capability a plugin tried to use, or asked for, while the chief
/// was yet to decide.
#[derive(Clone, PartialEq, Debug)]
pub struct Want {
    pub capability: Capability,
    /// What a scoped capability is wanted for, such as another
    /// plugin.
    pub target: Option<String>,
    /// The plugin's own reason, if it gave one.
    pub reason: String,
}

impl State {
    pub(super) fn ask_for(
        &mut self,
        name: &str,
        reason: &str,
    ) -> RequestResult {
        let Ok(capability) = Capability::parse(name) else {
            warn!("plugin {}: asked for `{name}`", self.plugin);
            return RequestResult::Denied;
        };
        match self.granted.verdict(capability) {
            Some(Verdict::Allowed) => RequestResult::Granted,
            Some(Verdict::Ask) => {
                self.want(capability, reason);
                RequestResult::Pending
            }
            Some(Verdict::Denied) | None => {
                warn!("plugin {}: {name} not granted", self.plugin);
                RequestResult::Denied
            }
        }
    }

    /// Notes that the plugin wants `capability`, once for each call.
    pub(super) fn want(
        &mut self,
        capability: Capability,
        reason: &str,
    ) {
        self.want_for(capability, None, reason);
    }

    /// Notes that the plugin wants `capability` for `target`, once
    /// for each call.
    pub(crate) fn want_for(
        &mut self,
        capability: Capability,
        target: Option<&str>,
        reason: &str,
    ) {
        let target = target.map(str::to_string);
        if self.wants.iter().any(|want| {
            want.capability == capability && want.target == target
        }) {
            return;
        }
        let mut reason = reason.to_string();
        reason.truncate(reason.floor_char_boundary(self.bytes));
        self.wants.push(Want {
            capability,
            target,
            reason,
        });
    }
}

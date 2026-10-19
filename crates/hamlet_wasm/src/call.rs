//! Calling a plugin: `entry` once, `step` for each inbox and
//! `panel-view` for each panel to draw, each under the time limit.

use hamlet_sdk::capability::{Capability, Verdict};
use wasmtime::Store;

use crate::fault::Fault;
use crate::host::{Declared, Logged, Offers, State, Want};
use crate::instance::Instance;
use crate::interface::Hamlet;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent,
};
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::Limits;
use crate::wire::{self, Shown};

/// What a finished call left.
#[derive(Default, Debug, PartialEq)]
pub struct Output {
    pub intents: Vec<Intent>,
    pub logs: Vec<Logged>,
    /// The panels the call declared. Only `entry` may.
    pub panels: Vec<Declared>,
    /// The commands, menu lines and shortcuts the call declared.
    /// Only `entry` may.
    pub offers: Offers,
    /// The collections that changed in the call, each as the plugin
    /// that shares it and its name.
    pub changes: Vec<(String, String)>,
    /// What the call tried to use that the chief is yet to decide.
    pub wants: Vec<Want>,
}

/// What a call to `panel-view` returned.
#[derive(Debug, PartialEq)]
pub struct Drawing {
    pub shown: Shown,
    pub logs: Vec<Logged>,
    pub wants: Vec<Want>,
}

impl Instance {
    /// Runs the plugin's `entry`.
    pub fn entry(
        &mut self,
        limits: &Limits,
    ) -> Result<Output, Fault> {
        let (_, output) = self.run(limits, |exports, store| {
            exports.hamlet_plugin_plugin().call_entry(store)
        })?;
        Ok(output)
    }

    /// Runs the plugin's `step` over an inbox.
    pub fn step(
        &mut self,
        limits: &Limits,
        inbox: &[Incoming],
    ) -> Result<Output, Fault> {
        let (outgoing, mut output) =
            self.run(limits, |exports, store| {
                exports.hamlet_plugin_plugin().call_step(store, inbox)
            })?;
        output.intents = outgoing.intents;
        let over = output
            .intents
            .len()
            .saturating_sub(limits.intents_per_call);
        if over > 0 {
            output.intents.truncate(limits.intents_per_call);
            output.logs.push(Logged {
                level: Level::Warn,
                message: format!(
                    "{over} intents over the limit of {} were \
                     dropped",
                    limits.intents_per_call
                ),
            });
        }
        output.panels.clear();
        output.offers = Offers::default();
        Ok(output)
    }

    /// Runs the plugin's `panel-view` for the panel it calls `panel`.
    pub fn panel_view(
        &mut self,
        limits: &Limits,
        panel: &str,
    ) -> Result<Drawing, Fault> {
        let (view, output) = self.run(limits, |exports, store| {
            exports
                .hamlet_plugin_plugin()
                .call_panel_view(store, panel)
        })?;
        let mut shown = wire::shown(view, limits).map_err(|why| {
            Fault::Trap(format!(
                "panel-view returned a bad tree: {why}"
            ))
        })?;
        let mut wants = output.wants;
        let granted = &self.store.data().granted;
        if let Shown::Tree(root) = &mut shown
            && !granted.holds(Capability::UiCanvas)
            && wire::without_canvases(root)
            && granted.verdict(Capability::UiCanvas)
                == Some(Verdict::Ask)
        {
            wants.push(Want {
                capability: Capability::UiCanvas,
                target: None,
                reason: String::new(),
            });
        }
        Ok(Drawing {
            shown,
            logs: output.logs,
            wants,
        })
    }

    /// The fuel the last call used of the `call_fuel` of `limits` it
    /// was given.
    pub fn fuel_used(&self, limits: &Limits) -> u64 {
        let left = self.store.get_fuel().unwrap_or(0);
        limits.call_fuel.saturating_sub(left)
    }

    fn run<T>(
        &mut self,
        limits: &Limits,
        call: impl FnOnce(
            &Hamlet,
            &mut Store<State>,
        ) -> wasmtime::Result<T>,
    ) -> Result<(T, Output), Fault> {
        self.store.data_mut().logs.clear();
        self.store.data_mut().declared.clear();
        self.store.data_mut().offered = Offers::default();
        self.store.data_mut().changes.clear();
        self.store.data_mut().wants.clear();
        self.store.set_epoch_deadline(limits.call_ticks());
        self.store.set_fuel(limits.call_fuel).map_err(Fault::load)?;
        self.store.set_hostcall_fuel(limits.lift_bytes);
        let returned = call(&self.exports, &mut self.store)
            .map_err(Fault::call)?;
        let state = self.store.data_mut();
        let output = Output {
            intents: Vec::new(),
            logs: std::mem::take(&mut state.logs),
            panels: std::mem::take(&mut state.declared),
            offers: std::mem::take(&mut state.offered),
            changes: std::mem::take(&mut state.changes),
            wants: std::mem::take(&mut state.wants),
        };
        Ok((returned, output))
    }
}

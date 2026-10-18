//! Calling a plugin: `entry` once, `step` for each inbox and
//! `panel-view` for each panel to draw, each under the time limit.

use hamlet_sdk::tree::node::Node;
use wasmtime::Store;

use crate::fault::Fault;
use crate::host::{Declared, Logged, State};
use crate::instance::Instance;
use crate::interface::Hamlet;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent,
};
use crate::limits::Limits;
use crate::wire;

/// What a finished call left.
#[derive(Default, Debug, PartialEq)]
pub struct Output {
    pub intents: Vec<Intent>,
    pub logs: Vec<Logged>,
    /// The panels the call declared. Only `entry` may.
    pub panels: Vec<Declared>,
}

/// What a call to `panel-view` returned.
#[derive(Debug, PartialEq)]
pub struct Drawing {
    pub root: Node,
    pub logs: Vec<Logged>,
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
        output.panels.clear();
        Ok(output)
    }

    /// Runs the plugin's `panel-view` for the panel it calls `panel`.
    pub fn panel_view(
        &mut self,
        limits: &Limits,
        panel: &str,
    ) -> Result<Drawing, Fault> {
        let (tree, output) = self.run(limits, |exports, store| {
            exports
                .hamlet_plugin_plugin()
                .call_panel_view(store, panel)
        })?;
        let root = wire::node(tree, limits).map_err(|why| {
            Fault::Trap(format!(
                "panel-view returned a bad tree: {why}"
            ))
        })?;
        Ok(Drawing {
            root,
            logs: output.logs,
        })
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
        self.store.set_epoch_deadline(limits.call_ticks());
        let returned = call(&self.exports, &mut self.store)
            .map_err(Fault::call)?;
        let state = self.store.data_mut();
        let output = Output {
            intents: Vec::new(),
            logs: std::mem::take(&mut state.logs),
            panels: std::mem::take(&mut state.declared),
        };
        Ok((returned, output))
    }
}

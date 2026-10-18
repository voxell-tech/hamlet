//! Calling a plugin: `entry` once, `step` for each inbox, each under
//! the time limit.

use wasmtime::Store;

use crate::fault::Fault;
use crate::host::{Declared, Logged, State};
use crate::instance::Instance;
use crate::interface::Hamlet;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent,
};
use crate::limits::Limits;

/// What a finished call left.
#[derive(Default, Debug, PartialEq)]
pub struct Output {
    pub intents: Vec<Intent>,
    pub logs: Vec<Logged>,
    /// The panels the call declared. Only `entry` may.
    pub panels: Vec<Declared>,
}

impl Instance {
    /// Runs the plugin's `entry`.
    pub fn entry(
        &mut self,
        limits: &Limits,
    ) -> Result<Output, Fault> {
        self.run(limits, |exports, store| {
            exports.hamlet_plugin_plugin().call_entry(store)?;
            Ok(Vec::new())
        })
    }

    /// Runs the plugin's `step` over an inbox.
    pub fn step(
        &mut self,
        limits: &Limits,
        inbox: &[Incoming],
    ) -> Result<Output, Fault> {
        let mut output = self.run(limits, |exports, store| {
            let outgoing = exports
                .hamlet_plugin_plugin()
                .call_step(store, inbox)?;
            Ok(outgoing.intents)
        })?;
        output.panels.clear();
        Ok(output)
    }

    fn run(
        &mut self,
        limits: &Limits,
        call: impl FnOnce(
            &Hamlet,
            &mut Store<State>,
        ) -> wasmtime::Result<Vec<Intent>>,
    ) -> Result<Output, Fault> {
        self.store.data_mut().logs.clear();
        self.store.data_mut().declared.clear();
        self.store.set_epoch_deadline(limits.call_ticks());
        let intents = call(&self.exports, &mut self.store)
            .map_err(Fault::call)?;
        let state = self.store.data_mut();
        Ok(Output {
            intents,
            logs: std::mem::take(&mut state.logs),
            panels: std::mem::take(&mut state.declared),
        })
    }
}

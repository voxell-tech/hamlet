//! A running plugin: its component instantiated in a store of its
//! own, and each call into it held to the plugin's budgets.
//!
//! Every value a plugin hands the host crosses as a string or a list
//! of bytes. wasmtime checks its pointer and length against the
//! plugin's memory, and takes its length from the store's hostcall
//! fuel, before it copies a byte; the fuel is set to
//! [`Budgets::bytes_per_call`] before every call.

use core::fmt;
use std::time::{Duration, Instant};

use bevy::ecs::component::Component;
use hamlet_engine_api::{
    AssetId, Contribution, Request, Ticket, WidgetEvent, WidgetTree,
    decode, encode,
};
use hamlet_engine_core::{Budgets, LogLevel};
use wasmtime::component::{Component as Compiled, HasSelf, Linker};
use wasmtime::{Engine, ResourceLimiter, Store, Trap};

use crate::host::WasmTuning;

mod bindings {
    wasmtime::component::bindgen!({
        path: "../wit/hamlet_engine.wit",
        world: "plugin",
    });
}

use self::bindings::exports::hamlet::engine::guest::Guest;
use self::bindings::hamlet::engine::host::{self, Level};
use self::bindings::{Plugin, PluginPre};

/// The interface of the host a plugin may import, besides the
/// system's (`wasi:`), whose calls trap.
const HOST: &str = "hamlet:engine/host@0.1.0";

/// The text wasmtime gives a call that copied more than the
/// hostcall fuel allows. It names the error in its text only.
const TOO_MUCH: &str =
    "fuel allocated for hostcalls has been exhausted";

/// What one call may spend.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub fuel: u64,
    /// Ticks of the epoch.
    pub ticks: u64,
    /// Bytes copied in or out.
    pub bytes: usize,
    /// Lines logged, and the bytes of one.
    pub lines: usize,
    pub line_bytes: usize,
    /// Panels asked to be drawn again.
    pub redraws: usize,
    /// Requests made of the engine's services.
    pub requests: usize,
    /// Bytes of all the plugin's memories.
    pub memory: usize,
}

impl Caps {
    pub fn of(budgets: &Budgets, tuning: &WasmTuning) -> Self {
        Self {
            fuel: budgets.fuel_per_call,
            ticks: tuning.call_ticks(budgets),
            bytes: budgets.bytes_per_call as usize,
            lines: budgets.log_lines_per_second as usize,
            line_bytes: budgets.log_line_bytes,
            redraws: budgets.contributions,
            requests: budgets.requests_per_call,
            memory: budgets.memory as usize,
        }
    }
}

/// Why a plugin is stopped: a call that failed, after which it cannot
/// be called again, or budgets of a second gone past too often.
#[derive(Clone, PartialEq, Debug)]
pub enum Fault {
    /// It went past a budget of a second this many seconds in a row.
    Over(u32),
    /// It used more than its fuel for one call.
    Fuel,
    /// It took longer than one call may.
    Time,
    /// Its memories grew past its budget.
    Memory,
    /// It handed over more bytes than one call may.
    Bytes,
    /// It trapped: why.
    Trap(String),
    /// It gave what cannot be read: why.
    Unreadable(String),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Over(seconds) => write!(
                f,
                "it went over its budgets {seconds} seconds in a row"
            ),
            Self::Fuel => {
                write!(f, "it used more than its fuel for one call")
            }
            Self::Time => {
                write!(f, "it took longer than one call may")
            }
            Self::Memory => {
                write!(f, "its memory grew past its budget")
            }
            Self::Bytes => {
                write!(
                    f,
                    "it handed over more bytes than one call may"
                )
            }
            Self::Trap(why) => write!(f, "it failed: {why}"),
            Self::Unreadable(why) => {
                write!(f, "it gave what cannot be read: {why}")
            }
        }
    }
}

impl Fault {
    fn of(error: wasmtime::Error) -> Self {
        if error.is::<OverMemory>() {
            return Self::Memory;
        }
        match error.downcast_ref::<Trap>() {
            Some(Trap::OutOfFuel) => Self::Fuel,
            Some(Trap::Interrupt) => Self::Time,
            _ if format!("{error:#}").contains(TOO_MUCH) => {
                Self::Bytes
            }
            _ => Self::Trap(format!("{error:#}")),
        }
    }
}

/// What a call spent.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Spent {
    pub time: Duration,
    pub fuel: u64,
}

/// A plugin's component, compiled and linked, ready to be
/// instantiated.
#[derive(Clone)]
pub struct Linked(PluginPre<State>);

/// Compiles and links the component in `bytes`.
pub fn link(engine: &Engine, bytes: &[u8]) -> Result<Linked, String> {
    let component =
        Compiled::new(engine, bytes).map_err(|e| format!("{e:#}"))?;
    let ty = component.component_type();
    for (name, _) in ty.imports(engine) {
        if name.starts_with("wasi:") || name == HOST {
            continue;
        }
        if name.starts_with("hamlet:engine/") {
            return Err(format!(
                "it is built against {name}, and this engine has \
                 {HOST}"
            ));
        }
        return Err(format!(
            "it needs {name}, which the engine does not give"
        ));
    }
    let mut linker = Linker::new(engine);
    let linked = host::add_to_linker::<State, HasSelf<State>>(
        &mut linker,
        |state| state,
    )
    .and_then(|()| linker.define_unknown_imports_as_traps(&component))
    .and_then(|()| linker.instantiate_pre(&component))
    .and_then(PluginPre::new);
    linked.map(Linked).map_err(|e| format!("{e:#}"))
}

/// What the host keeps for a plugin while it is called.
pub struct State {
    limiter: Limiter,
    caps: Caps,
    logs: Vec<(LogLevel, String)>,
    redraws: Vec<String>,
    requests: Vec<(Ticket, Request)>,
    /// The ticket the last request was given.
    ticket: Ticket,
}

impl State {
    /// Takes `request`, within the call's cap: its ticket, or 0.
    fn ask(&mut self, request: Request) -> Ticket {
        if self.requests.len() >= self.caps.requests {
            return 0;
        }
        self.ticket += 1;
        self.requests.push((self.ticket, request));
        self.ticket
    }
}

impl host::Host for State {
    fn create_asset(
        &mut self,
        kind: String,
        name: String,
        bytes: Vec<u8>,
    ) -> Ticket {
        self.ask(Request::CreateAsset { kind, name, bytes })
    }

    fn read_asset(&mut self, id: u64) -> Ticket {
        self.ask(Request::ReadAsset(AssetId(id)))
    }

    fn update_asset(&mut self, id: u64, bytes: Vec<u8>) -> Ticket {
        self.ask(Request::UpdateAsset {
            id: AssetId(id),
            bytes,
        })
    }

    fn delete_asset(&mut self, id: u64) -> Ticket {
        self.ask(Request::DeleteAsset(AssetId(id)))
    }

    fn list_assets(&mut self, kind: String) -> Ticket {
        self.ask(Request::ListAssets(kind))
    }

    fn hold_asset(&mut self, id: u64) -> Ticket {
        self.ask(Request::HoldAsset(AssetId(id)))
    }

    fn release_asset(&mut self, id: u64) -> Ticket {
        self.ask(Request::ReleaseAsset(AssetId(id)))
    }

    fn call(&mut self, service: String, argument: Vec<u8>) -> Ticket {
        self.ask(Request::Call { service, argument })
    }

    fn subscribe(&mut self, event: String) -> Ticket {
        self.ask(Request::Subscribe(event))
    }

    fn emit(&mut self, event: String, payload: Vec<u8>) -> Ticket {
        self.ask(Request::Emit { event, payload })
    }

    fn log(&mut self, level: Level, mut text: String) {
        if self.logs.len() >= self.caps.lines {
            return;
        }
        text.truncate(text.floor_char_boundary(self.caps.line_bytes));
        let level = match level {
            Level::Info => LogLevel::Info,
            Level::Warn => LogLevel::Warn,
            Level::Error => LogLevel::Error,
        };
        self.logs.push((level, text));
    }

    fn redraw(&mut self, name: String) {
        if self.redraws.len() < self.caps.redraws
            && !self.redraws.contains(&name)
        {
            self.redraws.push(name);
        }
    }
}

/// On a running plugin: its store and its exports.
#[derive(Component)]
pub struct Instance {
    store: Store<State>,
    plugin: Plugin,
}

impl Instance {
    /// Instantiates `linked` in a store of its own, held to `caps`.
    pub fn new(
        engine: &Engine,
        linked: &Linked,
        caps: &Caps,
    ) -> Result<Self, Fault> {
        let state = State {
            limiter: Limiter::new(caps.memory),
            caps: *caps,
            logs: Vec::new(),
            redraws: Vec::new(),
            requests: Vec::new(),
            ticket: 0,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limiter);
        store.epoch_deadline_trap();
        arm(&mut store, caps)?;
        let plugin =
            linked.0.instantiate(&mut store).map_err(Fault::of)?;
        Ok(Self { store, plugin })
    }

    /// What the plugin adds.
    pub fn declare(
        &mut self,
        caps: &Caps,
    ) -> (Result<Vec<Contribution>, Fault>, Spent) {
        let (bytes, spent) =
            self.call(caps, |guest, store| guest.call_declare(store));
        (bytes.and_then(|bytes| read(&bytes)), spent)
    }

    /// The widget tree of the panel or settings page `name`, not yet
    /// checked against the budgets.
    pub fn draw(
        &mut self,
        caps: &Caps,
        name: &str,
    ) -> (Result<WidgetTree, Fault>, Spent) {
        let (bytes, spent) = self
            .call(caps, |guest, store| guest.call_draw(store, name));
        (bytes.and_then(|bytes| read(&bytes)), spent)
    }

    pub fn event(
        &mut self,
        caps: &Caps,
        event: &WidgetEvent,
    ) -> (Result<(), Fault>, Spent) {
        let bytes = match encode(event) {
            Ok(bytes) if bytes.len() <= caps.bytes => bytes,
            _ => return (Err(Fault::Bytes), Spent::default()),
        };
        self.call(caps, |guest, store| {
            guest.call_event(store, &bytes)
        })
    }

    /// Runs the plugin's command `name`.
    pub fn run(
        &mut self,
        caps: &Caps,
        name: &str,
    ) -> (Result<(), Fault>, Spent) {
        self.call(caps, |guest, store| guest.call_run(store, name))
    }

    /// The plugin's state, for its next version.
    pub fn save(
        &mut self,
        caps: &Caps,
    ) -> (Result<Vec<u8>, Fault>, Spent) {
        self.call(caps, |guest, store| guest.call_save(store))
    }

    /// Hands the plugin the state its earlier version saved.
    pub fn restore(
        &mut self,
        caps: &Caps,
        state: &[u8],
    ) -> (Result<(), Fault>, Spent) {
        if state.len() > caps.bytes {
            return (Err(Fault::Bytes), Spent::default());
        }
        self.call(caps, |guest, store| {
            guest.call_restore(store, state)
        })
    }

    /// Hands the plugin `messages`, the bytes of a `Vec<Message>`.
    pub fn receive(
        &mut self,
        caps: &Caps,
        messages: &[u8],
    ) -> (Result<(), Fault>, Spent) {
        if messages.len() > caps.bytes {
            return (Err(Fault::Bytes), Spent::default());
        }
        self.call(caps, |guest, store| {
            guest.call_receive(store, messages)
        })
    }

    /// Serves a call of the plugin's service `service` by `caller`:
    /// its result, or why the plugin says it failed.
    pub fn serve(
        &mut self,
        caps: &Caps,
        service: &str,
        caller: &str,
        argument: &[u8],
    ) -> (Result<Result<Vec<u8>, String>, Fault>, Spent) {
        if argument.len() > caps.bytes {
            return (Err(Fault::Bytes), Spent::default());
        }
        self.call(caps, |guest, store| {
            guest.call_serve(store, service, caller, argument)
        })
    }

    /// Lines the plugin logged since this was last asked.
    pub fn take_logs(&mut self) -> Vec<(LogLevel, String)> {
        std::mem::take(&mut self.store.data_mut().logs)
    }

    /// Requests the plugin made since this was last asked, with
    /// their tickets.
    pub fn take_requests(&mut self) -> Vec<(Ticket, Request)> {
        std::mem::take(&mut self.store.data_mut().requests)
    }

    /// Goes on numbering requests from where `earlier`, the plugin's
    /// version before this one, left off.
    pub fn follow(&mut self, earlier: &Instance) {
        self.store.data_mut().ticket = earlier.store.data().ticket;
    }

    /// Panels the plugin asked to be drawn again since this was last
    /// asked.
    pub fn take_redraws(&mut self) -> Vec<String> {
        std::mem::take(&mut self.store.data_mut().redraws)
    }

    fn call<T>(
        &mut self,
        caps: &Caps,
        call: impl FnOnce(
            &Guest,
            &mut Store<State>,
        ) -> wasmtime::Result<T>,
    ) -> (Result<T, Fault>, Spent) {
        if let Err(fault) = arm(&mut self.store, caps) {
            return (Err(fault), Spent::default());
        }
        let started = Instant::now();
        let done =
            call(self.plugin.hamlet_engine_guest(), &mut self.store);
        let spent = Spent {
            time: started.elapsed(),
            fuel: caps
                .fuel
                .saturating_sub(self.store.get_fuel().unwrap_or(0)),
        };
        (done.map_err(Fault::of), spent)
    }
}

/// Sets what the next call into `store` may spend.
fn arm(store: &mut Store<State>, caps: &Caps) -> Result<(), Fault> {
    store.set_fuel(caps.fuel).map_err(Fault::of)?;
    store.set_epoch_deadline(caps.ticks);
    store.set_hostcall_fuel(caps.bytes);
    let state = store.data_mut();
    state.caps = *caps;
    state.limiter.memory = caps.memory;
    Ok(())
}

fn read<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, Fault> {
    decode(bytes).map_err(|why| Fault::Unreadable(why.to_string()))
}

/// A plugin's memories grew past its budget.
#[derive(Debug)]
struct OverMemory;

impl fmt::Display for OverMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "past the plugin's memory")
    }
}

impl core::error::Error for OverMemory {}

/// Holds a store's memories together to one budget. A growth past it
/// fails the call.
struct Limiter {
    memory: usize,
    /// Bytes of all the store's memories.
    used: usize,
    /// The last growth allowed, given back if it failed.
    growing: usize,
}

impl Limiter {
    fn new(memory: usize) -> Self {
        Self {
            memory,
            used: 0,
            growing: 0,
        }
    }
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let growth = desired.saturating_sub(current);
        if self.used.saturating_add(growth) > self.memory {
            return Err(wasmtime::Error::new(OverMemory));
        }
        self.used += growth;
        self.growing = growth;
        Ok(true)
    }

    fn memory_grow_failed(
        &mut self,
        _error: wasmtime::Error,
    ) -> wasmtime::Result<()> {
        self.used -= std::mem::take(&mut self.growing);
        Ok(())
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // The pool caps each table's elements.
        Ok(true)
    }
}

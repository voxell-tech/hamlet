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
    AgentsRequest, AssetId, Contribution, DesktopRequest,
    FetchRequest, Request, Ticket, decode,
};
use hamlet_engine_core::{Budgets, LogLevel};
use wasmtime::component::{Component as Compiled, HasSelf, Linker};
use wasmtime::{Engine, ResourceLimiter, Store, Trap};

use crate::host::WasmTuning;
use crate::nodes::{NodeChange, NodeError, Nodes};
use crate::scene::scene_bindings::exports::hamlet::engine::scene_guest::SceneInput;
use crate::scene::scene_bindings::{SceneView, SceneViewIndices};
use crate::scene::{GPU, SCENE, SceneCaps, SceneState, scene_bindings};
use crate::surfaces::SurfaceChange;
use crate::wasi::{self, Wasi};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../wit/hamlet_engine.wit",
        world: "plugin",
    });
}

pub(crate) mod ui_bindings {
    wasmtime::component::bindgen!({
        path: "../wit/hamlet_engine.wit",
        world: "ui-panels",
        imports: { default: trappable },
        with: { "hamlet:engine/ui.node": crate::nodes::UiNode },
    });
}

use self::bindings::exports::hamlet::engine::guest::Guest;
use self::bindings::hamlet::engine::host::{self, Level};
use self::bindings::{Plugin, PluginPre};
use self::ui_bindings::exports::hamlet::engine::ui_guest::{
    Action, Motions, UiEvent,
};
use self::ui_bindings::{UiPanels, UiPanelsIndices};

/// The interfaces of the host a plugin may import, besides the
/// system's (`wasi:`).
const HOST: &str = "hamlet:engine/host@0.1.0";
const UI: &str = "hamlet:engine/ui@0.1.0";

/// The text wasmtime gives a call that copied more than the
/// hostcall fuel allows. It names the error in its text only.
const TOO_MUCH: &str =
    "fuel allocated for hostcalls has been exhausted";

/// What one call may spend.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub fuel: u64,
    /// Ticks of the epoch, and the time they stand for.
    pub ticks: u64,
    pub time: Duration,
    /// Bytes copied in or out.
    pub bytes: usize,
    /// Lines logged, and the bytes of one.
    pub lines: usize,
    pub line_bytes: usize,
    /// Requests made of the engine's services.
    pub requests: usize,
    /// Bytes of all the plugin's memories.
    pub memory: usize,
    /// Resources of the system interface held at once.
    pub resources: usize,
    /// Nodes of retained panels made, set or ordered.
    pub node_changes: usize,
    /// Nodes of retained panels held at once.
    pub nodes: usize,
    /// How deep a node may sit under its panel's root.
    pub node_depth: usize,
    pub scene: SceneCaps,
}

impl Caps {
    pub fn of(budgets: &Budgets, tuning: &WasmTuning) -> Self {
        Self {
            fuel: budgets.fuel_per_call,
            ticks: tuning.call_ticks(budgets),
            time: Duration::from_secs_f32(budgets.call_time / 1000.0),
            bytes: budgets.bytes_per_call as usize,
            lines: budgets.log_lines_per_second as usize,
            line_bytes: budgets.log_line_bytes,
            requests: budgets.requests_per_call,
            memory: budgets.memory as usize,
            resources: budgets.wasi_resources,
            node_changes: budgets.node_changes_per_call,
            nodes: budgets.nodes,
            node_depth: budgets.node_depth,
            scene: SceneCaps::of(budgets),
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
    /// It went past a budget of the nodes of its panels, or misused
    /// one.
    Node(NodeError),
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
            Self::Node(error) => write!(f, "{error}"),
            Self::Trap(why) => write!(f, "it failed: {why}"),
            Self::Unreadable(why) => {
                write!(f, "it gave what cannot be read: {why}")
            }
        }
    }
}

impl Fault {
    pub(crate) fn of(error: wasmtime::Error) -> Self {
        if error.is::<OverMemory>() {
            return Self::Memory;
        }
        if let Some(&error) = error.downcast_ref::<NodeError>() {
            return Self::Node(error);
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
    /// Nodes of its panels made, set or ordered.
    pub changes: u32,
    /// Calls on nodes of its panels, of every kind.
    pub node_calls: u64,
    /// Changes to its scenes.
    pub scene_changes: u32,
}

/// A plugin's component, compiled and linked, ready to be
/// instantiated, with its views' exports if it has them.
#[derive(Clone)]
pub struct Linked {
    plugin: PluginPre<State>,
    panels: Option<UiPanelsIndices>,
    scenes: Option<SceneViewIndices>,
}

/// Compiles and links the component in `bytes`.
pub fn link(engine: &Engine, bytes: &[u8]) -> Result<Linked, String> {
    let component =
        Compiled::new(engine, bytes).map_err(|e| format!("{e:#}"))?;
    let ty = component.component_type();
    for (name, _) in ty.imports(engine) {
        if name.starts_with("wasi:")
            || [HOST, UI, SCENE, GPU].contains(&name)
        {
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
    let pre = host::add_to_linker::<State, HasSelf<State>>(
        &mut linker,
        |state| state,
    )
    .and_then(|()| {
        ui_bindings::hamlet::engine::ui::add_to_linker::<
            State,
            HasSelf<State>,
        >(&mut linker, |state| state)
    })
    .and_then(|()| {
        scene_bindings::hamlet::engine::scene::add_to_linker::<
            State,
            HasSelf<State>,
        >(&mut linker, |state| state)
    })
    .and_then(|()| {
        scene_bindings::hamlet::engine::gpu::add_to_linker::<
            State,
            HasSelf<State>,
        >(&mut linker, |state| state)
    })
    .and_then(|()| wasi::add_to_linker(&mut linker))
    .and_then(|()| linker.instantiate_pre(&component))
    .map_err(|e| format!("{e:#}"))?;
    let panels = UiPanelsIndices::new(&pre).ok();
    let scenes = SceneViewIndices::new(&pre).ok();
    let plugin = PluginPre::new(pre).map_err(|e| format!("{e:#}"))?;
    Ok(Linked {
        plugin,
        panels,
        scenes,
    })
}

/// What the host keeps for a plugin while it is called.
pub struct State {
    limiter: Limiter,
    pub(crate) caps: Caps,
    logs: Vec<(LogLevel, String)>,
    requests: Vec<(Ticket, Request)>,
    /// The ticket the last request was given.
    ticket: Ticket,
    pub(crate) wasi: Wasi,
    pub(crate) nodes: Nodes,
    pub(crate) surfaces: Vec<SurfaceChange>,
    pub(crate) scene: SceneState,
}

impl State {
    pub(crate) fn new(caps: &Caps) -> Self {
        Self {
            limiter: Limiter::new(caps.memory),
            caps: *caps,
            logs: Vec::new(),
            requests: Vec::new(),
            ticket: 0,
            wasi: Wasi::new(caps),
            nodes: Nodes::default(),
            surfaces: Vec::new(),
            scene: SceneState::new(caps.scene),
        }
    }

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

    /// A request that cannot be read is not taken.
    fn fetch(&mut self, request: Vec<u8>) -> Ticket {
        match decode::<FetchRequest>(&request) {
            Ok(request) => self.ask(Request::Fetch(request)),
            Err(_) => 0,
        }
    }

    /// A request that cannot be read is not taken.
    fn agents(&mut self, request: Vec<u8>) -> Ticket {
        match decode::<AgentsRequest>(&request) {
            Ok(request) => self.ask(Request::Agents(request)),
            Err(_) => 0,
        }
    }

    /// A request that cannot be read is not taken.
    fn desktop(&mut self, request: Vec<u8>) -> Ticket {
        match decode::<DesktopRequest>(&request) {
            Ok(request) => self.ask(Request::Desktop(request)),
            Err(_) => 0,
        }
    }

    fn log(&mut self, level: Level, text: String) {
        let level = match level {
            Level::Info => LogLevel::Info,
            Level::Warn => LogLevel::Warn,
            Level::Error => LogLevel::Error,
        };
        keep(&mut self.logs, &self.caps, level, text);
    }
}

/// Keeps the line `text` in `logs`, within `caps`.
pub(crate) fn keep(
    logs: &mut Vec<(LogLevel, String)>,
    caps: &Caps,
    level: LogLevel,
    mut text: String,
) {
    if logs.len() >= caps.lines {
        return;
    }
    text.truncate(text.floor_char_boundary(caps.line_bytes));
    logs.push((level, text));
}

/// On a running plugin: its store and its exports.
#[derive(Component)]
pub struct Instance {
    store: Store<State>,
    plugin: Plugin,
    panels: Option<UiPanels>,
    scenes: Option<SceneView>,
}

/// What the person did to a node of a retained panel.
#[derive(Clone, PartialEq, Debug)]
pub enum NodeInput {
    Click,
    Hover(bool),
    Press(bool),
    /// A toggle switched, to this.
    Toggle(bool),
    /// A field's text, as the person left it.
    Commit(String),
    /// A dropdown's option picked, by its place.
    Pick(u32),
    /// What was typed in a field of several lines as Enter sent it.
    Submit(String),
    /// A scroll that sticks reached its top.
    Top,
    /// An item of a node's menu chosen, by its place.
    Chose(u32),
    /// The folder chosen in the dialog a button opened.
    Chosen(String),
}

impl Instance {
    /// Instantiates `linked` in a store of its own, held to `caps`.
    pub fn new(
        engine: &Engine,
        linked: &Linked,
        caps: &Caps,
    ) -> Result<Self, Fault> {
        let mut store = Store::new(engine, State::new(caps));
        store.limiter(|state| &mut state.limiter);
        store.epoch_deadline_trap();
        store.arm(caps)?;
        let made = linked
            .plugin
            .instance_pre()
            .instantiate(&mut store)
            .and_then(|instance| {
                let plugin = Plugin::new(&mut store, &instance)?;
                let panels = linked
                    .panels
                    .as_ref()
                    .map(|panels| panels.load(&mut store, &instance))
                    .transpose()?;
                let scenes = linked
                    .scenes
                    .as_ref()
                    .map(|scenes| scenes.load(&mut store, &instance))
                    .transpose()?;
                Ok((plugin, panels, scenes))
            });
        store.take_written();
        let (plugin, panels, scenes) = made.map_err(Fault::of)?;
        Ok(Self {
            store,
            plugin,
            panels,
            scenes,
        })
    }

    /// Whether it builds panels and settings pages: it exports
    /// `ui-guest`.
    pub fn has_views(&self) -> bool {
        self.panels.is_some()
    }

    /// Builds the panel or settings page `name` under a root the host
    /// holds as `root`, its movements taking the seconds `motions`
    /// gives: whether something still moves.
    pub fn mount(
        &mut self,
        caps: &Caps,
        name: &str,
        motions: [f32; 2],
    ) -> (Result<bool, Fault>, Spent, u32) {
        let root = self.store.data_mut().nodes.root();
        let number = root.rep();
        let motions = Motions {
            interact: motions[0],
            expand: motions[1],
        };
        let (moving, spent) = self.call_ui(caps, |guest, store| {
            guest.call_mount(store, name, root, motions)
        });
        (moving, spent, number)
    }

    /// Lets go of the retained panel `name`, whose nodes are gone.
    pub fn unmount(
        &mut self,
        caps: &Caps,
        name: &str,
    ) -> (Result<bool, Fault>, Spent) {
        self.call_ui(caps, |guest, store| {
            guest.call_unmount(store, name).map(|()| false)
        })
    }

    /// Hands over what the person did to nodes of its retained
    /// panels, each by the plugin's key: whether something still
    /// moves. The texts of fields together are cut to the bytes one
    /// call may hand over.
    pub fn input(
        &mut self,
        caps: &Caps,
        inputs: Vec<(u32, NodeInput)>,
    ) -> (Result<bool, Fault>, Spent) {
        let mut left = caps.bytes;
        let events = inputs
            .into_iter()
            .map(|(key, input)| UiEvent {
                key,
                action: match input {
                    NodeInput::Click => Action::Click,
                    NodeInput::Hover(on) => Action::Hover(on),
                    NodeInput::Press(on) => Action::Press(on),
                    NodeInput::Toggle(on) => Action::Toggled(on),
                    NodeInput::Commit(mut text) => {
                        text.truncate(text.floor_char_boundary(left));
                        left -= text.len();
                        Action::Committed(text)
                    }
                    NodeInput::Submit(mut text) => {
                        text.truncate(text.floor_char_boundary(left));
                        left -= text.len();
                        Action::Submitted(text)
                    }
                    NodeInput::Pick(at) => Action::Picked(at),
                    NodeInput::Top => Action::ReachedTop,
                    NodeInput::Chose(at) => Action::Chose(at),
                    NodeInput::Chosen(mut folder) => {
                        folder.truncate(
                            folder.floor_char_boundary(left),
                        );
                        left -= folder.len();
                        Action::Chosen(folder)
                    }
                },
            })
            .collect::<Vec<_>>();
        self.call_ui(caps, |guest, store| {
            guest.call_input(store, &events)
        })
    }

    /// Moves its retained panels on by `micros`: whether something
    /// still moves.
    pub fn frame(
        &mut self,
        caps: &Caps,
        micros: u32,
    ) -> (Result<bool, Fault>, Spent) {
        self.call_ui(caps, |guest, store| {
            guest.call_frame(store, micros)
        })
    }

    /// The changes to its nodes since this was last asked.
    pub fn take_node_changes(&mut self) -> Vec<NodeChange> {
        std::mem::take(&mut self.store.data_mut().nodes.changes)
    }

    /// The changes to what it shows outside its panels since this was
    /// last asked.
    pub fn take_surface_changes(&mut self) -> Vec<SurfaceChange> {
        std::mem::take(&mut self.store.data_mut().surfaces)
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

    fn call<T>(
        &mut self,
        caps: &Caps,
        call: impl FnOnce(
            &Guest,
            &mut Store<State>,
        ) -> wasmtime::Result<T>,
    ) -> (Result<T, Fault>, Spent) {
        let guest = self.plugin.hamlet_engine_guest();
        self.store.spend(caps, |store| call(guest, store))
    }

    /// A call of its retained panels; one that has none answers that
    /// nothing moves.
    fn call_ui(
        &mut self,
        caps: &Caps,
        call: impl FnOnce(
            &self::ui_bindings::exports::hamlet::engine::ui_guest::Guest,
            &mut Store<State>,
        ) -> wasmtime::Result<bool>,
    ) -> (Result<bool, Fault>, Spent) {
        let Some(panels) = &self.panels else {
            return (Ok(false), Spent::default());
        };
        let guest = panels.hamlet_engine_ui_guest();
        self.store.spend(caps, |store| call(guest, store))
    }

    /// The plugin's scenes, as its host holds them.
    pub(crate) fn scene(&mut self) -> &mut SceneState {
        &mut self.store.data_mut().scene
    }

    /// Hands over what the person did in its scenes' views.
    pub fn scene_input(
        &mut self,
        caps: &Caps,
        inputs: &[SceneInput],
    ) -> (Result<(), Fault>, Spent) {
        let Some(scenes) = &self.scenes else {
            return (Ok(()), Spent::default());
        };
        let guest = scenes.hamlet_engine_scene_guest();
        self.store
            .spend(caps, |store| guest.call_input(store, inputs))
    }

    /// Moves its scenes on by `micros`.
    pub fn scene_tick(
        &mut self,
        caps: &Caps,
        micros: u32,
    ) -> (Result<(), Fault>, Spent) {
        let Some(scenes) = &self.scenes else {
            return (Ok(()), Spent::default());
        };
        let guest = scenes.hamlet_engine_scene_guest();
        self.store
            .spend(caps, |store| guest.call_tick(store, micros))
    }
}

/// The WASM host's methods on a plugin's [`Store`].
trait WasmStoreExt {
    /// Makes `call` into the store within `caps`, and what it spent.
    fn spend<T>(
        &mut self,
        caps: &Caps,
        call: impl FnOnce(&mut Self) -> wasmtime::Result<T>,
    ) -> (Result<T, Fault>, Spent);

    /// Sets what the next call into the store may spend.
    fn arm(&mut self, caps: &Caps) -> Result<(), Fault>;

    /// Moves what the plugin wrote to stdout and stderr into its log.
    fn take_written(&mut self);
}

impl WasmStoreExt for Store<State> {
    fn spend<T>(
        &mut self,
        caps: &Caps,
        call: impl FnOnce(&mut Self) -> wasmtime::Result<T>,
    ) -> (Result<T, Fault>, Spent) {
        if let Err(fault) = self.arm(caps) {
            return (Err(fault), Spent::default());
        }
        let started = Instant::now();
        let done = call(self);
        self.take_written();
        let nodes = &self.data().nodes;
        let spent = Spent {
            time: started.elapsed(),
            fuel: caps
                .fuel
                .saturating_sub(self.get_fuel().unwrap_or(0)),
            changes: nodes.spent as u32,
            node_calls: nodes.calls,
            scene_changes: self.data().scene.spent as u32,
        };
        (done.map_err(Fault::of), spent)
    }

    fn arm(&mut self, caps: &Caps) -> Result<(), Fault> {
        self.set_fuel(caps.fuel).map_err(Fault::of)?;
        self.set_epoch_deadline(caps.ticks);
        self.set_hostcall_fuel(caps.bytes);
        let state = self.data_mut();
        state.caps = *caps;
        state.limiter.memory = caps.memory;
        state.wasi.arm(caps);
        state.nodes.spent = 0;
        state.nodes.calls = 0;
        state.scene.spent = 0;
        state.scene.caps = caps.scene;
        Ok(())
    }

    fn take_written(&mut self) {
        let state = self.data_mut();
        state.wasi.take_written(&state.caps, &mut state.logs);
    }
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

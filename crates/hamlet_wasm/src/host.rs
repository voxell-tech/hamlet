//! What a plugin may call in the host: the functions the linker
//! defines, and the state they work on.

mod collections;
mod declare;
mod keeping;
mod read;
mod request;

use hamlet_sdk::agents::Agent;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::papers::Papers;
use wasmtime::component::{Component as Compiled, HasSelf, Linker};

pub use self::collections::{Reach, Shared};
pub use self::declare::{
    Chorded, Declared, Offered, Offers, Placed, Targeted,
};
pub use self::request::Want;
use crate::fault::Fault;
use crate::interface::hamlet::plugin::host::{
    self, Host, Level, RequestResult,
};
use crate::interface::{VERSION, built_for};
use crate::limits::{Limiter, Limits};
use crate::storage::Storage;

/// The name of the imported interface the linker defines.
const HOST: &str = "hamlet:plugin/host@";

/// A line a plugin logged.
#[derive(Clone, PartialEq, Debug)]
pub struct Logged {
    pub level: Level,
    pub message: String,
}

/// The data of one plugin's store.
pub struct State {
    pub(crate) limiter: Limiter,
    pub(crate) logs: Vec<Logged>,
    pub(crate) room: usize,
    pub(crate) bytes: usize,
    pub(crate) declared: Vec<Declared>,
    pub(crate) panel_room: usize,
    pub(crate) offered: Offers,
    pub(crate) command_room: usize,
    pub(crate) menu_room: usize,
    pub(crate) context_room: usize,
    pub(crate) key_room: usize,
    /// The id of the plugin this store belongs to.
    pub(crate) plugin: String,
    pub(crate) granted: Granted,
    /// What the call tried to use, or asked for, that the chief is
    /// yet to decide.
    pub(crate) wants: Vec<Want>,
    /// The agents, when the plugin may read them.
    pub(crate) agents: Vec<Agent>,
    /// The selected agent's papers, when the plugin may read them.
    pub(crate) papers: Papers,
    /// The plugin's storage, once it has a folder.
    pub(crate) storage: Option<Storage>,
    /// The blob the plugin keeps with the host.
    pub(crate) state: Vec<u8>,
    pub(crate) state_room: usize,
    /// The collections the plugin shares, and those of others that
    /// it may reach.
    pub(crate) reach: Reach,
    /// The collections, by the plugin that shares each and its name,
    /// that changed in the call.
    pub(crate) changes: Vec<(String, String)>,
    pub(crate) page_items: usize,
    pub(crate) page_bytes: usize,
}

impl State {
    pub fn new(limits: &Limits) -> Self {
        Self {
            limiter: Limiter::new(limits),
            logs: Vec::new(),
            room: limits.log_lines,
            bytes: limits.log_bytes,
            declared: Vec::new(),
            panel_room: limits.panels,
            offered: Offers::default(),
            command_room: limits.commands,
            menu_room: limits.menu_items,
            context_room: limits.context_items,
            key_room: limits.keys,
            plugin: String::new(),
            granted: Granted::default(),
            wants: Vec::new(),
            agents: Vec::new(),
            papers: Papers::default(),
            storage: None,
            state: Vec::new(),
            state_room: limits.state_bytes,
            reach: Reach::default(),
            changes: Vec::new(),
            page_items: limits.collection_page_items,
            page_bytes: limits.collection_page_bytes,
        }
    }

    /// Keeps a line for the log, if the call has room for it.
    pub(crate) fn write(
        &mut self,
        level: Level,
        mut message: String,
    ) {
        if self.logs.len() < self.room {
            message.truncate(message.floor_char_boundary(self.bytes));
            self.logs.push(Logged { level, message });
        }
    }
}

impl Host for State {
    fn log(&mut self, level: Level, message: String) {
        self.write(level, message);
    }

    fn panel(&mut self, id: String, title: String, icon: String) {
        self.declare(id, title, icon);
    }

    fn command(
        &mut self,
        id: String,
        title: String,
        icon: Option<String>,
    ) {
        self.offer(id, title, icon);
    }

    fn menu_item(
        &mut self,
        menu: String,
        section: u32,
        label: String,
        command: String,
    ) {
        self.place(menu, section, label, command);
    }

    fn key(&mut self, chord: String, command: String) {
        self.bind(chord, command);
    }

    fn context_item(
        &mut self,
        target: String,
        section: u32,
        label: String,
        command: String,
    ) {
        self.contextualise(target, section, label, command);
    }

    fn agents(&mut self) -> Result<Vec<host::Agent>, String> {
        self.read_agents()
    }

    fn papers(&mut self) -> Result<host::AgentPapers, String> {
        self.read_papers()
    }

    fn request(
        &mut self,
        capability: String,
        reason: String,
    ) -> RequestResult {
        self.ask_for(&capability, &reason)
    }

    fn storage_get(&mut self, key: String) -> Option<Vec<u8>> {
        self.stored(&key)
    }

    fn storage_set(
        &mut self,
        key: String,
        value: Vec<u8>,
    ) -> Result<(), String> {
        self.store(&key, &value)
    }

    fn storage_delete(&mut self, key: String) {
        self.unstore(&key);
    }

    fn storage_keys(&mut self, prefix: String) -> Vec<String> {
        self.stored_keys(&prefix)
    }

    fn collection_list(
        &mut self,
        plugin: String,
        name: String,
        after: Option<String>,
        limit: u32,
    ) -> Result<host::CollectionPage, String> {
        self.collection_page(&plugin, &name, after, limit)
    }

    fn collection_get(
        &mut self,
        plugin: String,
        name: String,
        key: String,
    ) -> Result<Option<Vec<u8>>, String> {
        self.collection_value(&plugin, &name, &key)
    }

    fn collection_put(
        &mut self,
        plugin: String,
        name: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<(), String> {
        self.collection_set(&plugin, &name, &key, &value)
    }

    fn collection_delete(
        &mut self,
        plugin: String,
        name: String,
        key: String,
    ) -> Result<(), String> {
        self.collection_remove(&plugin, &name, &key)
    }

    fn state_get(&mut self) -> Vec<u8> {
        self.state.clone()
    }

    fn state_set(&mut self, state: Vec<u8>) -> Result<(), String> {
        self.keep_state(state)
    }
}

/// A linker that defines only the host functions a plugin may use.
/// An import of the standard library of the plugin's language
/// (`wasi:`) is defined as a function that traps; any other import
/// the host does not provide fails the load.
pub fn linker(
    engine: &wasmtime::Engine,
    component: &Compiled,
) -> Result<Linker<State>, Fault> {
    let shape = component.component_type();
    let old = shape
        .imports(engine)
        .map(|(name, _)| name)
        .chain(shape.exports(engine).map(|(name, _)| name))
        .find_map(built_for);
    if let Some(version) = old {
        return Err(Fault::Load(format!(
            "built for interface {version}, this Hamlet speaks \
             {VERSION}"
        )));
    }
    let mut linker = Linker::new(engine);
    host::add_to_linker::<State, HasSelf<State>>(
        &mut linker,
        |state| state,
    )
    .map_err(Fault::load)?;
    let unknown =
        shape.imports(engine).map(|(name, _)| name).find(|name| {
            !name.starts_with("wasi:") && !name.starts_with(HOST)
        });
    if let Some(name) = unknown {
        return Err(Fault::Load(format!(
            "it imports `{name}`, which the host does not provide"
        )));
    }
    linker
        .define_unknown_imports_as_traps(component)
        .map_err(Fault::load)?;
    Ok(linker)
}

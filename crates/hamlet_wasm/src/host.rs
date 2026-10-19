//! What a plugin may call in the host: the functions the linker
//! defines, and the state they work on.

mod collections;
mod database;
mod declare;
mod folder;
mod interfaces;
mod keeping;
mod read;
mod request;

use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::Agent;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::papers::Papers;
use wasmtime::component::{Component as Compiled, HasSelf, Linker};

pub use self::collections::{Reach, Shared};
pub use self::declare::{
    Chorded, Declared, Offered, Offers, Paged, Placed, Tabbed,
    Targeted, Toolled,
};
pub use self::request::Want;
use crate::db::Db;
use crate::fault::Fault;
use crate::files::Files;
use crate::interface::hamlet::plugin::host::{
    self, Host, Level, RequestResult,
};
use crate::interface::hamlet::plugin::render;
use crate::interface::{VERSION, built_for};
use crate::limits::{Limiter, Limits};
use crate::scene::Kept;
use crate::storage::Storage;
use crate::warned::Warned;

/// The names of the imported interfaces the linker defines.
const HOST: &str = "hamlet:plugin/host@";
const RENDER: &str = "hamlet:plugin/render@";

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
    pub(crate) section_room: usize,
    pub(crate) page_room: usize,
    pub(crate) key_room: usize,
    pub(crate) tool_room: usize,
    pub(crate) field_room: usize,
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
    /// The realm, when the plugin may read it.
    pub(crate) survey: Survey,
    pub(crate) world_hamlets: usize,
    pub(crate) world_agents: usize,
    /// The plugin's storage, once it has a folder.
    pub(crate) storage: Option<Storage>,
    /// The plugin's folder of files, once it has a folder.
    pub(crate) files: Option<Files>,
    /// The plugin's database, once it has a folder.
    pub(crate) db: Option<Db>,
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
    /// What the plugin builds in the world.
    pub(crate) render: Kept,
    pub(crate) warned: Warned,
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
            section_room: limits.inspector_sections,
            page_room: limits.settings_pages,
            key_room: limits.keys,
            tool_room: limits.agent_tools,
            field_room: limits.tool_fields,
            plugin: String::new(),
            granted: Granted::default(),
            wants: Vec::new(),
            agents: Vec::new(),
            papers: Papers::default(),
            survey: Survey::default(),
            world_hamlets: limits.world_hamlets,
            world_agents: limits.world_agents,
            storage: None,
            files: None,
            db: None,
            state: Vec::new(),
            state_room: limits.state_bytes,
            reach: Reach::default(),
            changes: Vec::new(),
            page_items: limits.collection_page_items,
            page_bytes: limits.collection_page_bytes,
            render: Kept::new(limits),
            warned: Warned::default(),
        }
    }

    /// Logs that a call of the kind `kind` was refused, saying `why`.
    pub(crate) fn refused(&mut self, kind: &str, why: &str) {
        self.warned.warn(&self.plugin, kind, why);
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

    fn inspector_section(
        &mut self,
        target: String,
        title: String,
        panel: String,
    ) {
        self.tab(target, title, panel);
    }

    fn settings_page(
        &mut self,
        title: String,
        icon: String,
        panel: String,
    ) {
        self.page(title, icon, panel);
    }

    fn agent_tool(
        &mut self,
        name: String,
        description: String,
        fields: Vec<host::ToolField>,
    ) {
        self.give_tool(name, description, fields);
    }

    fn agents(&mut self) -> Result<Vec<host::Agent>, String> {
        self.read_agents()
    }

    fn papers(&mut self) -> Result<host::AgentPapers, String> {
        self.read_papers()
    }

    fn survey(&mut self) -> Result<host::WorldView, String> {
        self.read_world()
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

    fn files_list(&mut self) -> Result<Vec<String>, String> {
        self.list_files()
    }

    fn files_read(
        &mut self,
        path: String,
    ) -> Result<Vec<u8>, String> {
        self.read_file(&path)
    }

    fn files_write(
        &mut self,
        path: String,
        data: Vec<u8>,
    ) -> Result<(), String> {
        self.write_file(&path, &data)
    }

    fn files_delete(&mut self, path: String) -> Result<(), String> {
        self.delete_file(&path)
    }

    fn db_put(
        &mut self,
        table: String,
        id: String,
        bytes: Vec<u8>,
        fields: Vec<host::DbField>,
    ) -> Result<(), String> {
        self.db_set(&table, id, bytes, fields)
    }

    fn db_get(
        &mut self,
        table: String,
        id: String,
    ) -> Result<Option<host::DbRecord>, String> {
        self.db_record(&table, &id)
    }

    fn db_delete(
        &mut self,
        table: String,
        id: String,
    ) -> Result<(), String> {
        self.db_remove(&table, &id)
    }

    fn db_list(
        &mut self,
        table: String,
        after: Option<String>,
        limit: u32,
    ) -> Result<host::DbPage, String> {
        self.db_page(&table, after.as_deref(), limit)
    }

    fn db_find(
        &mut self,
        table: String,
        field: String,
        value: String,
        after: Option<String>,
        limit: u32,
    ) -> Result<host::DbPage, String> {
        self.db_search(
            &table,
            &field,
            &value,
            after.as_deref(),
            limit,
        )
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

    fn implementers(
        &mut self,
        interface: String,
    ) -> Result<Vec<String>, String> {
        self.implementers_of(&interface)
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
    render::add_to_linker::<State, HasSelf<State>>(
        &mut linker,
        |state| state,
    )
    .map_err(Fault::load)?;
    let unknown =
        shape.imports(engine).map(|(name, _)| name).find(|name| {
            !name.starts_with("wasi:")
                && !name.starts_with(HOST)
                && !name.starts_with(RENDER)
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

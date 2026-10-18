//! What a plugin may call in the host: the functions the linker
//! defines, and the state they work on.

mod declare;
mod read;

use hamlet_sdk::agents::Agent;
use wasmtime::component::{Component as Compiled, HasSelf, Linker};

pub use self::declare::Declared;
use crate::fault::Fault;
use crate::interface::hamlet::plugin::host::{self, Host, Level};
use crate::limits::{Limiter, Limits};

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
    /// The id of the plugin this store belongs to.
    pub(crate) plugin: String,
    pub(crate) granted: Vec<String>,
    /// The agents, when the plugin may read them.
    pub(crate) agents: Vec<Agent>,
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
            plugin: String::new(),
            granted: Vec::new(),
            agents: Vec::new(),
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

    fn agents(&mut self) -> Result<Vec<host::Agent>, String> {
        self.read_agents()
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
    let mut linker = Linker::new(engine);
    host::add_to_linker::<State, HasSelf<State>>(
        &mut linker,
        |state| state,
    )
    .map_err(Fault::load)?;
    let shape = component.component_type();
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

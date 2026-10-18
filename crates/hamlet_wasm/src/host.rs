//! What a plugin may call in the host: the functions the linker
//! defines, and the state they work on.

use wasmtime::component::{HasSelf, Linker};

use crate::interface::hamlet::plugin::host::{Host, Level};
use crate::limits::{Limiter, Limits};

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
}

impl State {
    pub fn new(limits: &Limits) -> Self {
        Self {
            limiter: Limiter::new(limits),
            logs: Vec::new(),
            room: limits.log_lines,
            bytes: limits.log_bytes,
        }
    }
}

impl Host for State {
    fn log(&mut self, level: Level, mut message: String) {
        if self.logs.len() < self.room {
            message.truncate(message.floor_char_boundary(self.bytes));
            self.logs.push(Logged { level, message });
        }
    }
}

/// A linker that defines only the host functions a plugin may use.
/// A component that imports anything else does not instantiate.
pub fn linker(
    engine: &wasmtime::Engine,
) -> wasmtime::Result<Linker<State>> {
    let mut linker = Linker::new(engine);
    crate::interface::hamlet::plugin::host::add_to_linker::<
        State,
        HasSelf<State>,
    >(&mut linker, |state| state)?;
    Ok(linker)
}

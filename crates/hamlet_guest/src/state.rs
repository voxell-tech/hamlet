//! The state the host holds for the plugin, as a value of a type of
//! the plugin's own. The host keeps it across a reload and a switch
//! off, and drops it when it restarts, so a plugin keeps what it must
//! not lose in its storage. It is written with `postcard`.

use std::marker::PhantomData;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bindings::hamlet::plugin::host;

/// The state of type `T`. It is never built: `State::<Model>::load()`
/// reads it and `State::save(&model)` writes it.
pub struct State<T>(PhantomData<fn() -> T>);

impl<T: Serialize + DeserializeOwned + Default> State<T> {
    /// The state as it was saved, or the default where nothing was
    /// saved, or what was saved cannot be read as a `T`, as after a
    /// new version changed the type.
    pub fn load() -> T {
        postcard::from_bytes(&host::state_get()).unwrap_or_default()
    }

    /// Replaces the state. An error is the host's cap on its size.
    pub fn save(value: &T) -> Result<(), String> {
        let bytes = postcard::to_allocvec(value)
            .map_err(|e| e.to_string())?;
        host::state_set(&bytes)
    }
}

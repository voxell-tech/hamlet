//! Opening a library and getting its entry, once the gate lets it
//! through.

use std::fmt;
use std::path::Path;

use bevy::app::App;
use libloading::Library;

use crate::abi::{
    ABI, ABI_SYMBOL, Abi, ENTRY_SYMBOL, Info, LAYOUT, LAYOUT_SYMBOL,
    VERSION_SYMBOL,
};
use crate::gate::{self, Mismatch};

/// The function a library exports to be called with the app. It
/// gives false when the entry panicked.
pub type EntryFn = fn(&mut App) -> bool;

/// Why a library was not opened.
#[derive(Debug)]
pub enum Refusal {
    /// The system could not open the file.
    Open(String),
    /// The library lacks a symbol of the contract.
    Missing(&'static str),
    /// The library is of another contract version.
    Contract(u32),
    /// The library's `Abi` is laid out otherwise, or it does not say
    /// how.
    Layout(Option<u64>),
    Mismatch(Mismatch),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Open(why) => {
                write!(f, "cannot be opened: {why}")
            }
            Refusal::Missing(symbol) => write!(
                f,
                "is not a Hamlet native plugin, it has no `{symbol}`"
            ),
            Refusal::Contract(found) => write!(
                f,
                "built for contract version {found}, this Hamlet \
                 speaks {ABI}"
            ),
            Refusal::Layout(None) => write!(
                f,
                "built for an older contract 1, it has no \
                 `{LAYOUT_SYMBOL}`; build it again for this Hamlet"
            ),
            Refusal::Layout(Some(found)) => write!(
                f,
                "built with another layout of the contract, \
                 `{LAYOUT_SYMBOL}` gives {} bytes, layout {:08x}; \
                 this Hamlet has {} bytes, layout {:08x}",
                found >> 32,
                *found as u32,
                LAYOUT >> 32,
                LAYOUT as u32
            ),
            Refusal::Mismatch(mismatch) => write!(f, "{mismatch}"),
        }
    }
}

/// Opens the library at `path`, checks it against this build and
/// gives its entry. The library is never closed: its code stays
/// where it is for as long as the process lives.
///
/// Opening runs whatever the library runs when it is loaded, before
/// any check can be made. That is why only a library the chief let
/// in is opened.
pub fn open(path: &Path) -> Result<EntryFn, Refusal> {
    // SAFETY: the chief let this library run as Hamlet.
    let library = unsafe { Library::new(path) }
        .map_err(|e| Refusal::Open(e.to_string()))?;
    // SAFETY: the symbols are looked up by the names of the contract,
    // and the contract's version and the layout of `Abi` are read
    // before the `Abi` is.
    let entry = unsafe {
        let version = library
            .get::<extern "C" fn() -> u32>(VERSION_SYMBOL.as_bytes())
            .map_err(|_| Refusal::Missing(VERSION_SYMBOL))?;
        let found = version();
        if found != ABI {
            return Err(Refusal::Contract(found));
        }
        let layout = library
            .get::<extern "C" fn() -> u64>(LAYOUT_SYMBOL.as_bytes())
            .map_err(|_| Refusal::Layout(None))?;
        let found = layout();
        if found != LAYOUT {
            return Err(Refusal::Layout(Some(found)));
        }
        let abi = library
            .get::<extern "C" fn() -> Abi>(ABI_SYMBOL.as_bytes())
            .map_err(|_| Refusal::Missing(ABI_SYMBOL))?;
        let found = abi().read();
        gate::check(&Info::ours(), &found)
            .map_err(Refusal::Mismatch)?;
        *library
            .get::<EntryFn>(ENTRY_SYMBOL.as_bytes())
            .map_err(|_| Refusal::Missing(ENTRY_SYMBOL))?
    };
    std::mem::forget(library);
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_a_library_is_refused() {
        let file = std::env::temp_dir().join(format!(
            "hamlet_not_a_library_{}.{}",
            std::process::id(),
            std::env::consts::DLL_EXTENSION
        ));
        std::fs::write(&file, b"not a library").unwrap();
        let refusal = open(&file).err().unwrap();
        std::fs::remove_file(&file).unwrap();
        assert!(matches!(refusal, Refusal::Open(_)), "{refusal}");
    }
}

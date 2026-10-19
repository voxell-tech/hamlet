//! What a native plugin's library says about itself, and how it is
//! made to say it.

use std::any::TypeId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::app::App;
use bevy::ecs::world::World;
use hamlet_sdk::plugin::{Declared, Entry};

/// The version of the contract between the host and a library: the
/// symbols below and the shape of [`Abi`]. It moves when either does.
pub const ABI: u32 = 2;

/// The name of the symbol that gives [`ABI`] and nothing else, so
/// that a library of another contract is turned down before its
/// [`Abi`] is read.
pub const VERSION_SYMBOL: &str = "hamlet_native_abi_version";

/// The name of the symbol that gives the [`Abi`].
pub const ABI_SYMBOL: &str = "hamlet_native_abi";

/// The name of the symbol that is called with the app.
pub const ENTRY_SYMBOL: &str = "hamlet_native_entry";

const FINGERPRINT: u64 = parse(env!("HAMLET_NATIVE_FINGERPRINT"));

const SOURCES: u64 = parse(env!("HAMLET_NATIVE_SOURCES"));

const fn parse(digits: &str) -> u64 {
    let bytes = digits.as_bytes();
    let mut value = 0u64;
    let mut at = 0;
    while at < bytes.len() {
        value = value * 10 + (bytes[at] - b'0') as u64;
        at += 1;
    }
    value
}

/// A string a library owns, as the host reads it.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Text {
    ptr: *const u8,
    len: usize,
}

impl Text {
    const fn of(text: &'static str) -> Self {
        Self {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }

    /// # Safety
    ///
    /// The text is one a library of the same [`ABI`] gave, and the
    /// library is still loaded.
    unsafe fn read(&self) -> String {
        let bytes =
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// What a library says about how it was built. It is what the
/// `hamlet_native_abi` symbol gives.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Abi {
    pub abi: u32,
    hamlet: Text,
    rustc: Text,
    profile: Text,
    fingerprint: u64,
    sources: u64,
    types: u64,
}

/// What a build says about itself, as plain values to compare.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Info {
    pub abi: u32,
    /// The version of Hamlet the library was built against.
    pub hamlet: String,
    /// The full version of the compiler.
    pub rustc: String,
    /// `debug` or `release`.
    pub profile: String,
    /// The hash of the lock entries of the engine crates.
    pub fingerprint: u64,
    /// The hash of the source files of `hamlet_sdk` and
    /// `hamlet_native`.
    pub sources: u64,
    /// The hash of the identities of the types that cross over.
    pub types: u64,
}

impl Abi {
    /// What this build says about itself.
    pub fn ours() -> Self {
        Self {
            abi: ABI,
            hamlet: Text::of(env!("CARGO_PKG_VERSION")),
            rustc: Text::of(env!("HAMLET_NATIVE_RUSTC")),
            profile: Text::of(env!("HAMLET_NATIVE_PROFILE")),
            fingerprint: FINGERPRINT,
            sources: SOURCES,
            types: types(),
        }
    }

    /// # Safety
    ///
    /// `self` came from a library of the same [`ABI`] that is still
    /// loaded.
    pub unsafe fn read(&self) -> Info {
        unsafe {
            Info {
                abi: self.abi,
                hamlet: self.hamlet.read(),
                rustc: self.rustc.read(),
                profile: self.profile.read(),
                fingerprint: self.fingerprint,
                sources: self.sources,
                types: self.types,
            }
        }
    }
}

impl Info {
    /// What this build says about itself.
    pub fn ours() -> Self {
        // SAFETY: it is this build's own, in this build's memory.
        unsafe { Abi::ours().read() }
    }
}

/// The hash of the identities of the types the host and a library
/// share. A type has another identity in a build made with other
/// sources, features or settings, and then it is not the same type to
/// the other side, whatever it looks like.
fn types() -> u64 {
    let mut hasher = DefaultHasher::new();
    TypeId::of::<App>().hash(&mut hasher);
    TypeId::of::<World>().hash(&mut hasher);
    TypeId::of::<Entry>().hash(&mut hasher);
    TypeId::of::<Declared>().hash(&mut hasher);
    hasher.finish()
}

/// Declares what the library adds to the window, through the SDK's
/// [`Entry`], as a built-in plugin does. What it declares goes when
/// the plugin is switched off. Call it from the entry.
pub fn declare(app: &mut App, entry: fn(&mut Entry)) {
    app.insert_resource(Declared(entry));
}

/// Exports the three symbols the host looks for. `$entry` is a
/// `fn(&mut App)` that adds what the plugin brings: systems,
/// observers, resources, and a call to [`declare`].
///
/// ```text
/// hamlet_native::export!(entry);
///
/// fn entry(app: &mut hamlet_native::App) { /* ... */ }
/// ```
#[macro_export]
macro_rules! export {
    ($entry:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn hamlet_native_abi_version() -> u32 {
            $crate::ABI
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn hamlet_native_abi() -> $crate::Abi {
            $crate::Abi::ours()
        }

        #[unsafe(no_mangle)]
        pub fn hamlet_native_entry(app: &mut $crate::App) {
            $entry(app)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_reads_back_what_it_says() {
        let info = Info::ours();
        assert_eq!(info.abi, ABI);
        assert_eq!(info.hamlet, env!("CARGO_PKG_VERSION"));
        // The compiler pinned in rust-toolchain.toml.
        let pinned = include_str!("../../../rust-toolchain.toml")
            .lines()
            .find_map(|line| line.strip_prefix("channel = "))
            .unwrap()
            .trim_matches('"');
        assert!(
            info.rustc.contains(&format!("release: {pinned};")),
            "{}",
            info.rustc
        );
        assert!(!info.profile.is_empty());
        assert_ne!(info.types, 0);
    }
}

//! Finds what a native plugin and the host must agree on before the
//! plugin's Rust is called: the compiler, the profile, and a hash of
//! the lock entries of the engine crates.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The packages whose exact version, source and dependencies decide
/// the layout of what is shared.
const WATCHED: [&str; 6] = [
    "bevy",
    "bevy_internal",
    "bevy_app",
    "bevy_ecs",
    "bevy_fynix",
    "hamlet_sdk",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=HAMLET_LOCK");
    println!("cargo:rerun-if-env-changed=RUSTC");
    let fingerprint = match find_lock() {
        Some(lock) => {
            println!("cargo:rerun-if-changed={}", lock.display());
            let text =
                std::fs::read_to_string(&lock).unwrap_or_default();
            fingerprint(&text)
        }
        None => 0,
    };
    println!(
        "cargo:rustc-env=HAMLET_NATIVE_FINGERPRINT={fingerprint}"
    );
    println!("cargo:rustc-env=HAMLET_NATIVE_RUSTC={}", rustc());
    let profile = std::env::var("PROFILE").unwrap_or_default();
    println!("cargo:rustc-env=HAMLET_NATIVE_PROFILE={profile}");
}

/// The `Cargo.lock` the build uses: the one named by `HAMLET_LOCK`,
/// or the nearest above the target folder, or above this crate.
fn find_lock() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HAMLET_LOCK") {
        return Some(PathBuf::from(path));
    }
    ["OUT_DIR", "CARGO_MANIFEST_DIR"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find_map(|start| {
            Path::new(&start)
                .ancestors()
                .map(|folder| folder.join("Cargo.lock"))
                .find(|lock| lock.is_file())
        })
}

/// The hash of the entries of the watched packages, in the order of
/// the lock.
fn fingerprint(lock: &str) -> u64 {
    let mut hash = FNV_OFFSET;
    for entry in lock.split("[[package]]").skip(1) {
        let named = entry.lines().find_map(|line| {
            line.strip_prefix("name = \"")?.strip_suffix('"')
        });
        if named.is_some_and(|name| WATCHED.contains(&name)) {
            hash = fnv(hash, entry.trim().as_bytes());
        }
    }
    hash
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The full version of the compiler, on one line.
fn rustc() -> String {
    let rustc =
        std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    Command::new(rustc)
        .arg("-vV")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.lines().collect::<Vec<_>>().join("; "))
        .unwrap_or_default()
}

//! Finds what a native plugin and the host must agree on before the
//! plugin's Rust is called: the compiler, the profile, a hash of the
//! lock entries of the engine crates and a hash of the manifests and
//! sources of this crate, the SDK and the path crates under it.

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "src/sources.rs"]
mod sources;

use sources::{FNV_OFFSET, fnv};

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
    let Some(lock) = find_lock() else {
        panic!(
            "no Cargo.lock found: set HAMLET_LOCK to the one in use"
        );
    };
    println!("cargo:rerun-if-changed={}", lock.display());
    let text = std::fs::read_to_string(&lock).unwrap_or_else(|e| {
        panic!("{} cannot be read: {e}", lock.display())
    });
    let mut fingerprint = fingerprint(&text);
    if fingerprint == FNV_OFFSET {
        // None of the watched packages is in the lock: the whole
        // lock stands for them.
        fingerprint = fnv(FNV_OFFSET, text.as_bytes());
    }
    assert!(
        fingerprint != 0 && !text.trim().is_empty(),
        "{} is empty, so it cannot fingerprint the engine",
        lock.display()
    );
    println!(
        "cargo:rustc-env=HAMLET_NATIVE_FINGERPRINT={fingerprint}"
    );
    println!("cargo:rustc-env=HAMLET_NATIVE_RUSTC={}", rustc());
    let profile = std::env::var("PROFILE").unwrap_or_default();
    println!("cargo:rustc-env=HAMLET_NATIVE_PROFILE={profile}");

    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fynix = here.join("../../vendor/fynix/crates");
    // In this order, each walked in the order of its paths.
    let crates = [
        here.clone(),
        here.join("../hamlet_sdk"),
        here.join("../hamlet_base"),
        here.join("../hamlet_home"),
        here.join("../hamlet_assets"),
        fynix.join("bevy_fynix"),
        fynix.join("fynix"),
        fynix.join("fynix_macros"),
    ];
    for root in &crates {
        for part in ["Cargo.toml", "src"] {
            println!(
                "cargo:rerun-if-changed={}",
                root.join(part).display()
            );
        }
    }
    let crates = crates.each_ref().map(PathBuf::as_path);
    let sources = sources::hash(&crates).unwrap_or_else(|e| {
        panic!("the sources the contract hashes cannot be read: {e}")
    });
    println!("cargo:rustc-env=HAMLET_NATIVE_SOURCES={sources}");
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

//! The policies of an agent's tools (C18, C20).

use std::path::{Path, PathBuf};

use hamlet_os::fs::path::text;

use super::policy::{Policy, host_env};

/// An agent's policy while its hamlet works on no folder: its house,
/// which is its home, and the shared skills to read (C18).
pub fn closed_policy(house: &Path, shared: &[PathBuf]) -> Policy {
    let mut env = super::base_env();
    env.insert("HOME".into(), text(house));
    Policy {
        ro: shared.to_vec(),
        rw: vec![house.to_owned()],
        env,
        ..Policy::default()
    }
}

/// An agent's policy while its hamlet works on a folder: the machine
/// to read but Hamlet's home, the folder and its house to write (C18,
/// C20).
pub fn open_policy(
    folder: &Path,
    house: &Path,
    home: &Path,
    shared: &[PathBuf],
) -> Policy {
    Policy {
        ro: shared.to_vec(),
        rw: vec![folder.to_owned(), house.to_owned()],
        open: true,
        hidden: vec![home.to_owned()],
        // The chief's own, so the project's tools are found.
        env: host_env(),
        ..Policy::default()
    }
}

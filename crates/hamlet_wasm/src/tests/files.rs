use hamlet_sdk::capability::{
    Asked, Asks, Capability, Choices, Granted, Scope,
};

use crate::files::{Caps, FILES, Files};
use crate::host::State;
use crate::interface::hamlet::plugin::host::Host;
use crate::limits::Limits;

fn kept(name: &str, granted: Granted) -> (State, std::path::PathBuf) {
    let limits = Limits::default();
    let root = std::env::temp_dir().join(format!(
        "hamlet_wasm_files_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mut state = State::new(&limits);
    state.granted = granted;
    state.files =
        Some(Files::new(root.join(FILES), Caps::of(&limits)));
    (state, root)
}

#[test]
fn files_need_a_grant_and_ask_the_chief_where_it_is_yet_to_decide() {
    let asks = Asks(
        [Capability::FilesRead, Capability::FilesWrite]
            .into_iter()
            .map(|capability| Asked {
                capability,
                reason: "a test".into(),
                scope: Scope::default(),
            })
            .collect(),
    );
    let granted = Granted::of(&asks, &Choices::default());
    let (mut state, root) = kept("gated", granted);

    assert_eq!(
        state.files_write("a".into(), vec![1]),
        Err("files.write not granted".into())
    );
    assert_eq!(
        state.files_read("a".into()),
        Err("files.read not granted".into())
    );
    assert_eq!(state.wants.len(), 2);
    assert!(!root.exists());

    let (mut none, root) = kept("unnamed", Granted::default());
    assert!(none.files_list().is_err());
    assert!(none.wants.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_plugin_with_the_grants_keeps_files_and_cannot_leave_its_folder()
{
    let granted = Granted::allowing(&[
        Capability::FilesRead,
        Capability::FilesWrite,
    ]);
    let (mut state, root) = kept("granted", granted);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("outside"), b"x").unwrap();

    state.files_write("a.txt".into(), b"one".to_vec()).unwrap();
    assert_eq!(state.files_read("a.txt".into()).unwrap(), b"one");
    assert_eq!(state.files_list().unwrap(), ["a.txt"]);
    for path in ["../outside", "/outside", "a/../../outside"] {
        assert!(state.files_read(path.into()).is_err(), "{path}");
        assert!(
            state.files_write(path.into(), vec![0]).is_err(),
            "{path}"
        );
    }
    assert_eq!(std::fs::read(root.join("outside")).unwrap(), b"x");
    state.files_delete("a.txt".into()).unwrap();
    assert!(state.files_list().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

use hamlet_sdk::capability::{
    Asked, Asks, Capability, Choices, Granted, Scope,
};

use crate::db::{Caps, DB, Db};
use crate::host::State;
use crate::interface::hamlet::plugin::host::{DbField, Host};
use crate::limits::Limits;

fn kept(
    name: &str,
    plugin: &str,
    granted: Granted,
) -> (State, std::path::PathBuf) {
    let limits = Limits::default();
    let root = std::env::temp_dir().join(format!(
        "hamlet_wasm_db_{name}_{}",
        std::process::id()
    ));
    let mut state = State::new(&limits);
    state.granted = granted;
    state.db =
        Some(Db::new(root.join(plugin).join(DB), Caps::of(&limits)));
    (state, root)
}

fn field(name: &str, value: &str) -> DbField {
    DbField {
        name: name.into(),
        value: value.into(),
    }
}

#[test]
fn the_database_needs_a_grant_and_asks_the_chief_where_it_is_yet_to_decide()
 {
    let asks = Asks(vec![Asked {
        capability: Capability::Db,
        reason: "a test".into(),
        scope: Scope::default(),
    }]);
    let granted = Granted::of(&asks, &Choices::default());
    let (mut state, root) = kept("gated", "a", granted);
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(
        state.db_put("t".into(), "x".into(), vec![1], vec![]),
        Err("db not granted".into())
    );
    assert_eq!(
        state.db_list("t".into(), None, 9),
        Err("db not granted".into())
    );
    assert_eq!(state.wants.len(), 1);
    assert!(!root.exists());

    let (mut none, root) = kept("unnamed", "a", Granted::default());
    assert!(none.db_get("t".into(), "x".into()).is_err());
    assert!(none.wants.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_plugin_with_the_grant_finds_its_records_and_no_other_plugin_does()
 {
    let granted = Granted::allowing(&[Capability::Db]);
    let (mut mine, root) = kept("granted", "mine", granted.clone());
    let _ = std::fs::remove_dir_all(&root);
    let (mut other, _) = kept("granted", "other", granted);

    mine.db_put(
        "items".into(),
        "x".into(),
        vec![7],
        vec![field("kind", "tea")],
    )
    .unwrap();
    let found = mine
        .db_find("items".into(), "kind".into(), "tea".into(), None, 9)
        .unwrap();
    assert_eq!(found.records.len(), 1);
    assert_eq!(found.records[0].bytes, [7]);

    assert_eq!(other.db_get("items".into(), "x".into()), Ok(None));
    assert!(
        other
            .db_list("items".into(), None, 9)
            .unwrap()
            .records
            .is_empty()
    );
    assert!(
        other.db_get("../mine/items".into(), "x".into()).is_err()
    );

    mine.db_delete("items".into(), "x".into()).unwrap();
    assert_eq!(mine.db_get("items".into(), "x".into()), Ok(None));
    let _ = std::fs::remove_dir_all(&root);
}

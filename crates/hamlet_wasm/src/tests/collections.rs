use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::plugin::Imports;

use super::interop::said;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::host::{Reach, Shared, State};
use crate::interface::hamlet::plugin::host::Host;
use crate::limits::Limits;
use crate::storage::{Caps, Storage};

/// The store of a plugin that reads the collection `notes/notes`,
/// which is the keys of `notes` that start with `note.`.
fn reader(
    scratch: &Scratch,
    granted: Granted,
    writable: bool,
) -> (State, Storage) {
    let limits = Limits::default();
    let storage = Storage::new(
        scratch.plugins().join("notes/data"),
        Caps::of(&limits),
    );
    for (key, value) in
        [("note.1", "one"), ("note.2", "two"), ("index", "x")]
    {
        storage.set(key, value.as_bytes()).unwrap();
    }
    let mut state = State::new(&limits);
    state.plugin = "reader".into();
    state.granted = granted;
    state.reach = Reach {
        imports: Imports {
            collections: vec!["notes/notes".into()],
            ..Imports::default()
        },
        own: Vec::new(),
        shared: vec![Shared {
            plugin: "notes".into(),
            name: "notes".into(),
            prefix: "note.".into(),
            writable,
            storage: storage.clone(),
        }],
        implementers: Vec::new(),
    };
    (state, storage)
}

fn allowing(capability: Capability) -> Granted {
    Granted::default().allowing_targets(capability, &["notes/notes"])
}

fn list(
    state: &mut State,
    after: Option<&str>,
    limit: u32,
) -> String {
    match state.collection_list(
        "notes".into(),
        "notes".into(),
        after.map(str::to_string),
        limit,
    ) {
        Ok(page) => {
            let keys = page
                .items
                .iter()
                .map(|item| item.key.as_str())
                .collect::<Vec<_>>();
            format!("{keys:?} next {:?}", page.next)
        }
        Err(why) => why,
    }
}

#[test]
fn a_collection_is_read_by_page_through_its_grant_and_not_without_it()
{
    let scratch = Scratch::new("collections_read");
    let (mut state, _) = reader(
        &scratch,
        allowing(Capability::CollectionsRead),
        false,
    );

    assert_eq!(list(&mut state, None, 10), r#"["1", "2"] next None"#);
    assert_eq!(list(&mut state, None, 1), r#"["1"] next Some("1")"#);
    assert_eq!(list(&mut state, Some("1"), 1), r#"["2"] next None"#);
    let one = state.collection_get(
        "notes".into(),
        "notes".into(),
        "1".into(),
    );
    assert_eq!(one, Ok(Some(b"one".to_vec())));
    let other = state.collection_get(
        "notes".into(),
        "notes".into(),
        "9".into(),
    );
    assert_eq!(other, Ok(None));

    let (mut state, _) = reader(
        &scratch,
        Granted::allowing(&[Capability::CollectionsRead]),
        false,
    );
    assert_eq!(
        list(&mut state, None, 10),
        "collections.read not granted"
    );
    assert_eq!(state.wants.len(), 1);
    assert_eq!(state.wants[0].target.as_deref(), Some("notes/notes"));

    let (mut state, _) = reader(
        &scratch,
        allowing(Capability::CollectionsRead),
        false,
    );
    state.reach.imports = Imports::default();
    assert!(
        list(&mut state, None, 10).contains("not in the manifest")
    );
}

#[test]
fn a_collection_is_written_only_when_it_says_it_is_writable() {
    let scratch = Scratch::new("collections_write");
    let put = |state: &mut State| {
        state.collection_put(
            "notes".into(),
            "notes".into(),
            "3".into(),
            b"three".to_vec(),
        )
    };

    let (mut state, storage) = reader(
        &scratch,
        allowing(Capability::CollectionsWrite),
        false,
    );
    assert_eq!(
        put(&mut state),
        Err("`notes/notes` is read-only".into())
    );
    assert_eq!(storage.get("note.3"), None);

    let (mut state, storage) = reader(
        &scratch,
        allowing(Capability::CollectionsWrite),
        true,
    );
    assert_eq!(put(&mut state), Ok(()));
    assert_eq!(storage.get("note.3"), Some(b"three".to_vec()));
    assert_eq!(state.changes, [("notes".into(), "notes".into())]);

    let (mut state, _) =
        reader(&scratch, allowing(Capability::CollectionsRead), true);
    assert!(put(&mut state).is_err());
}

#[test]
fn a_change_in_a_collection_reaches_the_plugins_that_import_the_event()
 {
    let scratch = Scratch::new("collections_event");
    let mut app = home::app(
        &scratch,
        &[
            // It stores "note" in its first step, in the collection
            // `notes` of the keys that start with "not".
            Plugin::new(
                "notes",
                fixture::component(&fixture::keeping()),
            )
            .saying(
                "[[exports.collections]]\nname = \"notes\"\nprefix \
                 = \"not\"\n",
            ),
            Plugin::new(
                "watcher",
                fixture::component(&fixture::listening()),
            )
            .saying(
                "[imports]\nevents = \
                 [\"notes/collection-changed\"]\n",
            )
            .allowed_for(&[("plugin.use", "notes")]),
        ],
    );

    let lines = said(&mut app, 6);

    assert!(
        lines.contains(&("watcher".into(), "notes".into())),
        "{lines:?}"
    );
}

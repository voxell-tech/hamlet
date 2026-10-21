use std::fs;

use hamlet_engine_api::{
    Ask, AssetType, Capability, Failure, Manifest, PluginId,
    PluginLimits, Reply, Request, TypeName,
};
use hamlet_engine_core::tuning::Tuning;
use hamlet_engine_core::{DefaultLimits, Home};
use hamlet_engine_grants::{Answer, Grants, Record};
use hamlet_engine_plugins::{PluginInfo, PluginKind};

use crate::{
    AssetStore, FOLDER, INDEX, Outcome, Rules, TypeRegistry,
};

fn home(name: &str) -> Home {
    let root = std::env::temp_dir().join(format!(
        "hamlet_engine_test_assets_{name}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    Home::at(root)
}

/// A plugin `id` that makes `makes` and uses `uses`, each at version
/// 1, asking to read what it uses.
fn plugin(
    id: &str,
    makes: &[&str],
    uses: &[(&str, u32)],
) -> PluginInfo {
    let kind = |name: &str, version| AssetType {
        name: TypeName::try_from(name).unwrap(),
        version,
    };
    let mut manifest =
        Manifest::new(PluginId::try_from(id).unwrap(), id, "0.1.0");
    manifest.assets.makes =
        makes.iter().map(|name| kind(name, 1)).collect();
    for (name, version) in uses {
        manifest.assets.uses.push(kind(name, *version));
        manifest.asks.push(Ask {
            capability: Capability::AssetRead,
            target: (*name).into(),
            reason: "To show it".into(),
        });
    }
    assert_eq!(manifest.check(), Ok(()));
    PluginInfo::of(manifest, PluginKind::Installed)
}

fn done(outcome: Option<Outcome>) -> Result<Reply, Failure> {
    match outcome {
        Some(Outcome::Done(done)) => *done,
        other => panic!("not done: {other:?}"),
    }
}

#[test]
fn the_store_keeps_to_types_grants_and_caps() {
    let home = home("rules");
    let notes = plugin("test.notes", &["notes.note"], &[]);
    let board = plugin("test.board", &[], &[("notes.note", 1)]);
    let old = plugin("test.old", &[], &[("notes.note", 2)]);
    let (registry, _) =
        TypeRegistry::of([&notes, &board, &old].into_iter());
    let mut grants = Grants::open(&home);
    let budgets = PluginLimits {
        asset_bytes: 8,
        assets: 1,
        ..DefaultLimits::designed().0
    };
    let mut store = AssetStore::open(&home);
    let create = |bytes: &[u8]| Request::CreateAsset {
        kind: "notes.note".into(),
        name: "First".into(),
        bytes: bytes.to_vec(),
    };

    let answer = |store: &mut AssetStore,
                  grants: &Grants,
                  plugin: &PluginInfo,
                  request: &Request| {
        let rules = Rules {
            registry: &registry,
            grants,
            budgets: &budgets,
        };
        rules.answer(store, plugin, request, false)
    };
    // Past the size, then made, then past the count.
    assert!(matches!(
        done(answer(&mut store, &grants, &notes, &create(&[0; 9]))),
        Err(Failure::OutOfBudget(_))
    ));
    let Ok(Reply::Created(id)) =
        done(answer(&mut store, &grants, &notes, &create(b"hello")))
    else {
        panic!("the note was not made");
    };
    assert!(matches!(
        done(answer(&mut store, &grants, &notes, &create(b"again"))),
        Err(Failure::OutOfBudget(_))
    ));
    // Only its maker makes a type, and changes its own.
    assert!(matches!(
        done(answer(&mut store, &grants, &board, &create(b"mine"))),
        Err(Failure::Refused(_))
    ));
    let update = Request::UpdateAsset {
        id,
        bytes: b"changed".to_vec(),
    };
    assert!(matches!(
        done(answer(&mut store, &grants, &board, &update)),
        Err(Failure::Refused(_))
    ));

    // Another's type is read at the version made, once allowed.
    let read = Request::ReadAsset(id);
    assert!(matches!(
        done(answer(&mut store, &grants, &old, &read)),
        Err(Failure::Mismatch(_))
    ));
    assert!(matches!(
        done(answer(&mut store, &grants, &board, &read)),
        Err(Failure::Refused(_))
    ));
    let mut record = Record {
        allowed: true,
        version: "0.1.0".into(),
        bundled: false,
        answers: Vec::new(),
    };
    record.set(Capability::AssetRead, "notes.note", Answer::Ask);
    grants.set(&board.id, record.clone()).unwrap();
    assert_eq!(
        answer(&mut store, &grants, &board, &read),
        Some(Outcome::Asking(
            TypeName::try_from("notes.note").unwrap()
        ))
    );
    record.set(Capability::AssetRead, "notes.note", Answer::Allow);
    grants.set(&board.id, record).unwrap();
    assert_eq!(
        done(answer(&mut store, &grants, &notes, &update)),
        Ok(Reply::Updated(2))
    );
    // What was written comes back when the store is opened again.
    let mut store = AssetStore::open(&home);
    let Ok(Reply::Read(asset)) =
        done(answer(&mut store, &grants, &board, &read))
    else {
        panic!("the note was not read");
    };
    assert_eq!(
        (asset.info.version, &asset.bytes[..]),
        (2, &b"changed"[..])
    );

    // A plugin that takes the type over does not read what was made
    // before it.
    let taker = plugin("test.alpha", &["notes.note"], &[]);
    let (registry, _) =
        TypeRegistry::of([&taker, &notes].into_iter());
    let rules = Rules {
        registry: &registry,
        grants: &grants,
        budgets: &budgets,
    };
    assert!(matches!(
        done(rules.answer(&mut store, &taker, &read, false)),
        Err(Failure::Refused(_))
    ));
    let list = Request::ListAssets("notes.note".into());
    assert_eq!(
        done(rules.answer(&mut store, &taker, &list, false)),
        Ok(Reply::Listed(Vec::new()))
    );
    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn an_unreadable_index_is_never_written_over() {
    let home = home("held");
    let index = home.path(FOLDER).join(INDEX);
    fs::create_dir_all(home.path(FOLDER)).unwrap();
    let broken = "next = maybe\n";
    fs::write(&index, broken).unwrap();
    let mut store = AssetStore::open(&home);
    assert!(store.held().is_some());
    let kind = AssetType {
        name: TypeName::try_from("notes.note").unwrap(),
        version: 1,
    };
    let owner = PluginId::try_from("test.notes").unwrap();
    assert!(matches!(
        store.create(owner, kind, "First".into(), b"hello"),
        Err(Failure::Failed(_))
    ));
    assert_eq!(fs::read_to_string(&index).unwrap(), broken);
    let _ = fs::remove_dir_all(home.root());
}

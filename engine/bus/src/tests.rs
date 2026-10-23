use std::fs;

use hamlet_engine_api::{
    Ask, Capability, Failure, Manifest, PluginId, ServiceExport,
    ServiceId, ServiceImport, TypeName,
};
use hamlet_engine_core::Home;
use hamlet_engine_grants::{Answer, Grants, Record};
use hamlet_engine_plugins::{PluginInfo, PluginKind};

use crate::{Admitted, Call};

const WORDS: &str = "test.notes/words";

fn kind(name: &str) -> TypeName {
    TypeName::try_from(name).unwrap()
}

/// A plugin `id` that calls the words service with `argument` for
/// `result`, asking to.
fn caller(id: &str, argument: &str, result: &str) -> PluginInfo {
    let mut manifest =
        Manifest::new(PluginId::try_from(id).unwrap(), id, "0.1.0");
    manifest.services.imports.push(ServiceImport {
        service: ServiceId::try_from(WORDS).unwrap(),
        argument: kind(argument),
        result: kind(result),
    });
    manifest.asks.push(Ask {
        capability: Capability::ServiceCall,
        target: WORDS.into(),
        reason: "To count words".into(),
    });
    assert_eq!(manifest.check(), Ok(()));
    PluginInfo::of(manifest, PluginKind::Installed)
}

#[test]
fn a_call_goes_through_only_within_its_rules() {
    let root = std::env::temp_dir().join(format!(
        "hamlet_engine_test_bus_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let home = Home::at(&root);
    let mut notes = Manifest::new(
        PluginId::try_from("test.notes").unwrap(),
        "Notes",
        "0.1.0",
    );
    notes.services.exports.push(ServiceExport {
        name: "words".into(),
        argument: kind("notes.text"),
        result: kind("notes.count"),
    });
    let notes = PluginInfo::of(notes, PluginKind::Installed);
    let board = caller("test.board", "notes.text", "notes.count");
    let other = caller("test.other", "notes.text", "notes.size");
    let stranger =
        caller("test.stranger", "notes.text", "notes.count");
    let mut grants = Grants::open(&home);
    for (info, answer) in [
        (&board, Answer::Allow),
        (&other, Answer::Allow),
        (&stranger, Answer::Deny),
    ] {
        let mut record = Record {
            allowed: true,
            version: "0.1.0".into(),
            bundled: false,
            answers: Vec::new(),
        };
        record.set(Capability::ServiceCall, WORDS, answer);
        grants.set(&info.id, record).unwrap();
    }
    let cases = [
        ("one that agrees", &board, true, "to"),
        ("a signature that differs", &other, true, "mismatch"),
        ("one not allowed", &stranger, true, "refused"),
        ("one to a plugin off", &board, false, "no_implementer"),
    ];
    for (case, info, running, wanted) in cases {
        let call = Call {
            caller: info,
            service: WORDS,
            allowed: false,
        };
        let plugins = [(0_usize, &notes, running), (1, info, true)];
        let got = match call.admit(plugins.into_iter(), &grants) {
            Ok(Admitted::To(0, name)) if name == "words" => "to",
            Ok(_) => "elsewhere",
            Err(Failure::Mismatch(_)) => "mismatch",
            Err(Failure::Refused(_)) => "refused",
            Err(Failure::NoImplementer) => "no_implementer",
            Err(_) => "failed",
        };
        assert_eq!(got, wanted, "{case}");
    }
    let _ = fs::remove_dir_all(&root);
}

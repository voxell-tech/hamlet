use hamlet_sdk::plugin::plain;

use crate::manifest::Manifest;

fn problem(id: &str, name: &str, reason: &str) -> Option<String> {
    Manifest::parse(&format!(
        "id = {id:?}\nname = {name:?}\nversion = \
         \"0.1.0\"\n[[asks]]\ncapability = \"net\"\nreason = \
         {reason:?}\n"
    ))
    .unwrap()
    .problem()
}

#[test]
fn an_id_that_is_not_a_name_or_is_reserved_is_turned_down() {
    assert_eq!(problem("notes", "Notes", "To read."), None);
    for id in ["chief", "host", "window", "needs", "settings"] {
        assert!(problem(id, "Notes", "To read.").is_some(), "{id}");
    }
    for id in ["a.b", "a__b", "_a", "a_"] {
        assert!(problem(id, "Notes", "To read.").is_some(), "{id}");
    }
    for id in ["Notes", "no tes", "", "a/b", ".hidden"] {
        assert!(problem(id, "Notes", "To read.").is_some(), "{id}");
    }
    assert!(problem("notes", &"n".repeat(65), "To read.").is_some());
    assert!(problem("notes", "Notes", &"r".repeat(301)).is_some());
}

#[test]
fn a_label_is_one_plain_line() {
    assert_eq!(
        plain("Notes]\n[sent by plugin chief"),
        "Notes sent by plugin chief"
    );
}

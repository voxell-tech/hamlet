use std::os::unix::fs::symlink;

use hamlet_engine_api::desktop::WEB;
use hamlet_engine_api::{Ask, Capability};
use hamlet_testing::Scratch;

use crate::{Opening, opening};

#[test]
fn only_https_and_files_in_folders_asked_for_are_opened() {
    let all = Scratch::new("desktop-opening");
    let (granted, other) = (all.join("granted"), all.join("other"));
    std::fs::create_dir_all(granted.join("notes")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(granted.join("notes/a.md"), "").unwrap();
    std::fs::write(other.join("secret.txt"), "").unwrap();
    symlink(other.join("secret.txt"), granted.join("door.txt"))
        .unwrap();
    let ask = |target: &str| Ask {
        capability: Capability::DesktopOpen,
        target: target.into(),
        reason: "Opens what you click".into(),
    };
    let folder = granted.to_str().unwrap();
    let asks = [ask(WEB), ask(folder)];
    let opens = |written: &str| opening(&asks, written).ok();

    assert_eq!(
        opens("https://example.org/a?b#c"),
        Some((
            Opening::Web("https://example.org/a?b#c".into()),
            WEB.into()
        ))
    );
    for refused in [
        "http://example.org",
        "file:///etc/passwd",
        "javascript://alert",
        "https://",
        "https://user@example.org",
        "https://example.org/a b",
        "mailto:someone@example.org",
        "notes/a.md",
    ] {
        assert_eq!(opens(refused), None, "{refused} was opened");
    }
    let note = granted.join("notes/a.md");
    assert_eq!(
        opens(note.to_str().unwrap()),
        Some((Opening::File(note.clone()), folder.into()))
    );
    let around = granted.join("notes/../notes/a.md");
    assert!(opens(around.to_str().unwrap()).is_some());
    for refused in [
        granted.join("../other/secret.txt"),
        granted.join("door.txt"),
        granted.join("missing.md"),
        other.join("secret.txt"),
    ] {
        let written = refused.to_str().unwrap();
        assert_eq!(opens(written), None, "{written} was opened");
    }
    // Without the ask for https, no address is opened.
    assert_eq!(opening(&asks[1..], "https://example.org").ok(), None);
}

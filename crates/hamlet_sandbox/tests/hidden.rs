//! A hidden folder stays hidden from a command and from the file
//! operations, also inside a root that holds it (C18).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_sandbox::{
    AllLeftovers, Env, Launch, Policy, Sandbox, base_env, host_env,
};
use hamlet_testing::Scratch;

const CREDENTIALS: &str = "secret-credentials";
const OTHER: &str = "secret-other-agent";
const LATE: &str = "secret-made-later";

struct Realm {
    root: PathBuf,
}

impl Realm {
    fn make(root: PathBuf) -> Self {
        let realm = Self { root };
        for (path, words) in [
            (realm.credentials(), CREDENTIALS),
            (realm.other().join("notes.md"), OTHER),
            (realm.own().join("locked/kept.md"), "kept"),
            (realm.library().join("book.md"), "book"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, words).unwrap();
        }
        realm
    }

    fn credentials(&self) -> PathBuf {
        self.root.join("credentials.json")
    }

    fn own(&self) -> PathBuf {
        self.root.join("actors/agent/1.fs")
    }

    fn other(&self) -> PathBuf {
        self.root.join("actors/agent/3.fs")
    }

    fn library(&self) -> PathBuf {
        self.root.join("library")
    }
}

fn shown(path: &Path) -> String {
    format!("'{}'", path.display())
}

async fn run(env: &Env, script: &str) -> String {
    let argv =
        ["bash".to_owned(), "-c".to_owned(), script.to_owned()];
    let mut out = String::new();
    let _ = env
        .exec(
            Launch::Shell(&argv),
            Duration::from_secs(30),
            &[],
            &AllLeftovers::default().of(1),
            &mut |text| out.push_str(text),
        )
        .await;
    out
}

async fn breaks(env: &Env, realm: &Realm) -> Vec<String> {
    let mut found = Vec::new();
    let mut expect = |holds: bool, what: String| {
        if !holds {
            found.push(what);
        }
    };
    let credentials = realm.credentials();
    let notes = realm.other().join("notes.md");
    for (path, words) in
        [(&credentials, CREDENTIALS), (&notes, OTHER)]
    {
        expect(
            env.read_text(&path.to_string_lossy()).is_err(),
            format!("a file read reached {path:?}"),
        );
        let read = run(env, &format!("cat {}", shown(path))).await;
        expect(
            !read.contains(words),
            format!("a command read {path:?}"),
        );
        let mut kept = |what: String| {
            let holds = fs::read_to_string(path).ok().as_deref()
                == Some(words);
            expect(holds, what);
            fs::write(path, words).unwrap();
        };
        let _ = env.write_file(&path.to_string_lossy(), "changed");
        kept(format!("a file write changed {path:?}"));
        let script =
            format!("printf changed > {} && echo wrote", shown(path));
        let wrote = run(env, &script).await;
        kept(format!("a command changed {path:?}"));
        run(env, &format!("rm -f {}", shown(path))).await;
        kept(format!("a command removed {path:?}"));
        expect(
            !wrote.contains("wrote"),
            format!("a command's write of {path:?} succeeded"),
        );
    }
    let agents = realm.root.join("actors/agent");
    let listed = run(env, &format!("ls {}", shown(&agents))).await;
    expect(
        !listed.contains("3.fs"),
        "a command listed the agents".into(),
    );

    let mine = realm.own().join("mine.md");
    run(env, &format!("printf mine > {}", shown(&mine))).await;
    expect(
        fs::read_to_string(&mine).ok().as_deref() == Some("mine"),
        "a command could not write its own filesystem".into(),
    );
    let written = env.write_file(
        &realm.own().join("tool.md").to_string_lossy(),
        "tool",
    );
    expect(
        written.is_ok(),
        format!("a file write in its own filesystem: {written:?}"),
    );

    let book = realm.library().join("book.md");
    let kept = realm.own().join("locked/kept.md");
    for (path, words) in [(&book, "book"), (&kept, "kept")] {
        let read = run(env, &format!("cat {}", shown(path))).await;
        expect(
            read == words,
            format!("a command read {read:?} at {path:?}"),
        );
        run(env, &format!("printf changed > {}", shown(path))).await;
        let _ = env.write_file(&path.to_string_lossy(), "changed");
        expect(
            fs::read_to_string(path).unwrap() == words,
            format!("{path:?} was changed"),
        );
    }
    found
}

fn policy(project: &Path, realm: &Realm) -> Policy {
    Policy {
        ro: vec![realm.library()],
        rw: vec![project.to_owned(), realm.own()],
        open: true,
        hidden: vec![realm.root.clone()],
        locked: vec![realm.library(), realm.own().join("locked")],
        env: host_env(),
        ..Policy::default()
    }
}

#[tokio::test]
async fn a_project_that_holds_the_hidden_folder_keeps_it_hidden() {
    let mut failed = Vec::new();
    for (case, inside) in [
        ("in the project", "realm"),
        ("three down", "a/b/realm"),
        ("beside the project", "../realm"),
    ] {
        let scratch = Scratch::new("sandbox-hidden");
        let project = scratch.join("project");
        fs::create_dir_all(&project).unwrap();
        let realm = Realm::make(project.join(inside));
        let realm = Realm {
            root: fs::canonicalize(&realm.root).unwrap(),
        };
        let elsewhere = scratch.join("elsewhere.md");
        fs::write(&elsewhere, "seen").unwrap();
        let env = Env::new(
            &project.to_string_lossy(),
            policy(&project, &realm),
            Sandbox::testing(),
        )
        .unwrap();
        let mut found = breaks(&env, &realm).await;

        let link = project.join("link");
        symlink(&realm.root, &link).unwrap();
        let through = run(
            &env,
            &format!("cat {}", shown(&link.join("credentials.json"))),
        )
        .await;
        if through.contains(CREDENTIALS) {
            found.push("a command read through a link".into());
        }

        let late = realm.root.join("actors/agent/5.fs/late.md");
        let script = format!("sleep 2; cat {}", shown(&late));
        let (read, ()) = tokio::join!(run(&env, &script), async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            fs::create_dir_all(late.parent().unwrap()).unwrap();
            fs::write(&late, LATE).unwrap();
        });
        if read.contains(LATE) {
            found.push(
                "a command read what was made while it ran".into(),
            );
        }

        let note = project.join("note.md");
        run(&env, &format!("printf note > {}", shown(&note))).await;
        if fs::read_to_string(&note).ok().as_deref() != Some("note") {
            found
                .push("a command could not write the project".into());
        }
        let seen =
            run(&env, &format!("cat {}", shown(&elsewhere))).await;
        if seen != "seen" {
            found.push(format!("a command read {seen:?} elsewhere"));
        }
        failed.extend(
            found.into_iter().map(|each| format!("{case}: {each}")),
        );
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[tokio::test]
async fn a_read_root_that_holds_the_hidden_folder_keeps_it_hidden() {
    let scratch = Scratch::new("sandbox-hidden-read");
    let shared = scratch.join("shared");
    let realm = Realm::make(shared.join("realm"));
    let realm = Realm {
        root: fs::canonicalize(&realm.root).unwrap(),
    };
    fs::write(shared.join("shared.md"), "shared").unwrap();
    let policy = Policy {
        ro: vec![shared.clone(), realm.library()],
        rw: vec![realm.own()],
        hidden: vec![realm.root.clone()],
        locked: vec![realm.library(), realm.own().join("locked")],
        env: base_env(),
        ..Policy::default()
    };
    let env = Env::new(
        &realm.own().to_string_lossy(),
        policy,
        Sandbox::testing(),
    )
    .unwrap();
    let mut found = breaks(&env, &realm).await;
    let read = run(
        &env,
        &format!("cat {}", shown(&shared.join("shared.md"))),
    )
    .await;
    if read != "shared" {
        found.push(format!(
            "a command read {read:?} in the read root"
        ));
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[tokio::test]
async fn a_hidden_folder_named_through_a_link_stays_hidden() {
    let scratch = Scratch::new("sandbox-hidden-link");
    let project = scratch.join("project");
    let real = Realm::make(project.join("realm"));
    let alias = scratch.join("alias");
    symlink(fs::canonicalize(&project).unwrap(), &alias).unwrap();
    let realm = Realm {
        root: alias.join("realm"),
    };
    let env = Env::new(
        &project.to_string_lossy(),
        policy(&project, &realm),
        Sandbox::testing(),
    )
    .unwrap();
    let mut found = breaks(&env, &realm).await;
    let read =
        run(&env, &format!("cat {}", shown(&real.credentials())))
            .await;
    if read.contains(CREDENTIALS) {
        found.push("a command read it by its real name".into());
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[tokio::test]
async fn a_write_root_inside_a_locked_folder_stays_read_only() {
    let scratch = Scratch::new("sandbox-locked-cache");
    let dir = scratch.join("tool");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).unwrap();
    let policy = Policy {
        ro: vec![dir.clone()],
        rw: vec![cache.clone()],
        locked: vec![dir],
        env: base_env(),
        ..Policy::default()
    };
    let env = Env::new(
        &cache.to_string_lossy(),
        policy,
        Sandbox::testing(),
    )
    .unwrap();
    let new = cache.join("new.md");
    let wrote = run(
        &env,
        &format!("printf x > {} && echo wrote", shown(&new)),
    )
    .await;
    let by_file = env.write_file(&new.to_string_lossy(), "x");
    assert!(!wrote.contains("wrote"), "a command wrote: {wrote:?}");
    assert!(by_file.is_err(), "a file write: {by_file:?}");
    assert!(!new.exists());
}

#[tokio::test]
async fn a_write_root_at_a_hidden_place_is_granted() {
    let scratch = Scratch::new("sandbox-hidden-granted");
    let dir = scratch.join("folder");
    fs::create_dir_all(&dir).unwrap();
    let policy = Policy {
        rw: vec![dir.clone()],
        open: true,
        hidden: vec![dir.clone()],
        env: host_env(),
        ..Policy::default()
    };
    let env =
        Env::new(&dir.to_string_lossy(), policy, Sandbox::testing())
            .unwrap();
    let new = dir.join("new.md");
    let wrote = run(
        &env,
        &format!("printf x > {} && echo wrote", shown(&new)),
    )
    .await;
    assert!(wrote.contains("wrote"), "{wrote:?}");
    assert_eq!(fs::read_to_string(&new).unwrap(), "x");
}

//! A dream's errand works in its main's checkout inside the realm
//! folder, writes its hire's, reaches nothing else of the realm
//! folder and never waits for a chief.

use std::path::Path;

use hamlet_messages::workspace::Exit;
use hamlet_messages::workspace::files::{
    ChangeAnswer, ChangeEnd, ReadAnswer, ReadFile,
};
use hamlet_messages::workspace::reach::{Reach, SecretsVersion};
use hamlet_messages::workspace::refusal::{
    PathRefusal, ToChange, ToRead, WriteRefusal,
};
use hamlet_testing::Scratch;

use super::changes::{read, refused, write};
use super::files::ask;
use super::{AGENT, agent, agent_runs, done, host, realm_sets, run};

const CREDENTIALS: &str = "secret-credentials";
const OTHER: &str = "secret-other-agent";
const ANOTHER_DREAM: &str = "secret-another-dream";

fn reading(path: &Path) -> ReadFile {
    ReadFile {
        path: path.to_owned(),
        offset: None,
        limit: None,
    }
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.display())
}

fn dream(folder: &Path) -> Reach {
    Reach::Dream {
        main: folder.join("main"),
        hire: Some(folder.join("hire")),
        network: false,
        secrets: SecretsVersion(1),
    }
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_dreams_errand_works_in_its_main_and_nowhere_else_in_the_realm()
 {
    let scratch = Scratch::new("workspace-dream");
    let dir = scratch.join("realm");
    let folder = dir.join("dreams/3");
    put(&folder.join("hire/memory/kept.md"), "kept");
    std::fs::create_dir_all(folder.join("main/memory")).unwrap();
    let credentials = dir.join("credentials.json");
    let outside = scratch.join("outside.md");
    let notes = dir.join("actors/agent/9.fs/notes.md");
    let other_dream = dir.join("dreams/4/main/memory/other.md");
    put(&credentials, CREDENTIALS);
    put(&notes, OTHER);
    put(&other_dream, ANOTHER_DREAM);
    let host = host(&dir).await;
    realm_sets(&host, 1, dream(&folder)).await;

    let moved = agent_runs(
        &host,
        &run("m1", "mv ../hire/memory/kept.md memory/kept.md"),
    )
    .await;
    let relative = agent_runs(
        &host,
        &run("r1", "printf relative > memory/relative.md"),
    )
    .await;
    let networked = agent_runs(
        &host,
        &run("n1", "curl -s http://127.0.0.1:9/ || true"),
    )
    .await;
    let mut commands = Vec::new();
    for (n, script) in [
        format!("cat {}", quoted(&credentials)),
        format!("cat {}", quoted(&notes)),
        format!("cat {}", quoted(&other_dream)),
        format!("printf changed > {}", quoted(&credentials)),
        format!("printf changed > {}", quoted(&other_dream)),
        format!("ls {}", quoted(&dir.join("dreams"))),
        format!("printf out > {}", quoted(&outside)),
    ]
    .into_iter()
    .enumerate()
    {
        let end = done(
            agent_runs(&host, &run(&format!("c{n}"), &script)).await,
        );
        commands.push((script, end.output));
    }
    let wrote: ChangeAnswer<WriteRefusal> = ask(
        &host,
        &agent(AGENT),
        &write("w1", &folder.join("main/memory/new.md"), "new"),
    )
    .await;
    let read_back: ReadAnswer = ask(
        &host,
        &agent(AGENT),
        &reading(&folder.join("main/memory/kept.md")),
    )
    .await;
    let mut reads = Vec::new();
    for path in [&credentials, &other_dream] {
        let answer: ReadAnswer =
            ask(&host, &agent(AGENT), &reading(path)).await;
        reads.push(answer);
    }
    let mut changes = Vec::new();
    for (n, path) in
        [&other_dream, &notes, &outside].iter().enumerate()
    {
        let answer: ChangeAnswer<WriteRefusal> = ask(
            &host,
            &agent(AGENT),
            &write(&format!("x{n}"), path, "changed"),
        )
        .await;
        changes.push(answer);
    }
    host.shutdown().await.unwrap();

    assert_eq!(done(moved).exit, Exit::Code { code: 0 });
    assert_eq!(read(&folder.join("main/memory/kept.md")), "kept");
    assert!(!folder.join("hire/memory/kept.md").exists());
    assert_eq!(done(relative).exit, Exit::Code { code: 0 });
    assert_eq!(
        read(&folder.join("main/memory/relative.md")),
        "relative"
    );
    assert_eq!(done(networked).exit, Exit::Code { code: 0 });
    for (script, output) in &commands {
        for words in [CREDENTIALS, OTHER, ANOTHER_DREAM] {
            assert!(
                !output.contains(words),
                "{script} printed {output:?}"
            );
        }
    }
    let (_, listed) = &commands[5];
    assert!(!listed.contains('4'), "listed {listed:?}");
    assert_eq!(read(&credentials), CREDENTIALS);
    assert_eq!(read(&other_dream), ANOTHER_DREAM);
    assert!(
        matches!(
            wrote,
            ChangeAnswer::Ended {
                end: ChangeEnd::Changed(_)
            }
        ),
        "{wrote:?}"
    );
    assert_eq!(read(&folder.join("main/memory/new.md")), "new");
    assert!(
        matches!(
            &read_back,
            ReadAnswer::Text(read) if read.text.contains("kept")
        ),
        "{read_back:?}"
    );
    assert_eq!(
        reads,
        [credentials.clone(), other_dream.clone()].map(|path| {
            ReadAnswer::Refused {
                why: PathRefusal::OutsideReach {
                    path,
                    access: ToRead::Read,
                }
                .into(),
            }
        })
    );
    assert_eq!(
        changes,
        [
            refused(PathRefusal::OutsideReach {
                path: other_dream.clone(),
                access: ToChange::Change,
            }),
            refused(PathRefusal::<ToChange>::AgentFilesystem {
                path: notes.clone()
            }),
            refused(PathRefusal::OutsideReach {
                path: outside.clone(),
                access: ToChange::Change,
            }),
        ]
    );
    assert_eq!(read(&notes), OTHER);
    assert!(!outside.exists());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_dreams_folder_named_through_a_linked_realm_folder_works() {
    let scratch = Scratch::new("workspace-dream-linked");
    let real = scratch.join("var/realm");
    put(&real.join("dreams/3/hire/memory/kept.md"), "kept");
    std::fs::create_dir_all(real.join("dreams/3/main")).unwrap();
    put(&real.join("credentials.json"), CREDENTIALS);
    let dir = scratch.join("realm");
    std::os::unix::fs::symlink(&real, &dir).unwrap();
    let folder = dir.join("dreams/3");
    let host = host(&dir).await;
    realm_sets(&host, 1, dream(&folder)).await;

    let moved =
        agent_runs(&host, &run("m1", "mv ../hire/memory/kept.md ."))
            .await;
    let wrote: ChangeAnswer<WriteRefusal> = ask(
        &host,
        &agent(AGENT),
        &write("w1", &folder.join("main/new.md"), "new"),
    )
    .await;
    let read_back: ReadAnswer = ask(
        &host,
        &agent(AGENT),
        &reading(&folder.join("main/kept.md")),
    )
    .await;
    let credentials = done(
        agent_runs(
            &host,
            &run(
                "c1",
                &format!(
                    "cat {}",
                    quoted(&dir.join("credentials.json"))
                ),
            ),
        )
        .await,
    );
    host.shutdown().await.unwrap();

    assert_eq!(done(moved).exit, Exit::Code { code: 0 });
    assert_eq!(read(&real.join("dreams/3/main/kept.md")), "kept");
    assert!(
        matches!(
            wrote,
            ChangeAnswer::Ended {
                end: ChangeEnd::Changed(_)
            }
        ),
        "{wrote:?}"
    );
    assert_eq!(read(&real.join("dreams/3/main/new.md")), "new");
    assert!(
        matches!(
            &read_back,
            ReadAnswer::Text(read) if read.text.contains("kept")
        ),
        "{read_back:?}"
    );
    assert!(!credentials.output.contains(CREDENTIALS));
}

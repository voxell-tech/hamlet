//! A dream's errand works in its dream's folder inside the realm
//! folder, reaches nothing else of the realm folder and never waits
//! for a chief.

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

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_dreams_errand_works_in_its_folder_and_nowhere_else_in_the_realm()
 {
    let scratch = Scratch::new("workspace-dream");
    let dir = scratch.join("realm");
    let folder = dir.join("dreams/3");
    put(&folder.join("hire/memory/kept.md"), "kept");
    std::fs::create_dir_all(folder.join("main/memory")).unwrap();
    let credentials = dir.join("credentials.json");
    let notes = dir.join("actors/agent/9.fs/notes.md");
    let other_dream = dir.join("dreams/4/main/memory/other.md");
    put(&credentials, CREDENTIALS);
    put(&notes, OTHER);
    put(&other_dream, ANOTHER_DREAM);
    let host = host(&dir).await;
    realm_sets(
        &host,
        1,
        Reach::Dream {
            folder: folder.clone(),
            secrets: SecretsVersion(1),
        },
    )
    .await;

    let moved = agent_runs(
        &host,
        &run("m1", "mv hire/memory/kept.md main/memory/kept.md"),
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
    for (n, path) in [&other_dream, &notes].iter().enumerate() {
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
        ]
    );
    assert_eq!(read(&notes), OTHER);
}

//! C18: an agent never reaches the rest of the realm's folder, also
//! when the folder its hamlet works on holds it, and works on no
//! folder inside it.

use std::path::Path;

use hamlet_messages::workspace::files::{
    ChangeAnswer, FileAnswer, ReadFile,
};
use hamlet_messages::workspace::reach::{Access, Refusal};
use hamlet_messages::workspace::{CallRefusal, RunAnswer};
use hamlet_testing::Scratch;

use super::changes::{read, write};
use super::files::ask;
use super::{
    AGENT, agent, agent_runs, done, filesystem, host, reach,
    realm_sets, run,
};

const CREDENTIALS: &str = "secret-credentials";
const OTHER: &str = "secret-other-agent";

fn reading(path: &Path) -> ReadFile {
    ReadFile {
        agent: AGENT,
        path: path.to_owned(),
        offset: None,
        limit: None,
    }
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.display())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_project_that_holds_the_realm_folder_reaches_none_of_it() {
    for inside in ["realm", "a/b/realm"] {
        let scratch = Scratch::new("workspace-realm-in-project");
        let project = scratch.join("project");
        let dir = project.join(inside);
        let own = filesystem(&dir);
        let credentials = dir.join("credentials.json");
        let notes = dir.join("actors/agent/9.fs/notes.md");
        std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
        std::fs::write(&credentials, CREDENTIALS).unwrap();
        std::fs::write(&notes, OTHER).unwrap();
        let host = host(&dir).await;
        realm_sets(&host, 1, reach(project.to_str())).await;

        let mut commands = Vec::new();
        for (n, script) in [
            format!("cat {}", quoted(&credentials)),
            format!("cat {}", quoted(&notes)),
            format!("printf changed > {}", quoted(&credentials)),
            format!("printf changed > {}", quoted(&notes)),
            format!("ls {}", quoted(&dir.join("actors/agent"))),
            format!("printf mine > {}", quoted(&own.join("mine.md"))),
            format!(
                "printf note > {}",
                quoted(&project.join("note.md"))
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let end = done(
                agent_runs(&host, &run(&format!("c{n}"), &script))
                    .await,
            );
            commands.push((script, end.output));
        }
        let mut reads = Vec::new();
        for path in [&credentials, &notes] {
            let answer: FileAnswer =
                ask(&host, &agent(AGENT), &reading(path)).await;
            reads.push(answer);
        }
        let mut changes = Vec::new();
        for (n, path) in [&credentials, &notes].iter().enumerate() {
            let answer: ChangeAnswer = ask(
                &host,
                &agent(AGENT),
                &write(&format!("w{n}"), path, "changed"),
            )
            .await;
            changes.push(answer);
        }
        host.shutdown().await.unwrap();

        for (script, output) in &commands {
            for words in [CREDENTIALS, OTHER] {
                assert!(
                    !output.contains(words),
                    "{inside}: {script} printed {output:?}"
                );
            }
        }
        let (_, listed) = &commands[4];
        assert!(
            !listed.contains("9.fs"),
            "{inside}: listed {listed:?}"
        );
        assert_eq!(read(&credentials), CREDENTIALS, "{inside}");
        assert_eq!(read(&notes), OTHER, "{inside}");
        assert_eq!(read(&own.join("mine.md")), "mine", "{inside}");
        assert_eq!(
            read(&project.join("note.md")),
            "note",
            "{inside}"
        );
        assert_eq!(
            reads,
            [
                FileAnswer::Refused(CallRefusal::Reach(
                    Refusal::OutsideReach {
                        path: credentials.clone(),
                        access: Access::Read,
                    }
                )),
                FileAnswer::Refused(CallRefusal::Reach(
                    Refusal::AgentFilesystem {
                        path: notes.clone()
                    }
                )),
            ],
            "{inside}"
        );
        assert_eq!(
            changes,
            [
                ChangeAnswer::Refused(CallRefusal::Reach(
                    Refusal::OutsideReach {
                        path: credentials.clone(),
                        access: Access::Change,
                    }
                )),
                ChangeAnswer::Refused(CallRefusal::Reach(
                    Refusal::AgentFilesystem {
                        path: notes.clone()
                    }
                )),
            ],
            "{inside}"
        );
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_project_in_the_realm_folder_is_refused() {
    let scratch = Scratch::new("workspace-project-in-realm");
    let dir = scratch.join("realm");
    filesystem(&dir);
    let notes = dir.join("projects/site/notes.md");
    std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
    std::fs::write(&notes, "notes").unwrap();
    let alias = scratch.join("alias");
    std::os::unix::fs::symlink(&dir, &alias).unwrap();
    for (n, folder) in [
        dir.clone(),
        dir.join("projects/site"),
        alias.join("projects/site"),
    ]
    .into_iter()
    .enumerate()
    {
        let host = host(&dir).await;
        realm_sets(&host, n as u64 + 1, reach(folder.to_str())).await;
        let refusal = CallRefusal::Reach(Refusal::ProjectInRealm {
            folder: folder.clone(),
        });
        let ran = agent_runs(
            &host,
            &run(
                &format!("c{n}"),
                &format!("printf x > {}", quoted(&notes)),
            ),
        )
        .await;
        let reads: FileAnswer =
            ask(&host, &agent(AGENT), &reading(&notes)).await;
        let changes: ChangeAnswer = ask(
            &host,
            &agent(AGENT),
            &write(&format!("w{n}"), &notes, "x"),
        )
        .await;
        host.shutdown().await.unwrap();
        let case = folder.display();
        assert_eq!(
            ran,
            RunAnswer::Refused(refusal.clone()),
            "{case}"
        );
        assert_eq!(
            reads,
            FileAnswer::Refused(refusal.clone()),
            "{case}"
        );
        assert_eq!(changes, ChangeAnswer::Refused(refusal), "{case}");
        assert_eq!(read(&notes), "notes", "{case}");
    }
}

//! C18: an agent never reaches the rest of the realm's folder, also
//! when the folder its hamlet works on holds it, and works on no
//! folder inside it.

use std::path::Path;

use hamlet_messages::workspace::files::{
    ChangeAnswer, FileAnswer, ReadFile,
};
use hamlet_messages::workspace::reach::{Access, Refusal};
use hamlet_messages::workspace::{CallRefusal, Exit, RunAnswer};
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
                FileAnswer::Refused {
                    why: CallRefusal::Reach {
                        why: Refusal::OutsideReach {
                            path: credentials.clone(),
                            access: Access::Read,
                        }
                    }
                },
                FileAnswer::Refused {
                    why: CallRefusal::Reach {
                        why: Refusal::AgentFilesystem {
                            path: notes.clone()
                        }
                    }
                },
            ],
            "{inside}"
        );
        assert_eq!(
            changes,
            [
                ChangeAnswer::Refused {
                    why: CallRefusal::Reach {
                        why: Refusal::OutsideReach {
                            path: credentials.clone(),
                            access: Access::Change,
                        }
                    }
                },
                ChangeAnswer::Refused {
                    why: CallRefusal::Reach {
                        why: Refusal::AgentFilesystem {
                            path: notes.clone()
                        }
                    }
                },
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
        let refusal = CallRefusal::Reach {
            why: Refusal::ProjectInRealm {
                folder: folder.clone(),
            },
        };
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
            RunAnswer::Refused {
                why: refusal.clone()
            },
            "{case}"
        );
        assert_eq!(
            reads,
            FileAnswer::Refused {
                why: refusal.clone()
            },
            "{case}"
        );
        assert_eq!(
            changes,
            ChangeAnswer::Refused { why: refusal },
            "{case}"
        );
        assert_eq!(read(&notes), "notes", "{case}");
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_realm_folder_reached_through_a_link_works() {
    let scratch = Scratch::new("workspace-linked-realm");
    let real = scratch.join("var/realm");
    filesystem(&real);
    let credentials = real.join("credentials.json");
    std::fs::write(&credentials, CREDENTIALS).unwrap();
    let dir = scratch.join("realm");
    std::os::unix::fs::symlink(&real, &dir).unwrap();
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let note = project.join("note.md");
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(project.to_str())).await;

    let ran = agent_runs(
        &host,
        &run("c1", &format!("printf ran > {}", quoted(&note))),
    )
    .await;
    let wrote: ChangeAnswer = ask(
        &host,
        &agent(AGENT),
        &write("w1", &project.join("a.md"), "written"),
    )
    .await;
    let read_back: FileAnswer =
        ask(&host, &agent(AGENT), &reading(&project.join("a.md")))
            .await;
    let own = dir.join("actors/agent/7.fs/x");
    let into_own: ChangeAnswer =
        ask(&host, &agent(AGENT), &write("w2", &own, "x")).await;
    let mut commands = Vec::new();
    let mut reads = Vec::new();
    for (n, path) in
        [dir.join("credentials.json"), credentials.clone()]
            .iter()
            .enumerate()
    {
        let end = done(
            agent_runs(
                &host,
                &run(
                    &format!("r{n}"),
                    &format!("cat {}", quoted(path)),
                ),
            )
            .await,
        );
        commands.push(end.output);
        let answer: FileAnswer =
            ask(&host, &agent(AGENT), &reading(path)).await;
        reads.push(answer);
    }
    host.shutdown().await.unwrap();

    assert_eq!(done(ran).exit, Exit::Code { code: 0 });
    assert_eq!(read(&note), "ran");
    assert!(matches!(wrote, ChangeAnswer::Changed(_)), "{wrote:?}");
    assert!(
        matches!(
            &read_back,
            FileAnswer::Text(read) if read.text.contains("written")
        ),
        "{read_back:?}"
    );
    assert_eq!(
        into_own,
        ChangeAnswer::Refused {
            why: CallRefusal::Reach {
                why: Refusal::AgentFilesystem { path: own }
            }
        }
    );
    for output in &commands {
        assert!(!output.contains(CREDENTIALS), "{output:?}");
    }
    for answer in &reads {
        assert!(
            matches!(
                answer,
                FileAnswer::Refused {
                    why: CallRefusal::Reach {
                        why: Refusal::OutsideReach { .. }
                    }
                }
            ),
            "{answer:?}"
        );
    }
    assert_eq!(read(&credentials), CREDENTIALS);
}

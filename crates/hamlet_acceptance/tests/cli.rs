//! The `hamlet` command against a served home: chat on stdin and
//! stdout, approvals, switching agents and hamlets, and documents in
//! EDITOR.

#[path = "cli/processes.rs"]
mod processes;

use std::path::PathBuf;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

/// An EDITOR that copies what it was given to `seen`, then runs
/// `code` with the document's path in `$1`.
struct Editor {
    command: String,
    seen: PathBuf,
}

fn editor(s: &Session, name: &str, code: &str) -> Editor {
    let script = s.home.side().join(format!("{name}.sh"));
    let seen = s.home.side().join(format!("{name}.seen"));
    std::fs::write(
        &script,
        format!("cp \"$1\" '{}'\n{code}\n", seen.display()),
    )
    .unwrap();
    Editor {
        command: format!("sh {}", script.display()),
        seen,
    }
}

impl Editor {
    fn seen(&self) -> String {
        std::fs::read_to_string(&self.seen).unwrap()
    }
}

async fn edit(s: &Session, doc: &[&str], editor: &Editor) -> Ended {
    let args = [&["edit"], doc].concat();
    Cli::run(s.home.path(), &args, "", &[("EDITOR", &editor.command)])
        .await
}

#[tokio::test]
async fn chats_with_answers_on_stdout_and_the_rest_on_stderr() {
    let mut s = Session::start(Faux::new().responses([
        call("bash", json!({"command": "echo hi"})),
        words("It says hi."),
        words("Nothing else."),
    ]))
    .await;
    let chat = Cli::run(
        s.home.path(),
        &["chat"],
        "What does it say?\n\nAnything else?\n",
        &[],
    )
    .await;
    assert_eq!(chat.code(), Some(0), "{chat:?}");
    assert_eq!(chat.out, "It says hi.\nNothing else.\n");
    assert!(
        chat.err.contains("\n  [bash] {\"command\":\"echo hi\"}\n"),
        "{chat:?}"
    );
    assert!(s.server.is_running());
}

#[tokio::test]
async fn ends_with_a_failure_when_its_agent_does_not_exist() {
    let mut s = Session::start(Faux::new()).await;
    for input in ["", "Hello.\n"] {
        let chat = Cli::run(
            s.home.path(),
            &["chat", "--to", "nobody"],
            input,
            &[],
        )
        .await;
        assert_eq!(chat.code(), Some(1), "{chat:?}");
        assert_eq!(chat.out, "");
        assert!(
            chat.err.contains("no agent is at `nobody`"),
            "{chat:?}"
        );
    }
    assert!(s.server.is_running());
}

#[tokio::test]
async fn starts_at_another_hamlet_and_switches_agents_and_hamlets() {
    let mut s = Session::start(
        Faux::new()
            .agent(1, [words("Hand answer.")])
            .agent(2, [words("Ada answer."), words("Ada again.")])
            .agent(3, [words("Bo answer.")]),
    )
    .await;
    s.found("docs", "ada", "keep the docs").await;
    s.found("mill", "bo", "grind").await;
    let (docs, mill) =
        (s.chief.hamlet_id("docs"), s.chief.hamlet_id("mill"));
    let home = s.home.path();
    let chat = Cli::run(
        home,
        &["chat", "--to", &format!("mayor@{mill}")],
        &format!("Mill?\n/hamlet {docs}\nDocs?\n/hand\nHand?\n"),
        &[],
    )
    .await;
    assert_eq!(chat.code(), Some(0), "{chat:?}");
    assert_eq!(
        chat.out, "Bo answer.\nAda answer.\nHand answer.\n",
        "{chat:?}"
    );
    let switched = Cli::run(
        home,
        &["chat"],
        "/agent hire#2\nAgain?\n/agent missing\n",
        &[],
    )
    .await;
    assert_eq!(switched.code(), Some(0), "{switched:?}");
    assert!(switched.out.contains("Ada again.\n"), "{switched:?}");
    assert!(
        switched.err.contains("no agent is at `missing`"),
        "{switched:?}"
    );
}

#[tokio::test]
async fn asks_for_approvals_and_keeps_them_across_a_crash() {
    let faux = Faux::new().agent(
        1,
        [
            call("bash", json!({"command": "echo curl one > one"})),
            words("Done."),
            call("bash", json!({"command": "echo curl two > two"})),
        ],
    );
    let mut s = Session::start(faux).await;
    let mut chat = Cli::start(s.home.path(), &["chat"], &[]);
    chat.write("Fetch the first.\n").await;
    chat.err_shows(
        "NEEDS YOU: [approval] hand wants network access to run",
    )
    .await;
    chat.write("/allow\n").await;
    chat.out_shows("Done.\n").await;
    assert_eq!(chat.err().matches("NEEDS YOU:").count(), 1);
    chat.write("/budget\n").await;
    chat.err_shows("Realm today:").await;
    chat.write("Fetch the second.\n").await;
    chat.until("the second ask", |c| {
        c.err().matches("NEEDS YOU:").count() == 2
    })
    .await;
    let wait = s
        .chief
        .wait_for("the second approval", |w| {
            w.subject.contains("two")
        })
        .await;
    let Session {
        mut home,
        mut server,
        chief,
    } = s;
    drop(chief);
    server.kill();
    // The command ends when its server goes.
    assert_eq!(chat.wait().await.code(), Some(1));
    let again = home
        .serve(
            &Faux::new().agent(1, [words("After restart.")]),
            QUIET,
        )
        .await;
    let chief = Client::join(&again).await;
    assert_eq!(chief.waits(), std::slice::from_ref(&wait));
    let mut next = Cli::start(home.path(), &["chat"], &[]);
    next.err_shows(
        "NEEDS YOU: [approval] hand wants network access to run",
    )
    .await;
    next.write("/allow\n").await;
    next.out_shows("After restart.\n").await;
    let read = std::fs::read_to_string(home.file("hand/two"));
    let Ok(two) = read else {
        let mut chief = chief;
        let hand = hamlet_acceptance::chat(chief.hand());
        let tools = chief
            .open(&hand)
            .await
            .items
            .into_iter()
            .filter_map(|item| item.tool)
            .collect::<Vec<_>>();
        panic!(
            "hand/two: {read:?}\ntools: {tools:#?}\nerr: {}\nlog: {}",
            next.err(),
            again.log()
        );
    };
    assert_eq!(two, "curl two\n");
    assert_eq!(next.end().await.code(), Some(0));
}

#[tokio::test]
async fn edits_limits_in_editor_and_reports_a_refusal() {
    let mut s = Session::start(Faux::new()).await;
    s.found("docs", "ada", "keep the docs").await;
    let docs = s.chief.hamlet_id("docs");
    let cost = |from: i32, to: i32| {
        format!(
            "sed -E 's/\"cost\": {from}(\\.0)?,/\"cost\": {to},/' \
             \"$1\" > \"$1.next\" && mv \"$1.next\" \"$1\""
        )
    };
    let good = editor(&s, "good", &cost(5, 8));
    let saved = edit(&s, &["limits", "--hamlet", &docs], &good).await;
    assert_eq!(saved.code(), Some(0), "{saved:?}");
    assert!(saved.err.contains("saved"), "{saved:?}");
    let seen =
        serde_json::from_str::<serde_json::Value>(&good.seen())
            .unwrap();
    assert_eq!(seen["budget"]["period"], "day");
    assert_eq!(seen["task"]["messages"], 12);
    s.chief
        .until("the new cost", |c| {
            c.hamlet("docs").unwrap().limits.budget.cost == Some(8.0)
        })
        .await;
    // The realm's own limits are another document.
    let realm = editor(&s, "realm", "");
    let seen = edit(&s, &["limits"], &realm).await;
    assert_eq!(seen.code(), Some(0), "{seen:?}");
    assert!(
        realm.seen().contains("roadsBetween"),
        "{}",
        realm.seen()
    );
    let broken = editor(&s, "broken", "echo '{' > \"$1\"");
    let refused =
        edit(&s, &["limits", "--hamlet", &docs], &broken).await;
    assert_eq!(refused.code(), Some(1), "{refused:?}");
    assert_eq!(
        s.chief.hamlet("docs").unwrap().limits.budget.cost,
        Some(8.0)
    );
}

#[tokio::test]
async fn edits_a_scholars_character_and_memory_in_editor() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let scholar = s.chief.scholar("ada").unwrap().id;
    let write =
        editor(&s, "write", "echo 'Brave and plain.' > \"$1\"");
    let saved = edit(
        &s,
        &["character", &format!("scholar#{}", scholar.0)],
        &write,
    )
    .await;
    assert_eq!(saved.code(), Some(0), "{saved:?}");
    let note = editor(
        &s,
        "note",
        "printf -- '---\\ndescription: Tea\\n---\\nEarl grey.\\n' > \
         \"$1\"",
    );
    let saved =
        edit(&s, &["memory", "hire#2", "tea.md"], &note).await;
    assert_eq!(saved.code(), Some(0), "{saved:?}");
    let papers = s.chief.papers(Keeper::Agent(ada)).await;
    assert_eq!(papers.character.trim(), "Brave and plain.");
    assert_eq!(papers.memory[0].path, "tea.md");
    // The scholar's own memory is another file than the hire's.
    assert!(
        s.chief
            .papers(Keeper::Scholar(scholar))
            .await
            .memory
            .is_empty()
    );
    let failing =
        editor(&s, "failing", "echo 'not saved' > \"$1\"; exit 2");
    let failed = edit(
        &s,
        &["character", &format!("scholar#{}", scholar.0)],
        &failing,
    )
    .await;
    assert_eq!(failed.code(), Some(1), "{failed:?}");
    assert!(
        failed.err.contains("The editor ended with 2"),
        "{failed:?}"
    );
}

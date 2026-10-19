//! The `hamlet` command against a served home: chat on stdin and
//! stdout, approvals, switching agents and hamlets, documents in
//! EDITOR, and models.

#[path = "cli/processes.rs"]
mod processes;

use std::path::PathBuf;

use hamlet_acceptance::*;
use hamlet_protocol::v0::*;
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

/// An EDITOR that changes the budget's cost from `from` to `to`.
fn cost_editor(
    s: &Session,
    name: &str,
    from: i32,
    to: i32,
) -> Editor {
    let code = format!(
        "sed -E 's/\"cost\": {from}(\\.0)?,/\"cost\": {to},/' \
         \"$1\" > \"$1.next\" && mv \"$1.next\" \"$1\""
    );
    editor(s, name, &code)
}

async fn edit(s: &Session, doc: &[&str], editor: &Editor) -> Ended {
    let args = [&["edit"], doc].concat();
    Cli::run(s.home.path(), &args, "", &[("EDITOR", &editor.command)])
        .await
}

#[tokio::test]
async fn prints_its_usage_on_stdout() {
    let s = Session::start(Script::new()).await;
    let help = Cli::run(s.home.path(), &["--help"], "", &[]).await;
    assert_eq!(help.code(), Some(0));
    assert!(help.out.starts_with("usage: hamlet"), "{help:?}");
    assert_eq!(help.err, "");
    for args in [
        &["server", "--port", "0", "-h"][..],
        &["--help", "chat"],
        &["chat", "--to", "-h"],
    ] {
        let help = Cli::run(s.home.path(), args, "", &[]).await;
        assert_eq!(help.code(), Some(0), "{args:?}: {help:?}");
        assert!(help.out.starts_with("usage: hamlet"), "{help:?}");
    }
    for (args, words) in [
        (
            &["server", "--hedless"][..],
            "unknown argument `--hedless`",
        ),
        (&["server", "--port"], "--port needs a port number"),
        (&["server", "--faux"], "--faux needs the path of a script"),
        (&["chat", "--to"], "--to needs an agent's name"),
        (
            &["server", "--port", "high"],
            "--port needs a port number, not `high`",
        ),
        (&["serve"], "unknown argument `serve`"),
        (&["chat", "--port", "1"], "unknown argument `--port`"),
    ] {
        let wrong = Cli::run(s.home.path(), args, "", &[]).await;
        assert_eq!(wrong.code(), Some(1), "{args:?}: {wrong:?}");
        assert!(wrong.err.contains(words), "{args:?}: {wrong:?}");
        assert_eq!(wrong.out, "");
    }
}

#[tokio::test]
async fn chats_with_answers_on_stdout_and_the_rest_on_stderr() {
    let mut s = Session::start(Script::new().responses([
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
    assert!(s.server.running());
}

#[tokio::test]
async fn ends_with_a_failure_when_its_agent_does_not_exist() {
    let mut s = Session::start(Script::new()).await;
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
            chat.err.contains("no agent named `nobody`"),
            "{chat:?}"
        );
    }
    assert!(s.server.running());
}

#[tokio::test]
async fn prints_a_notice_for_people_on_stderr() {
    let mut s = Session::start(
        Script::new().responses([words("Never sent.")]),
    )
    .await;
    s.chief.limit("home", "budget", "cost", json!(0)).await;
    let chat =
        Cli::run(s.home.path(), &["chat"], "Hello.\n", &[]).await;
    assert_eq!(chat.code(), Some(0), "{chat:?}");
    assert_eq!(chat.out, "");
    let notice = chat
        .err
        .lines()
        .find(|l| l.starts_with("  (") && l.ends_with(')'));
    assert!(notice.is_some_and(|l| l.contains("budget")), "{chat:?}");
}

#[tokio::test]
async fn asks_for_approvals_and_keeps_them_across_a_crash() {
    let script = Script::new().responses([
        call("write", json!({"path": "a.txt", "content": "one"})),
        words("Done."),
        call("bash", json!({"command": "echo started; sleep 30"})),
        call(
            "write",
            json!({"path": "b.txt", "content": "after restart"}),
        ),
    ]);
    let mut s = Session::start(script).await;
    let folder = s.home.side().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    let mut chat = Cli::start(s.home.path(), &["chat"], &[]);
    chat.write(&format!(
        "/folder {}\n/mode manual\nMake a file.\n",
        folder.display()
    ))
    .await;
    chat.err_shows(
        "NEEDS YOU: [approval] mayor wants to edit `a.txt`",
    )
    .await;
    chat.write("/allow\n").await;
    chat.out_shows("Done.\n").await;
    assert_eq!(
        std::fs::read_to_string(folder.join("a.txt")).unwrap(),
        "one"
    );
    chat.write("/budget\n").await;
    chat.err_shows("Hamlet today:").await;
    assert_eq!(chat.err().matches("NEEDS YOU:").count(), 1);
    chat.write("/mode auto\nRun a long command.\n").await;
    chat.err_shows("[bash] {\"command\":\"echo started; sleep 30\"}")
        .await;
    chat.write("/stop\n").await;
    chat.err_shows("Stopped: stopped by chief").await;
    chat.write("/mode manual\nMake another file.\n").await;
    chat.err_shows(
        "NEEDS YOU: [approval] mayor wants to edit `b.txt`",
    )
    .await;
    s.chief
        .until("the wait for b.txt", |c| {
            let waits = c.waits();
            waits.len() == 1
                && realm::wait_says(&waits[0]) == "wants to edit"
                && format!("{waits:?}").contains("b.txt")
        })
        .await;
    let wait = s.chief.waits().remove(0);
    let Session {
        mut home,
        server,
        chief,
    } = s;
    drop(chief);
    server.kill().await;
    // The command ends when its server goes.
    assert_eq!(chat.wait().await.code(), Some(1));
    let again = home
        .serve(
            &Script::new().responses([words("After restart.")]),
            QUIET,
        )
        .await;
    let chief = Client::join(&again).await;
    assert_eq!(chief.waits(), [wait]);
    let mut next = Cli::start(home.path(), &["chat"], &[]);
    next.err_shows(
        "NEEDS YOU: [approval] mayor wants to edit `b.txt`",
    )
    .await;
    next.write("/allow\n").await;
    next.out_shows("After restart.\n").await;
    assert_eq!(next.end().await.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(folder.join("b.txt")).unwrap(),
        "after restart"
    );
}

/// A home whose mayor makes a scout and then says `home`, and a
/// mill whose mayor says `mill`.
fn two_hamlets(home: &[&str], mill: &[&str]) -> Script {
    let made = call(
        "create_agent",
        json!({"name": "scout", "purpose": "Scout", "reason": "Explore"}),
    );
    let home = [made, words("Scout made")]
        .into_iter()
        .chain(home.iter().map(|w| words(w)));
    Script::new()
        .agent("home:0", home.collect::<Vec<_>>())
        .agent(
            "mill:0",
            mill.iter().map(|w| words(w)).collect::<Vec<_>>(),
        )
}

async fn found_mill(s: &mut Session) {
    s.chief.say(mayor(), "Make a scout").await;
    s.chief
        .until("scout created", |c| {
            c.agent("home", "scout").is_some()
        })
        .await;
    s.chief.idle("home", "founder").await;
    s.chief.say(mayor(), "/found mill miller Grind flour").await;
}

#[tokio::test]
async fn starts_at_another_hamlet_and_switches_agents_and_hamlets() {
    let script = two_hamlets(
        &["Home answer."],
        &["Mill answer.", "Named answer."],
    );
    let mut s = Session::start(script).await;
    found_mill(&mut s).await;
    let home = s.home.path();
    let chat = Cli::run(
        home,
        &["chat", "--to", "mayor@mill"],
        "Mill?\n/hamlet home\nHome?\n",
        &[],
    )
    .await;
    assert_eq!(chat.code(), Some(0), "{chat:?}");
    for words in ["Mill answer.\n", "Home answer.\n"] {
        assert!(chat.out.contains(words), "{words}: {chat:?}");
    }
    assert!(
        chat.err.contains("talking to the mayor of home"),
        "{chat:?}"
    );
    let named =
        Cli::run(home, &["chat", "--to", "miller"], "Named?\n", &[])
            .await;
    assert!(named.out.contains("Named answer.\n"), "{named:?}");
    let switched = Cli::run(
        home,
        &["chat"],
        "/agent scout\n/agent miller\n/hamlet mill\n/agent missing\n",
        &[],
    )
    .await;
    assert_eq!(switched.code(), Some(0), "{switched:?}");
    for words in [
        "talking to the agent scout",
        "talking to the agent miller",
        "error: no agent named `missing`",
    ] {
        assert!(
            switched.err.contains(words),
            "{words}: {switched:?}"
        );
    }
    assert!(!switched.err.contains("error: no agent named `miller`"));
}

#[tokio::test]
async fn follows_the_realm_as_hamlets_close_and_come_back() {
    let script = two_hamlets(
        &["Home again"],
        &["Flour from mill", "Back in mill"],
    );
    let mut s = Session::start(script).await;
    found_mill(&mut s).await;
    let mut chat = Cli::start(
        s.home.path(),
        &["chat", "--to", "mayor@mill"],
        &[],
    );
    chat.write("How is the mill?\n").await;
    chat.out_shows("Flour from mill").await;
    chat.write("/hamlet home\nHello home\n").await;
    chat.out_shows("Home again").await;
    s.chief.say(mayor(), "/remove-hamlet mill Finished").await;
    chat.write("/hamlets\n").await;
    chat.err_shows("- home (`home`)").await;
    assert!(
        !chat.err().contains("- mill (`mill`)"),
        "{}",
        chat.err()
    );
    chat.write("/hamlet mill\n").await;
    chat.err_shows("error: no hamlet named `mill`").await;
    s.chief.say(mayor(), "/restore-hamlet mill").await;
    chat.write("/hamlets\n").await;
    chat.err_shows("- mill (`mill`)").await;
    chat.write("/hamlet mill\nWelcome back\n").await;
    chat.out_shows("Back in mill").await;
    assert_eq!(chat.end().await.code(), Some(0));
}

#[tokio::test]
async fn edits_limits_in_editor_and_reports_a_refusal() {
    let s = Session::start(Script::new()).await;
    let good = cost_editor(&s, "good", 5, 8);
    let saved = edit(&s, &["limits"], &good).await;
    assert_eq!(saved.code(), Some(0), "{saved:?}");
    assert!(saved.err.contains("Saved."), "{saved:?}");
    let seen =
        serde_json::from_str::<serde_json::Value>(&good.seen())
            .unwrap();
    let number = |v: &serde_json::Value| v.as_f64();
    assert_eq!(
        (number(&seen["budget"]["cost"]), &seen["budget"]["period"]),
        (Some(5.0), &json!("day"))
    );
    assert_eq!(
        (
            number(&seen["task"]["cost"]),
            number(&seen["task"]["messages"])
        ),
        (Some(1.0), Some(12.0))
    );
    let bad = cost_editor(&s, "bad", 8, -1);
    let refused = edit(&s, &["limits"], &bad).await;
    assert_eq!(refused.code(), Some(1), "{refused:?}");
    assert!(
        refused.err.contains(
            "budget.cost: a cost is a number of zero or more"
        ),
        "{refused:?}"
    );
    let report =
        Cli::run(s.home.path(), &["chat"], "/budget\n", &[]).await;
    assert!(
        report.err.contains("Hamlet today: $0.00 of $8.00"),
        "{report:?}"
    );
}

#[tokio::test]
async fn edits_whole_documents_in_editor_and_reads_them_back() {
    let s = Session::start(Script::new()).await;
    let text = format!("  {}\n\n", "x".repeat(7000));
    let file = s.home.side().join("text");
    std::fs::write(&file, &text).unwrap();
    let write = editor(
        &s,
        "write",
        &format!("cp '{}' \"$1\"", file.display()),
    );
    let look = editor(&s, "look", "");
    let first = edit(&s, &["notes"], &look).await;
    assert_eq!(first.code(), Some(0), "{first:?}");
    assert_eq!(look.seen(), "# Notes\n");
    for doc in ["notes", "charter", "character"] {
        assert_eq!(edit(&s, &[doc], &write).await.code(), Some(0));
        assert_eq!(edit(&s, &[doc], &look).await.code(), Some(0));
        assert_eq!(look.seen(), text, "{doc}");
    }
    let failing =
        editor(&s, "failing", "echo 'not saved' > \"$1\"; exit 2");
    let failed = edit(&s, &["notes"], &failing).await;
    assert_eq!(failed.code(), Some(1));
    assert!(
        failed
            .err
            .contains("The editor ended with 2. Nothing was saved."),
        "{failed:?}"
    );
    edit(&s, &["notes"], &look).await;
    assert_eq!(look.seen(), text);
}

#[tokio::test]
async fn edits_settings_as_whole_json_without_credentials() {
    let s = Session::start(Script::new()).await;
    let look = editor(&s, "look", "");
    edit(&s, &["settings"], &look).await;
    let seen =
        serde_json::from_str::<serde_json::Value>(&look.seen())
            .unwrap();
    assert_eq!(
        seen,
        json!({
            "model": "faux::faux-1",
            "reflectEvery": 0,
            "allowNet": false,
            "endpoints": [
                {"name": "local", "baseUrl": "http://localhost:11434/v1"}
            ],
        })
    );
    let wrong = editor(
        &s,
        "wrong",
        "sed 's/\"allowNet\"/\"allowNte\"/' \"$1\" > \"$1.next\" && \
         mv \"$1.next\" \"$1\"",
    );
    let refused = edit(&s, &["settings"], &wrong).await;
    assert_eq!(refused.code(), Some(1));
    assert!(refused.err.contains("nothing was saved"), "{refused:?}");
}

#[tokio::test]
async fn edits_another_hamlets_limits_alone() {
    let mut s = Session::start(two_hamlets(&[], &[])).await;
    found_mill(&mut s).await;
    let nine = cost_editor(&s, "nine", 5, 9);
    let saved =
        edit(&s, &["limits", "--hamlet", "mill"], &nine).await;
    assert_eq!(saved.code(), Some(0), "{saved:?}");
    s.chief
        .until("the mill's limits", |c| {
            let cost = |id: &str| {
                c.realm()
                    .hamlets
                    .into_iter()
                    .find(|h| h.id == id)
                    .and_then(|h| h.limits.budget.cost)
            };
            cost("mill") == Some(9.0) && cost("home") == Some(5.0)
        })
        .await;
}

#[tokio::test]
async fn says_plainly_when_the_model_tried_answers_no_hello() {
    let s =
        Session::start(Script::new().responses([words("")])).await;
    let tried = Cli::run(
        s.home.path(),
        &["models", "try", "faux::faux-1"],
        "",
        &[],
    )
    .await;
    assert_eq!(tried.code(), Some(0));
    assert_eq!(
        tried.out,
        "That model did not answer a hello. Pick another one. \
         (Error: the model answered with nothing)\n"
    );
    assert_eq!(tried.err, "");
}

#[tokio::test]
async fn signs_in_with_a_key_lists_models_and_tries_a_hello() {
    let endpoint = openai::Endpoint::start().await;
    let s = Session::with_endpoint(&endpoint).await;
    let key = "sk-terminal-private-key";
    let login = Cli::run(
        s.home.path(),
        &["login", "openai", "--key"],
        &format!("{key}\n"),
        &[],
    )
    .await;
    assert_eq!(login.code(), Some(0), "{login:?}");
    assert!(!login.out.contains(key) && !login.err.contains(key));
    assert!(login.err.contains("Signed in to openai."), "{login:?}");
    let stored =
        std::fs::read_to_string(s.home.file("credentials.json"))
            .unwrap();
    let stored =
        serde_json::from_str::<serde_json::Value>(&stored).unwrap();
    assert_eq!(stored["models"]["openai"]["key"], key);
    let models = Cli::run(s.home.path(), &["models"], "", &[]).await;
    assert_eq!(models.code(), Some(0));
    assert!(models.out.contains("local::org/large"), "{models:?}");
    let hello =
        Cli::run(s.home.path(), &["models", "try"], "", &[]).await;
    assert_eq!(hello.code(), Some(0));
    assert_eq!(hello.out, "Hello.\n");
}

#[tokio::test]
async fn reads_the_realm_it_is_given() {
    let mut s = Session::start(Script::new()).await;
    let chat =
        Cli::run(s.home.path(), &["chat"], "/queue\n", &[]).await;
    assert_eq!(chat.code(), Some(0));
    assert!(chat.err.contains("Nothing waits for you."), "{chat:?}");
    let reply = s.chief.say(mayor(), "/queue").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
}

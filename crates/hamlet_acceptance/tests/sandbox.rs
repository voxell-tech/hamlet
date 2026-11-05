//! Tool reach through real commands and file tools, with canaries on
//! disk: what a hire's commands and file tools can touch.

use std::fs;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{Step, openai};
use serde_json::json;

fn command(text: &str) -> Step {
    call("bash", json!({"command": text}))
}

#[tokio::test]
async fn confines_reads_writes_and_the_lodge_with_and_without_a_folder()
 {
    for open in [false, true] {
        let home = Home::new();
        let work = home.side().join("work");
        let secret = home.side().join("secret");
        fs::create_dir(&work).unwrap();
        fs::write(&secret, "outside-canary").unwrap();
        let lodge = home.file("hamlets/docs/0");
        let attempts = [
            (format!("cat '{}'", secret.display()), open),
            (
                format!(
                    "cat '{}/credentials.json'",
                    home.path().display()
                ),
                false,
            ),
            (format!("echo damaged > '{}'", secret.display()), false),
            ("echo damaged > escape".into(), false),
            (format!("echo own > '{}/own'", lodge.display()), true),
            (
                "echo temporary > \"$TMPDIR/note\"; test -s \
                 \"$TMPDIR/note\""
                    .into(),
                true,
            ),
        ];
        let script = attempts
            .iter()
            .map(|(cmd, _)| {
                format!(
                    "if ({cmd}) >/dev/null 2>&1; then echo yes; \
                     else echo no; fi"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let script = format!("{{ {script}\n}} > reach; pwd -P > cwd");
        let mut s = Session::start_in(
            home,
            Faux::new()
                .agent(2, [command(&script), words("Checked.")]),
            QUIET,
        )
        .await;
        let ada = s.found("docs", "ada", "keep the docs").await;
        while !lodge.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(20))
                .await;
        }
        let root = if open { &work } else { &lodge };
        std::os::unix::fs::symlink(&secret, root.join("escape"))
            .unwrap();
        if open {
            s.chief
                .command(ada, &format!("/folder {}", work.display()))
                .await;
            s.chief.command(ada, "/mode auto").await;
        }
        let now = chat(ada);
        s.chief.open(&now).await;
        s.chief.tell(ada, "Check reach.").await;
        s.chief.shows(&now, "Checked.").await;
        let got = fs::read_to_string(root.join("reach")).unwrap();
        assert_eq!(
            got.lines().collect::<Vec<_>>(),
            attempts
                .iter()
                .map(|(_, allowed)| if *allowed {
                    "yes"
                } else {
                    "no"
                })
                .collect::<Vec<_>>(),
            "with a folder: {open}"
        );
        assert_eq!(
            fs::read_to_string(root.join("cwd")).unwrap().trim(),
            root.to_str().unwrap()
        );
        assert_eq!(
            fs::read_to_string(&secret).unwrap(),
            "outside-canary"
        );
    }
}

#[tokio::test]
async fn the_file_tools_keep_to_the_lodge_and_never_write_a_character()
 {
    let home = Home::new();
    let lodge = home.file("hamlets/docs/0");
    let outside = home.side().join("outside");
    let mut s = Session::start_in(
        home,
        Faux::new().agent(
            2,
            [
                calls([
                    (
                        "write",
                        json!({"path": "CHARACTER.md", "content": "Cruel."}),
                    ),
                    (
                        "write",
                        json!({"path": outside, "content": "damaged"}),
                    ),
                    (
                        "write",
                        json!({"path": "memory/mine.md", "content": "---\ndescription: Mine\n---\nKept.\n"}),
                    ),
                ]),
                words("Tried."),
            ],
        ),
        QUIET,
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Write files.").await;
    s.chief.shows(&now, "Tried.").await;
    assert!(!lodge.join("CHARACTER.md").exists());
    assert!(!outside.exists());
    assert!(lodge.join("memory/mine.md").is_file());
    let failed = s
        .chief
        .items(&now)
        .into_iter()
        .filter_map(|i| i.tool)
        .map(|t| t.failure.is_some())
        .collect::<Vec<_>>();
    assert_eq!(failed, [true, true, false]);
}

#[tokio::test]
async fn reaches_a_listener_only_for_the_approved_network_call() {
    let endpoint = openai::Endpoint::start().await;
    let fetch = format!(
        "curl --noproxy '*' -sS --max-time 2 '{}/models' > fetched",
        endpoint.url
    );
    let home = Home::new();
    let lodge = home.file("hamlets/docs/0");
    let mut s = Session::start_in(
        home,
        Faux::new().agent(
            2,
            [
                command("sh fetch.sh"),
                words("Blocked."),
                call(
                    "bash",
                    json!({"command": "sh fetch.sh", "network": true}),
                ),
                words("Fetched."),
                command("sh fetch.sh"),
                words("Done."),
            ],
        ),
        QUIET,
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    while !lodge.is_dir() {
        tokio::time::sleep(std::time::Duration::from_millis(20))
            .await;
    }
    fs::write(lodge.join("fetch.sh"), fetch).unwrap();

    // Without leave the sandbox keeps the command off the network.
    s.chief.tell(ada, "Try without leave.").await;
    s.chief.shows(&now, "Blocked.").await;
    let first = s
        .chief
        .items(&now)
        .into_iter()
        .find_map(|i| i.tool)
        .unwrap();
    assert_ne!(first.exit, Some(0));
    assert_eq!(fs::read(lodge.join("fetched")).unwrap(), b"");

    // With the chief's leave it reaches the listener.
    s.chief.tell(ada, "Ask for leave.").await;
    let wait = s
        .chief
        .wait_for("network leave", |w| {
            matches!(
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Network,
                    ..
                }
            )
        })
        .await;
    s.chief.choose(&wait.id, Choice::Allow, None).await;
    s.chief.shows(&now, "Fetched.").await;
    assert!(
        fs::read_to_string(lodge.join("fetched"))
            .unwrap()
            .contains("org/large")
    );
    assert!(s.chief.realm().grants.is_empty());

    // The leave was for that call alone.
    s.chief.tell(ada, "Try without leave again.").await;
    s.chief.shows(&now, "Done.").await;
    assert_eq!(fs::read(lodge.join("fetched")).unwrap(), b"");
}

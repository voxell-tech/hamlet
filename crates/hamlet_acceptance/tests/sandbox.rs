//! Tool reach through real commands and file tools, with disk
//! canaries.

use std::fs;
use std::os::unix::fs::symlink;
use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

fn command(text: &str) -> Step {
    call("bash", json!({"command": text}))
}

#[tokio::test]
async fn confines_reads_writes_and_links_with_and_without_a_folder() {
    for open in [false, true] {
        let mut home = Home::new();
        let work = home.side().join("work");
        let secret = home.side().join("secret");
        fs::create_dir(&work).unwrap();
        fs::write(&secret, "outside-canary").unwrap();
        let house = home.file("houses/home/0");
        let library = home.file("library/probe/SKILL.md");
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
            (format!("cat '{}'", library.display()), true),
            (
                format!("echo damaged > '{}'", library.display()),
                false,
            ),
            ("echo damaged > escape".into(), false),
            (format!("echo own > '{}/own'", house.display()), true),
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
        let server = home
            .serve(
                &Script::new()
                    .responses([command(&script), words("Checked.")]),
                QUIET,
            )
            .await;
        fs::create_dir_all(library.parent().unwrap()).unwrap();
        fs::write(&library, "library-canary").unwrap();
        let root = if open { &work } else { &house };
        symlink(&secret, root.join("escape")).unwrap();
        let mut chief = Client::join(&server).await;
        if open {
            chief
                .say(mayor(), &format!("/folder {}", work.display()))
                .await;
            chief.say(mayor(), "/mode auto").await;
        }
        chief.say(mayor(), "Check reach.").await;
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
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fs::read_to_string(root.join("cwd")).unwrap().trim(),
            root.to_str().unwrap()
        );
        assert_eq!(
            fs::read_to_string(&secret).unwrap(),
            "outside-canary"
        );
        assert_eq!(
            fs::read_to_string(&library).unwrap(),
            "library-canary"
        );
    }
}

#[tokio::test]
async fn reaches_a_listener_only_for_the_approved_network_call() {
    let endpoint = openai::Endpoint::start().await;
    let fetch = format!(
        "curl --noproxy '*' -sS --max-time 2 '{}/models' > fetched",
        endpoint.url
    );
    let mut s = Session::start(Script::new().responses([
        command("sh fetch.sh"),
        words("Blocked."),
        call(
            "bash",
            json!({"command": "sh fetch.sh", "network": true}),
        ),
        words("Fetched."),
        command("sh fetch.sh"),
        words("Done."),
    ]))
    .await;
    fs::write(s.home.file("houses/home/0/fetch.sh"), fetch).unwrap();
    s.chief.say(mayor(), "Try without leave.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert_ne!(
        page.items
            .iter()
            .find_map(|i| i.tool.as_ref())
            .unwrap()
            .exit,
        Some(0)
    );
    assert_eq!(
        fs::read(s.home.file("houses/home/0/fetched")).unwrap(),
        b""
    );
    s.chief.tell(mayor(), "Ask for leave.").await;
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
    s.chief.shows(&chat(mayor()), "Fetched.").await;
    assert!(
        fs::read_to_string(s.home.file("houses/home/0/fetched"))
            .unwrap()
            .contains("org/large")
    );
    assert!(s.chief.realm().grants.is_empty());
    s.chief.say(mayor(), "Try without leave again.").await;
    assert_eq!(
        fs::read(s.home.file("houses/home/0/fetched")).unwrap(),
        b""
    );
}

#[tokio::test]
async fn keeps_notes_and_skills_in_the_house_and_git_files_in_the_folder()
 {
    let mut s = Session::start(Script::new().responses([
        calls([
            ("write", json!({"path":"NOTES.md", "content":"house notes"})),
            ("write", json!({"path":"skills/probe/SKILL.md", "content":"---\nname: probe\n---\n"})),
            ("write", json!({"path":"project.txt", "content":"project"})),
            ("edit", json!({"path":"binary", "oldText":"A", "newText":"B"})),
            ("bash", json!({"command":"mkdir -p .git/hooks nested/.git/hooks; echo hook > .git/hooks/pre-commit; echo config > nested/.git/config"})),
        ]), words("Placed."),
    ])).await;
    let work = s.home.side().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("binary"), [255, 254, 65]).unwrap();
    s.chief
        .say(mayor(), &format!("/folder {}", work.display()))
        .await;
    s.chief.say(mayor(), "/mode auto").await;
    s.chief.say(mayor(), "Place files.").await;
    assert_eq!(
        fs::read_to_string(s.home.file("houses/home/0/NOTES.md"))
            .unwrap(),
        "house notes"
    );
    assert!(
        s.home.file("houses/home/0/skills/probe/SKILL.md").is_file()
    );
    assert!(!work.join("NOTES.md").exists());
    assert!(!work.join("skills").exists());
    assert_eq!(
        fs::read_to_string(work.join("project.txt")).unwrap(),
        "project"
    );
    assert_eq!(
        fs::read(work.join("binary")).unwrap(),
        [255, 254, 65]
    );
    assert_eq!(
        fs::read_to_string(work.join(".git/hooks/pre-commit"))
            .unwrap(),
        "hook\n"
    );
    assert_eq!(
        fs::read_to_string(work.join("nested/.git/config")).unwrap(),
        "config\n"
    );
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items
            .iter()
            .filter_map(|i| i.tool.as_ref())
            .any(|t| t.name == "edit" && t.failure.is_some())
    );
}

#[tokio::test]
async fn refuses_a_work_root_replaced_by_a_link_or_removed() {
    for linked in [true, false] {
        let mut s = Session::start(Script::new().responses([
            command("echo ready > ready"),
            words("Ready."),
            calls([
                (
                    "write",
                    json!({"path":"canary", "content":"damaged"}),
                ),
                ("bash", json!({"command":"echo damaged > canary"})),
            ]),
            words("Refused."),
        ]))
        .await;
        let work = s.home.side().join("work");
        let outside = s.home.side().join("outside");
        fs::create_dir(&work).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("canary"), "private").unwrap();
        s.chief
            .say(mayor(), &format!("/folder {}", work.display()))
            .await;
        s.chief.say(mayor(), "/mode auto").await;
        s.chief.say(mayor(), "Prepare.").await;
        fs::rename(&work, s.home.side().join("old")).unwrap();
        if linked {
            symlink(&outside, &work).unwrap();
        }
        s.chief.say(mayor(), "Try again.").await;
        let page = s.chief.open(&chat(mayor())).await;
        let tools = page
            .items
            .iter()
            .filter_map(|i| i.tool.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(tools.len(), 3);
        assert!(
            tools[1..]
                .iter()
                .all(|t| t.exit != Some(0) && t.failure.is_some()),
            "{tools:?}"
        );
        assert_eq!(
            fs::read_to_string(outside.join("canary")).unwrap(),
            "private"
        );
        assert!(!s.home.side().join("old/canary").exists());
    }
}

#[tokio::test]
async fn refuses_a_linked_houses_root_at_startup() {
    let mut home = Home::new();
    let server = home.serve(&Script::new(), QUIET).await;
    assert_eq!(server.stop().await, Some(0));
    let outside = home.side().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), "private").unwrap();
    fs::rename(home.file("houses/home"), home.file("houses/old"))
        .unwrap();
    symlink(&outside, home.file("houses/home")).unwrap();
    home.try_serve(&Script::new(), QUIET).await;
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(outside.join("secret")).unwrap(),
        "private"
    );
}

#[tokio::test]
async fn bounds_commands_and_kills_timed_out_children() {
    let mut s = Session::start(Script::new().responses([
        command("ulimit -t > limits; ulimit -u >> limits; ulimit -f >> limits; ulimit -c >> limits"),
        command("kill -9 $$"),
        call("bash", json!({"command":"(sleep 2; echo escaped > late) & wait", "timeout":1})),
        words("Finished."),
    ])).await;
    s.chief.say(mayor(), "Run bounded commands.").await;
    let page = s.chief.open(&chat(mayor())).await;
    let tools = page
        .items
        .iter()
        .filter_map(|i| i.tool.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(tools[1].exit, Some(137));
    assert_ne!(tools[2].exit, Some(0));
    assert!(
        tools[2].failure.as_ref().unwrap().contains("timed out"),
        "{tools:?}"
    );
    let limits =
        fs::read_to_string(s.home.file("houses/home/0/limits"))
            .unwrap();
    let limits = limits
        .lines()
        .map(|n| n.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(limits[0], 60);
    assert!(limits[1] <= 4096);
    assert_eq!(&limits[2..], &[524288, 0]);
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert!(!s.home.file("houses/home/0/late").exists());
}

fn manifest(home: &Home, folder: &str, value: serde_json::Value) {
    let folder = home.file(&format!("tools/{folder}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("manifest.json"), value.to_string())
        .unwrap();
}

#[tokio::test]
async fn loads_hand_tools_checks_arguments_and_confines_their_reach()
{
    let mut home = Home::new();
    let output = home.side().join("output");
    fs::create_dir(&output).unwrap();
    manifest(
        &home,
        "checked",
        json!({"name":"checked", "exec":["bash","-c",format!("cat > '{}/args'; pwd > '{}/cwd'; if touch changed; then echo writable; else echo readonly; fi > '{}/reach'", output.display(), output.display(), output.display())],
        "parameters":{"type":"object","properties":{"text":{"type":"string","minLength":2}},"required":["text"],"additionalProperties":false},
        "sandbox":{"fs_rw":[output, home.file("tools/checked")]}}),
    );
    manifest(
        &home,
        "reserved",
        json!({"name":"bash", "exec":["false"]}),
    );
    let server = home
        .serve(
            &Script::new().responses([
                call("checked", json!({"text":"x"})),
                words("Rejected."),
                call("checked", json!({"text":"valid"})),
                words("Accepted."),
            ]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    let prompt = chief.prompt(mayor()).await;
    assert!(prompt.contains("name 'bash' is reserved"), "{prompt}");
    chief.say(mayor(), "Bad arguments.").await;
    assert!(!output.join("args").exists());
    chief.say(mayor(), "Good arguments.").await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(output.join("args")).unwrap()
        )
        .unwrap(),
        json!({"text":"valid"})
    );
    assert_eq!(
        fs::read_to_string(output.join("cwd")).unwrap().trim(),
        home.file("tools/checked").to_str().unwrap()
    );
    assert_eq!(
        fs::read_to_string(output.join("reach")).unwrap(),
        "readonly\n"
    );
    let page = chief.open(&chat(mayor())).await;
    let tools = page
        .items
        .iter()
        .filter_map(|i| i.tool.as_ref())
        .collect::<Vec<_>>();
    assert!(
        tools[0]
            .failure
            .as_ref()
            .unwrap()
            .contains("Validation failed")
    );
    assert_eq!(tools[1].exit, Some(0));
}

#[tokio::test]
async fn reloads_hand_additions_edits_and_removals() {
    let mut s = Session::start(Script::new().responses([
        call("mark", json!({})),
        words("First."),
        call("mark", json!({})),
        words("Second."),
        call("mark", json!({})),
        words("Gone."),
    ]))
    .await;
    let output = s.home.side().join("output");
    fs::create_dir(&output).unwrap();
    for version in ["one", "two"] {
        manifest(
            &s.home,
            "mark",
            json!({"name":"mark", "exec":["sh","-c",format!("echo {version} > '{}/version'", output.display())], "sandbox":{"fs_rw":[output]}}),
        );
        tokio::time::sleep(Duration::from_millis(700)).await;
        s.chief.say(mayor(), "Mark.").await;
        assert_eq!(
            fs::read_to_string(output.join("version")).unwrap(),
            format!("{version}\n")
        );
    }
    fs::remove_dir_all(s.home.file("tools/mark")).unwrap();
    fs::remove_file(output.join("version")).unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    s.chief.say(mayor(), "Mark again.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items
            .iter()
            .filter_map(|i| i.tool.as_ref())
            .any(|t| t.name == "mark" && t.failure.is_some()),
        "{page:?}"
    );
    assert!(!output.join("version").exists());
}

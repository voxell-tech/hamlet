//! The tools under the platform sandbox, on scratch houses.

use std::fs;
use std::time::{Duration, Instant};

use hamlet_testing::Scratch;
use hamlet_testing::links::symlink;
use serde_json::{Value, json};

use super::{Outcome, Place, ToolSession};
use crate::sandbox::Sandbox;

/// A house of its own in `scratch`, with no folder.
fn house(scratch: &Scratch) -> Place {
    let house = scratch.join("houses/home/0");
    fs::create_dir_all(&house).unwrap();
    Place {
        house,
        folder: None,
        home: scratch.to_path_buf(),
        shared: Vec::new(),
    }
}

async fn call(
    place: &Place,
    net: bool,
    tool: &str,
    args: Value,
) -> Outcome {
    let secrets = ["sk-test-secret-42".to_owned()];
    let Value::Object(args) = args else {
        panic!("arguments are an object");
    };
    ToolSession::local(place.clone(), net, Sandbox::platform())
        .run(&secrets, tool, &args)
        .await
}

#[tokio::test]
async fn hand_tools_keep_declared_limits_environment_and_private_scratch()
 {
    let scratch = Scratch::new("hand-policy");
    let place = house(&scratch);
    let dir = scratch.join("tools/check");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("data"), "declared\n").unwrap();
    let manifest = |command: &str| {
        json!({
            "name":"check", "exec":["bash", "-c", command],
            "sandbox":{"env":{"CHOICE":"kept", "IGNORED":42},"limits":{"wall_secs":1,"cpu_secs":2}}
        })
    };
    fs::write(
        dir.join("manifest.json"),
        manifest(
            "cat data; echo \"$CHOICE:$IGNORED\"; ulimit -t; printf \
             sk-test-; sleep 0.1; printf 'secret-42\\n'; echo \
             \"$TMPDIR\"",
        )
        .to_string(),
    )
    .unwrap();
    let (a, b) = tokio::join!(
        call(&place, false, "check", json!({})),
        call(&place, false, "check", json!({}))
    );
    assert!(!a.error && !b.error, "{a:?} {b:?}");
    let prefix = "declared\nkept:\n2\n[REDACTED]\n";
    let a_tmp = a.text.strip_prefix(prefix).unwrap().trim();
    let b_tmp = b.text.strip_prefix(prefix).unwrap().trim();
    assert_ne!(a_tmp, b_tmp);
    assert!(!std::path::Path::new(a_tmp).exists());
    assert!(!std::path::Path::new(b_tmp).exists());
    fs::write(
        dir.join("manifest.json"),
        manifest("sleep 3; echo should-not-run").to_string(),
    )
    .unwrap();
    let timed = call(&place, false, "check", json!({})).await;
    assert!(timed.error);
    assert_eq!(timed.text, "\n[timed out after 1s]");
}

#[tokio::test]
async fn a_hand_tool_gets_its_argv_as_written_and_its_arguments_on_stdin()
 {
    let scratch = Scratch::new("hand-argv");
    let place = house(&scratch);
    let dir = scratch.join("tools/echo");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("manifest.json"),
        json!({
            "name":"echo",
            "exec":["bash", "-c", "printf '%s\\n' \"$1\"; cat", "bash", "it's \"quoted\" $HOME"],
            "parameters":{"type":"object","properties":{"say":{"type":"string"}}}
        })
        .to_string(),
    )
    .unwrap();
    let said =
        call(&place, false, "echo", json!({"say": "it's"})).await;
    assert!(!said.error, "{said:?}");
    assert_eq!(
        said.text,
        "it's \"quoted\" $HOME\n{\"say\":\"it's\"}"
    );
}

#[tokio::test]
async fn a_command_writes_only_in_its_house_and_reaches_the_network_only_with_leave()
 {
    let scratch = Scratch::new("tools-confined");
    let place = house(&scratch);
    let elsewhere = scratch.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();

    let own = call(
        &place,
        false,
        "bash",
        json!({"command": "echo kept > note.txt; pwd -P"}),
    )
    .await;
    let outside = format!("echo x > {}/x", elsewhere.display());
    let away =
        call(&place, false, "bash", json!({"command": outside}))
            .await;
    let refused = call(
        &place,
        false,
        "bash",
        json!({"command": "touch /usr/hamlet-outside"}),
    )
    .await;

    assert_eq!(own.text, format!("{}\n", place.house.display()));
    assert!(!own.error);
    assert_eq!(
        fs::read_to_string(place.house.join("note.txt")).unwrap(),
        "kept\n"
    );
    assert!(refused.error);
    assert!(refused.text.ends_with(
        "\n[The sandbox lets a command write only inside the \
         workspace, which is its working directory. Use a relative \
         path there, not /tmp or another folder.]"
    ));
    assert_eq!(
        refused.diagnostics[0].message,
        "Command exited with code 1"
    );
    assert_eq!(refused.details.unwrap()["exitCode"], json!(1));
    assert!(away.error);
    assert!(!elsewhere.join("x").exists());

    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let reach = json!({
        "command": format!("echo > /dev/tcp/127.0.0.1/{port} && echo reached"),
    });
    let closed = call(&place, false, "bash", reach.clone()).await;
    let open = call(&place, true, "bash", reach).await;
    assert!(closed.error);
    assert!(!closed.text.contains("reached"));
    assert_eq!(open.text, "reached\n");
}

/// The size of `path`, which a command left running would grow.
fn size(path: &std::path::Path) -> u64 {
    fs::metadata(path).map_or(0, |meta| meta.len())
}

const TICKING: &str = "(while true; do echo x >> tick.txt; sleep \
                       0.05; done) & echo started; sleep 30";

#[tokio::test]
async fn a_timeout_stops_the_command_and_all_it_started() {
    let scratch = Scratch::new("tools-timeout");
    let place = house(&scratch);
    let started = Instant::now();

    let stopped = call(
        &place,
        false,
        "bash",
        json!({"command": TICKING, "timeout": 1}),
    )
    .await;

    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(stopped.text, "started\n\n[timed out after 1s]");
    assert!(stopped.error);
    assert_eq!(stopped.details.unwrap()["exitCode"], json!(-1));
    let tick = place.house.join("tick.txt");
    let before = size(&tick);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(size(&tick), before);
}

#[tokio::test]
async fn a_dropped_call_stops_the_command_and_all_it_started() {
    let scratch = Scratch::new("tools-dropped");
    let place = house(&scratch);
    let running = tokio::spawn({
        let place = place.clone();
        async move {
            call(&place, false, "bash", json!({"command": TICKING}))
                .await
        }
    });
    let tick = place.house.join("tick.txt");
    if tokio::time::timeout(Duration::from_secs(10), async {
        while size(&tick) == 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_err()
    {
        running.abort();
        panic!(
            "the command did not write tick.txt: {:?}",
            running.await
        );
    }

    running.abort();
    let _ = running.await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = size(&tick);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(size(&tick), before);
}

#[tokio::test]
async fn keeps_a_credential_out_of_kept_and_spilled_output_even_split_across_chunks()
 {
    let scratch = Scratch::new("tools-spill");
    let place = house(&scratch);
    let command = "seq 1 3000; printf 'key sk-test-'; sleep 0.2; \
                   printf 'secret-42\\n'";

    let ran =
        call(&place, false, "bash", json!({"command": command}))
            .await;

    assert!(!ran.error);
    assert!(ran.text.starts_with("1002\n"));
    assert!(ran.text.ends_with("3000\nkey [REDACTED]\n"));
    let spilled = ran.diagnostics[0]
        .message
        .strip_prefix("Full output: ")
        .unwrap()
        .to_owned();
    let full = fs::read_to_string(&spilled).unwrap();
    assert!(full.starts_with("1\n2\n3\n"));
    assert!(full.ends_with("3000\nkey [REDACTED]\n"));
    let read = call(
        &place,
        false,
        "read",
        json!({"path": spilled, "offset": 3000}),
    )
    .await;
    assert_eq!(read.text, "3000\nkey [REDACTED]\n");
}

#[tokio::test]
async fn reads_a_long_file_in_parts_saying_where_to_go_on() {
    let scratch = Scratch::new("tools-read");
    let place = house(&scratch);
    let lines = (1..=2500).map(|n| n.to_string()).collect::<Vec<_>>();
    fs::write(place.house.join("long.txt"), lines.join("\n"))
        .unwrap();
    let read = |args: Value| call(&place, false, "read", args);

    let head = read(json!({"path": "long.txt"})).await;
    let some =
        read(json!({"path": "long.txt", "offset": 10, "limit": 2}))
            .await;
    let past =
        read(json!({"path": "long.txt", "offset": 2600})).await;

    assert!(head.text.ends_with("\n2000"));
    assert_eq!(
        head.diagnostics[0].message,
        "Showing lines 1-2000 of 2500. Use offset=2001 to continue."
    );
    assert_eq!(some.text, "10\n11");
    assert_eq!(
        some.diagnostics[0].message,
        "2489 more lines in file. Use offset=12 to continue."
    );
    assert!(past.error);
    assert_eq!(
        past.diagnostics[0].message,
        "Offset 2600 is beyond end of file (2500 lines total)"
    );
}

#[tokio::test]
async fn writes_and_edits_nothing_out_of_reach() {
    let scratch = Scratch::new("tools-reach");
    let place = house(&scratch);
    let elsewhere = scratch.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(scratch.join("houses/home/1")).unwrap();
    symlink(&elsewhere, place.house.join("out")).unwrap();
    let write = |path: String| {
        call(
            &place,
            false,
            "write",
            json!({"path": path, "content": "x"}),
        )
    };

    let up = write("../1/NOTES.md".into()).await;
    let full = write(elsewhere.join("x").display().to_string()).await;
    let linked = write("out/sub/x".into()).await;

    for refused in [&up, &full, &linked] {
        assert!(refused.error);
    }
    assert_eq!(
        full.diagnostics[0].message,
        format!(
            "{}/x is out of this agent's reach to read",
            elsewhere.display()
        )
    );
    assert!(!scratch.join("houses/home/1/NOTES.md").exists());
    assert!(!elsewhere.join("x").exists());
    assert!(!elsewhere.join("sub").exists());

    let notes = place.house.join("NOTES.md");
    fs::write(&notes, "# Notes\n").unwrap();
    let edit = |args: Value| call(&place, false, "edit", args);
    let edited = edit(json!({
        "path": "NOTES.md",
        "edits": [{"oldText": "# Notes", "newText": "# Notes\nTea."}],
    }))
    .await;
    fs::write(place.house.join("raw.txt"), [0x61, 0xff, 0x0a])
        .unwrap();
    let raw = edit(json!({
        "path": "raw.txt",
        "edits": [{"oldText": "a", "newText": "A"}],
    }))
    .await;

    assert_eq!(
        edited.text,
        format!(
            "Successfully replaced 1 block(s) in {}.",
            notes.display()
        )
    );
    assert_eq!(
        fs::read_to_string(&notes).unwrap(),
        "# Notes\nTea.\n"
    );
    assert_eq!(
        raw.diagnostics[0].message,
        format!(
            "Could not edit file: {}. Error code: invalid.",
            place.house.join("raw.txt").display()
        )
    );
    assert_eq!(
        fs::read(place.house.join("raw.txt")).unwrap(),
        [0x61, 0xff, 0x0a]
    );
}

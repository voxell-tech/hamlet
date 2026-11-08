//! Files on a line and in a letter: uploaded in parts, saved in the
//! receiver's inbox, and over the cap held for a chief.

use std::fs;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::words;
use serde_json::json;

fn attached(kept: &Reply, name: &str, size: usize) -> AttachedFile {
    AttachedFile {
        id: kept.file.clone().expect("the last part names it"),
        name: name.into(),
        size: size as u64,
    }
}

fn lane(from: AgentId, with: AgentId) -> ChatRef {
    ChatRef {
        from: Some(from),
        kind: Some(ChatRefKind::Letters),
        ..chat(with)
    }
}

#[tokio::test]
async fn files_on_a_line_are_saved_in_the_inbox_and_named_in_its_item()
 {
    let mut s = Session::start(
        Faux::new().responses([words("Read."), words("Again.")]),
    )
    .await;
    let hand = s.hand();
    let inbox = s.home.file("hand/inbox");
    let now = chat(hand);
    s.chief.open(&now).await;

    // Two parts, so the upload is whole only at the last.
    let notes = b"first line\nsecond line\n";
    let kept = s.chief.put_file(notes, 10).await;
    let notes_file = attached(&kept, "notes/day.txt", notes.len());
    let empty = s.chief.put_file(b"", 10).await;
    let empty_file = attached(&empty, "notes/empty", 0);

    let said = s
        .chief
        .say_files(
            hand,
            "Look at these.",
            vec![notes_file.clone(), empty_file.clone()],
        )
        .await;
    assert_eq!(said.kind, ReplyKind::Notice, "{}", said.text);
    s.chief.shows(&now, "Read.").await;
    let items = s.chief.items(&now);
    assert_eq!(items[0].files, [notes_file.clone(), empty_file]);
    assert_eq!(fs::read(inbox.join("notes/day.txt")).unwrap(), notes);
    assert!(inbox.join("notes/empty").is_file());

    // The same folder sent again is kept beside the first.
    let said = s
        .chief
        .say_files(hand, "And again.", vec![notes_file])
        .await;
    assert_eq!(said.kind, ReplyKind::Notice, "{}", said.text);
    s.chief.shows(&now, "Again.").await;
    assert_eq!(
        fs::read(inbox.join("notes-2/day.txt")).unwrap(),
        notes
    );

    let unsent = AttachedFile {
        id: format!("{:0>64}", "a").parse().unwrap(),
        name: "ghost.txt".into(),
        size: 1,
    };
    let refused =
        s.chief.say_files(hand, "Nope.", vec![unsent]).await;
    assert_eq!(refused.kind, ReplyKind::Error);
}

#[tokio::test]
async fn a_letter_copies_the_files_it_names_into_the_receivers_inbox()
{
    let home = Home::new();
    let secret = home.side().join("secret");
    fs::write(&secret, "canary").unwrap();
    let mut s = Session::start_in(
        home,
        Faux::new()
            .agent(
                2,
                [
                    call(
                        "write",
                        json!({"path": "out/report.txt", "content": "findings"}),
                    ),
                    call(
                        "tell",
                        json!({"to": "hire#3", "text": "See attached.", "paths": ["out"]}),
                    ),
                    call(
                        "tell",
                        json!({"to": "hire#3", "text": "Escape.", "paths": [secret]}),
                    ),
                    words("Sent."),
                ],
            )
            .agent(3, [words("Noted.")]),
        QUIET,
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let cy = s.hire(ada, "cy", "dig").await;
    let now = chat(ada);
    s.chief.open(&now).await;

    s.chief.tell(ada, "Send cy the report.").await;
    s.chief.shows(&now, "Sent.").await;
    let inbox = s.home.file(&format!(
        "hamlets/{}/3/inbox/from-hire-2",
        s.chief.hamlet_id("docs")
    ));
    assert_eq!(
        fs::read_to_string(inbox.join("out/report.txt")).unwrap(),
        "findings"
    );
    let letters = lane(ada, cy);
    let page = s.chief.open(&letters).await;
    let first = &page.items[0].text;
    assert!(
        first.starts_with("See attached.")
            && first.contains("[Folder 1 of this message, 1 files"),
        "{first}"
    );
    // A file outside what the sender may read is refused.
    let tools = s
        .chief
        .items(&now)
        .into_iter()
        .filter_map(|item| item.tool)
        .map(|tool| tool.failure.is_some())
        .collect::<Vec<_>>();
    assert_eq!(tools, [false, false, true]);
    assert!(!inbox.join("secret").exists());
}

#[tokio::test]
async fn a_letter_over_the_cap_waits_for_a_chief_and_a_refusal_is_told()
 {
    let make = "mkdir big; for i in $(seq 1001); do : > big/$i; done";
    let mut s = Session::start(
        Faux::new()
            .agent(
                2,
                [
                    call("bash", json!({"command": make})),
                    call(
                        "tell",
                        json!({"to": "hire#3", "text": "Everything.", "paths": ["big"]}),
                    ),
                    words("Sent."),
                    call(
                        "tell",
                        json!({"to": "hire#3", "text": "Again.", "paths": ["big"]}),
                    ),
                    words("Refused."),
                ],
            )
            .agent(3, [words("Noted.")]),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.hire(ada, "cy", "dig").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    let inbox = format!(
        "hamlets/{}/3/inbox/from-hire-2/big",
        s.chief.hamlet_id("docs")
    );

    s.chief.tell(ada, "Send cy everything.").await;
    let ask = s
        .chief
        .wait_for("the send", |w| w.subject.contains("1001 files"))
        .await;
    let WaitDetail::Approval { danger, always, .. } = &ask.detail
    else {
        panic!("an approval: {ask:?}");
    };
    assert_eq!(*danger, WaitDanger::Send);
    assert_eq!(*always, None);
    assert!(!s.home.file(&inbox).exists());
    s.chief.choose(&ask.id, Choice::Allow, None).await;
    s.chief.shows(&now, "Sent.").await;
    assert_eq!(
        fs::read_dir(s.home.file(&inbox)).unwrap().count(),
        1001
    );

    s.chief.tell(ada, "Once more.").await;
    let ask = s
        .chief
        .wait_for("the second send", |w| w.subject.contains("Again."))
        .await;
    s.chief
        .choose(&ask.id, Choice::Deny, Some("too much"))
        .await;
    s.chief.shows(&now, "Refused.").await;
    let refusals = s
        .chief
        .items(&now)
        .into_iter()
        .filter_map(|item| item.tool?.failure)
        .collect::<Vec<_>>();
    assert!(
        refusals.iter().any(|words| words.contains("too much")),
        "{refusals:?}"
    );
    assert!(!s.home.file(&format!("{inbox}-2")).exists());
}

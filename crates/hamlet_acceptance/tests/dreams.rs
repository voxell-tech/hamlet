//! Dreams: a hire brings what it learned home to its scholar's
//! `main`, and a scholar tidies it.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{call, words};
use serde_json::json;

fn note(description: &str) -> String {
    format!("---\ndescription: {description}\n---\nA fact.\n")
}

#[tokio::test]
async fn a_hire_dreams_what_it_learned_onto_its_scholars_main() {
    let home = Home::new();
    // The first dream found nothing; this is the second, whose
    // checkout of `main` the hire is told of.
    let shared = home.file("dreams/2/main/memory/shared.md");
    let mut s = Session::start_in(
        home,
        Faux::new().kind(
            "dream",
            [
                call(
                    "write",
                    json!({
                        "path": shared.to_str().unwrap(),
                        "content": note("What every hire knows."),
                    }),
                ),
                words("Brought one note home."),
                words("Nothing to tidy."),
            ],
        ),
        QUIET,
    )
    .await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    let scholar = s.chief.scholar("ada").unwrap().id;

    // A hire that has learned nothing has nothing to dream about.
    assert_eq!(
        s.chief.command(hand, "/dream docs#0").await,
        "There is nothing to dream about."
    );

    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(ada),
                path: "own.md".into(),
                text: note("Only this hire's."),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    assert!(
        s.chief
            .papers(Keeper::Scholar(scholar))
            .await
            .memory
            .is_empty()
    );

    assert_eq!(
        s.chief.command(hand, "/dream docs#0").await,
        "The dream began."
    );
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while s
            .chief
            .papers(Keeper::Scholar(scholar))
            .await
            .memory
            .is_empty()
        {
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    })
    .await
    .expect("the dream lands on the scholar's main");
    let papers = s.chief.papers(Keeper::Scholar(scholar)).await;
    assert_eq!(papers.memory.len(), 1);
    assert_eq!(papers.memory[0].path, "shared.md");
    assert_eq!(
        papers.memory[0].description,
        "What every hire knows."
    );
    // What only the hire needs stays in its lodge.
    assert!(
        papers.memory.iter().all(|m| m.path != "own.md"),
        "{papers:?}"
    );
}

#[tokio::test]
async fn refuses_a_dream_of_someone_unknown() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    s.chief.refused(hand, "/dream").await;
    s.chief.refused(hand, "/dream scholar#999").await;
}

#[tokio::test]
async fn a_scholars_dream_tidies_its_main_from_its_checkout() {
    let mut s = Session::start(Faux::new().kind(
        "dream",
        [
            call(
                "write",
                json!({
                    "path": "memory/tidy.md",
                    "content": note("Tidied."),
                }),
            ),
            words("Kept one note."),
        ],
    ))
    .await;
    let hand = s.hand();
    s.found("docs", "ada", "keep the docs").await;
    let scholar = s.chief.scholar("ada").unwrap().id;
    assert_eq!(
        s.chief
            .command(hand, &format!("/dream scholar#{}", scholar.0))
            .await,
        "The dream began."
    );
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while s
            .chief
            .papers(Keeper::Scholar(scholar))
            .await
            .memory
            .is_empty()
        {
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    })
    .await
    .expect("the dream lands on the scholar's main");
    let papers = s.chief.papers(Keeper::Scholar(scholar)).await;
    assert_eq!(papers.memory[0].path, "tidy.md");
}

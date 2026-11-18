//! Dreams: an agent brings what it learned home to its profile's
//! `main`, and a profile tidies it.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{call, words};
use serde_json::json;

fn note(description: &str) -> String {
    format!("---\ndescription: {description}\n---\nA fact.\n")
}

#[tokio::test]
async fn an_agent_dreams_what_it_learned_onto_its_profiles_main() {
    let home = Home::new();
    // The first dream found nothing; this is the second, whose
    // checkout of `main` the agent is told of.
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
                        "content": note("What every agent knows."),
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
    let profile = s.chief.profile("ada").unwrap().id;

    // An agent that has learned nothing has nothing to dream about.
    assert_eq!(
        s.chief.command(hand, "/dream agent#2").await,
        "There is nothing to dream about."
    );

    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(ada),
                path: "own.md".into(),
                text: note("Only this agent's."),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    assert!(
        s.chief
            .papers(Keeper::Profile(profile))
            .await
            .memory
            .is_empty()
    );

    assert_eq!(
        s.chief.command(hand, "/dream agent#2").await,
        "The dream began."
    );
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while s
            .chief
            .papers(Keeper::Profile(profile))
            .await
            .memory
            .is_empty()
        {
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    })
    .await
    .expect("the dream lands on the profile's main");
    let papers = s.chief.papers(Keeper::Profile(profile)).await;
    assert_eq!(papers.memory.len(), 1);
    assert_eq!(papers.memory[0].path, "shared.md");
    assert_eq!(
        papers.memory[0].description,
        "What every agent knows."
    );
    // What only the agent needs stays in its worktree.
    assert!(
        papers.memory.iter().all(|m| m.path != "own.md"),
        "{papers:?}"
    );
}

#[tokio::test]
async fn an_agent_ended_with_a_dream_reflects_first_and_brings_that_home()
 {
    let home = Home::new();
    let kept = home.file("hamlets/{{docs}}/3/memory/build.md");
    let shared = home.file("dreams/1/main/memory/build.md");
    let mut s = Session::start_in(
        home,
        Faux::new().agent(
            3,
            [
                words("The docs build with make."),
                call(
                    "write",
                    json!({
                        "path": kept.to_str().unwrap(),
                        "content": note("How the docs build."),
                    }),
                )
                .filled_from(NAMES),
                words("Kept one note."),
                call(
                    "write",
                    json!({
                        "path": shared.to_str().unwrap(),
                        "content": note("How the docs build."),
                    }),
                ),
                words("Brought one note home."),
            ],
        ),
        QUIET,
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.add_agent(ada, "bob", "build the docs").await;
    let profile = s.chief.profile("bob").unwrap().id;
    let now = chat(bob);
    s.chief.open(&now).await;
    s.chief.tell(bob, "how do the docs build?").await;
    s.chief.shows(&now, "The docs build with make.").await;

    // What it learned is only in its conversation until it reflects.
    s.chief.command(ada, "/end agent#3 dream done").await;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while s
            .chief
            .papers(Keeper::Profile(profile))
            .await
            .memory
            .is_empty()
        {
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    })
    .await
    .expect("the dream lands on the profile's main");
    let papers = s.chief.papers(Keeper::Profile(profile)).await;
    assert_eq!(papers.memory[0].path, "build.md");
}

#[tokio::test]
async fn rejects_a_dream_of_someone_unknown() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    s.chief.rejected(hand, "/dream").await;
    s.chief.rejected(hand, "/dream profile#999").await;
}

#[tokio::test]
async fn a_profiles_dream_tidies_its_main_from_its_checkout() {
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
    let profile = s.chief.profile("ada").unwrap().id;
    assert_eq!(
        s.chief
            .command(hand, &format!("/dream profile#{}", profile.0))
            .await,
        "The dream began."
    );
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while s
            .chief
            .papers(Keeper::Profile(profile))
            .await
            .memory
            .is_empty()
        {
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    })
    .await
    .expect("the dream lands on the profile's main");
    let papers = s.chief.papers(Keeper::Profile(profile)).await;
    assert_eq!(papers.memory[0].path, "tidy.md");
}

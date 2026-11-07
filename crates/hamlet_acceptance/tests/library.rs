//! The shared library: a skill offered to the librarian and judged,
//! a question to the librarian, and a chief's `/unshelve`.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{call, words};
use serde_json::json;

fn write_skill(name: &str) -> hamlet_testing::Step {
    call(
        "bash",
        json!({"command": format!(
            "mkdir -p skills/{name} && printf -- '---\\nname: {name}\\n\
             description: Does {name}\\n---\\nRun it.\\n' > \
             skills/{name}/SKILL.md"
        )}),
    )
}

fn offer(name: &str) -> hamlet_testing::Step {
    call(
        "submit_skill",
        json!({"skill": name, "reason": format!("{name} is useful")}),
    )
}

fn shelved(client: &Client) -> Vec<String> {
    client.realm().library.into_iter().map(|s| s.name).collect()
}

fn tool_answers(client: &Client, chat: &ChatRef) -> Vec<String> {
    client
        .items(chat)
        .into_iter()
        .filter_map(|item| item.tool)
        .map(|tool| tool.failure.unwrap_or_default())
        .collect()
}

#[tokio::test]
async fn the_librarian_judges_offers_and_answers_questions() {
    let mut s = Session::start(
        Faux::new()
            .responses([
                write_skill("greet"),
                offer("greet"),
                write_skill("shout"),
                offer("shout"),
                offer("greet"),
                offer("../x"),
                call(
                    "ask_librarian",
                    json!({"question": "How do I say hello?"}),
                ),
                words("Done."),
            ])
            .kind(
                "errand",
                [
                    words("ACCEPT It helps others."),
                    words("REFUSE It repeats greet."),
                    words("Use greet: run it."),
                ],
            ),
    )
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    assert!(shelved(&s.chief).contains(&"weather".to_string()));
    assert!(!shelved(&s.chief).contains(&"greet".to_string()));

    s.chief.tell(hand, "Offer your skills.").await;
    s.chief.shows(&now, "Done.").await;

    // The accepted skill is on the shelves, the refused one is not.
    let library = s.chief.realm().library;
    let greet = library.iter().find(|s| s.name == "greet").unwrap();
    assert_eq!(greet.description, "Does greet");
    assert!(!shelved(&s.chief).contains(&"shout".to_string()));
    assert!(s.home.file("library/greet/SKILL.md").exists());
    assert!(!s.home.file("library/shout").exists());

    // The offer of a name the library has, and a name that is no
    // folder, are refused without asking the librarian.
    let failures = tool_answers(&s.chief, &now);
    assert!(
        failures.iter().filter(|f| !f.is_empty()).count() >= 2,
        "{failures:?}"
    );

    // The question was answered from the shelves.
    let said = s.chief.texts(&now);
    assert!(said.contains(&"Done.".to_string()));

    // A chief takes a skill off the shelves again.
    assert_eq!(
        s.chief.command(hand, "/unshelve greet out of date").await,
        "Took greet off the shelves."
    );
    s.chief
        .until("the shelf without greet", |c| {
            !shelved(c).contains(&"greet".to_string())
        })
        .await;
    s.chief.refused(hand, "/unshelve greet").await;
}

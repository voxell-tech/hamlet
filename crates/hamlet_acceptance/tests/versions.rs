//! A scholar's history: versions named and listed, a hire pinned, a
//! commit reverted, a scholar forked, and a file purged.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;

async fn character(s: &mut Session, scholar: ScholarId, text: &str) {
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Character {
                id,
                scholar,
                text: text.into(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
}

/// Waits until the hire `agent` holds `text` as its character.
async fn wait_for_character(
    s: &mut Session,
    agent: AgentId,
    text: &str,
) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while !s.chief.prompt(agent).await.contains(text) {
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    })
    .await
    .expect("the hire takes the character in");
}

#[tokio::test]
async fn names_pins_reverts_and_forks_versions() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let docs = s.found("docs", "ada", "keep the docs").await;
    let ada = s.chief.scholar("ada").unwrap().id;
    character(&mut s, ada, "Brave.").await;

    assert!(
        s.chief
            .command(hand, "/versions ada")
            .await
            .starts_with("No versions yet; /tag names one.")
    );
    assert_eq!(
        s.chief.command(hand, "/tag ada v1 The first draft").await,
        "Tagged ada as v1."
    );
    let listed = s.chief.command(hand, "/versions ada").await;
    assert!(listed.contains("Versions:"), "{listed}");
    assert!(listed.contains("- v1 ("), "{listed}");
    assert!(listed.contains("): The first draft"), "{listed}");
    assert!(listed.contains("Latest commits of main:"), "{listed}");
    // A name is taken once, and `main` is no version name.
    for line in ["/tag ada v1", "/tag ada main", "/tag nobody v2"] {
        s.chief.refused(hand, line).await;
    }

    wait_for_character(&mut s, docs, "Brave.").await;
    // A pinned hire takes no later learning, until it follows main
    // again.
    let said = s.chief.command(hand, "/pin ada@docs v1 steady").await;
    assert_eq!(
        said,
        "ada@docs takes v1 in once idle, and no later learning."
    );
    s.chief.refused(hand, "/pin ada@docs v9").await;
    character(&mut s, ada, "Careful.").await;
    assert_eq!(
        s.chief.papers(Keeper::Scholar(ada)).await.character,
        "Careful."
    );
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let prompt = s.chief.prompt(docs).await;
    assert!(prompt.contains("Brave."), "{prompt}");
    assert!(!prompt.contains("Careful."), "{prompt}");
    assert_eq!(
        s.chief.command(hand, "/pin ada@docs main").await,
        "ada@docs follows its scholar's main again."
    );
    wait_for_character(&mut s, docs, "Careful.").await;

    // A revert undoes the commit a version names.
    s.chief.command(hand, "/tag ada v2").await;
    let said =
        s.chief.command(hand, "/revert ada v2 too careful").await;
    assert!(said.starts_with("Reverted "), "{said}");
    assert!(said.ends_with("on ada's main."), "{said}");
    assert_eq!(
        s.chief.papers(Keeper::Scholar(ada)).await.character,
        "Brave."
    );

    // A fork is a new scholar that starts from a version.
    character(&mut s, ada, "Loud.").await;
    assert_eq!(
        s.chief.command(hand, "/fork ada@v1 bo").await,
        "Forked ada@v1 as bo."
    );
    s.chief.refused(hand, "/fork ada@v9 cy").await;
    let bo = s.chief.scholar("bo").unwrap().id;
    assert_eq!(
        s.chief.papers(Keeper::Scholar(bo)).await.character,
        "Brave."
    );
    assert_eq!(
        s.chief.papers(Keeper::Scholar(ada)).await.character,
        "Loud."
    );
}

#[tokio::test]
async fn purges_a_file_from_every_commit_and_lodge() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let docs = s.found("docs", "ada", "keep the docs").await;
    let ada = s.chief.scholar("ada").unwrap().id;
    let note = "---\ndescription: Secret\n---\nhush\n";
    for of in [Keeper::Scholar(ada), Keeper::Agent(docs)] {
        let reply = s
            .chief
            .request(|id| {
                ClientMessage::Edit(Edit::Memory {
                    id,
                    of,
                    path: "secret.md".into(),
                    text: note.into(),
                })
            })
            .await;
        assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    }
    assert_eq!(
        s.chief.papers(Keeper::Scholar(ada)).await.memory.len(),
        1
    );
    let said =
        s.chief.command(hand, "/purge ada memory/secret.md").await;
    assert!(
        said.starts_with("Purged memory/secret.md from ada: "),
        "{said}"
    );
    assert!(
        s.chief.papers(Keeper::Scholar(ada)).await.memory.is_empty()
    );
    assert!(
        s.chief.papers(Keeper::Agent(docs)).await.memory.is_empty()
    );
    for line in ["/purge ada ../x", "/purge nobody a.md"] {
        s.chief.refused(hand, line).await;
    }
}

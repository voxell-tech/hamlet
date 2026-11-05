//! Merging two hires of a hamlet into one, and splitting them again.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;

async fn learn(s: &mut Session, hire: AgentId, path: &str) {
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(hire),
                path: path.into(),
                text: "---\ndescription: Learned\n---\nA fact.\n"
                    .into(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
}

async fn learned(s: &mut Session, hire: AgentId) -> Vec<String> {
    s.chief.papers(Keeper::Agent(hire)).await.learned
}

#[tokio::test]
async fn merging_brings_a_hires_learning_and_restoring_splits_it() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief.command(ada, "/hire bob write tests").await;
    let bob = s.chief.id("bob@docs");
    s.chief.command(ada, "/hire cy read").await;
    let cy = s.chief.id("cy@docs");
    s.chief.command(ada, "/hire dee under cy sweep").await;
    learn(&mut s, bob, "tools.md").await;

    let said = s.chief.command(ada, "/merge bob into ada").await;
    assert_eq!(said, "Merged `bob` into `ada`.");
    let gone = s.chief.agent("bob@docs").unwrap().left.unwrap();
    assert_eq!(gone.how, LeftHow::Merged);
    assert!(
        learned(&mut s, ada)
            .await
            .contains(&"memory/tools.md".into())
    );

    // The absorbed hire's own hires go to the one that stays.
    s.chief.command(ada, "/merge cy into ada kept one").await;
    assert_eq!(s.chief.agent("dee@docs").unwrap().parent, Some(ada));
    assert_eq!(s.chief.agent("cy@docs").unwrap().id, cy);

    // Splitting takes out what it brought in again.
    let said = s.chief.command(ada, "/restore bob").await;
    assert!(said.starts_with("`bob` is back; "), "{said}");
    assert!(s.chief.agent("bob@docs").unwrap().left.is_none());
    assert!(
        !learned(&mut s, ada)
            .await
            .contains(&"memory/tools.md".into())
    );
    assert!(
        learned(&mut s, bob)
            .await
            .contains(&"memory/tools.md".into())
    );

    // The mayor cannot be absorbed, and a hire joins only a hire.
    s.chief.refused(ada, "/merge ada into bob").await;
    s.chief.refused(ada, "/merge bob into nobody").await;
}

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
    let bob = s.hire(ada, "bob", "write tests").await;
    let cy = s.hire(ada, "cy", "read").await;
    s.hire_under(ada, "dee", "docs#2", "sweep").await;
    learn(&mut s, bob, "tools.md").await;

    let said =
        s.chief.command(ada, "/merge docs#1 into docs#0").await;
    assert_eq!(said, "Merged `docs#1` into `docs#0`.");
    let gone = s.chief.agent("docs#1").unwrap().left.unwrap();
    assert_eq!(gone.how, LeftHow::Merged);
    assert!(
        learned(&mut s, ada)
            .await
            .contains(&"memory/tools.md".into())
    );

    // The absorbed hire's own hires go to the one that stays.
    s.chief
        .command(ada, "/merge docs#2 into docs#0 kept one")
        .await;
    assert_eq!(s.chief.agent("docs#3").unwrap().parent, Some(ada));
    assert_eq!(s.chief.agent("docs#2").unwrap().id, cy);

    // Splitting takes out what it brought in again.
    let said = s.chief.command(ada, "/restore docs#1").await;
    assert!(said.starts_with("`docs#1` is back; "), "{said}");
    assert!(s.chief.agent("docs#1").unwrap().left.is_none());
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
    s.chief.refused(ada, "/merge docs#0 into docs#1").await;
    s.chief.refused(ada, "/merge docs#1 into docs#9").await;
}

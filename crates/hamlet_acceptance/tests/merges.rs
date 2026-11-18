//! Merging two agents of a hamlet into one, and splitting them again.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;

async fn learn(s: &mut Session, agent: AgentId, path: &str) {
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(agent),
                path: path.into(),
                text: "---\ndescription: Learned\n---\nA fact.\n"
                    .into(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
}

async fn learned(s: &mut Session, agent: AgentId) -> Vec<String> {
    s.chief.papers(Keeper::Agent(agent)).await.learned
}

#[tokio::test]
async fn merging_brings_an_agents_learning_and_restoring_splits_it() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.add_agent(ada, "bob", "write tests").await;
    let cy = s.add_agent(ada, "cy", "read").await;
    s.add_agent_under(ada, "dee", "agent#4", "sweep").await;
    learn(&mut s, bob, "tools.md").await;

    let said =
        s.chief.command(ada, "/merge agent#3 into agent#2").await;
    assert_eq!(said, "Merged `agent#3` into `agent#2`.");
    let gone = s.chief.agent("agent#3").unwrap().left.unwrap();
    assert_eq!(gone.how, LeftHow::Merged);
    assert!(
        learned(&mut s, ada)
            .await
            .contains(&"memory/tools.md".into())
    );

    // The absorbed agent's own agents go to the one that stays.
    s.chief
        .command(ada, "/merge agent#4 into agent#2 kept one")
        .await;
    assert_eq!(s.chief.agent("agent#5").unwrap().parent, Some(ada));
    assert_eq!(s.chief.agent("agent#4").unwrap().id, cy);

    // Splitting takes out what it brought in again.
    let said = s.chief.command(ada, "/restore agent#3").await;
    assert!(said.starts_with("`agent#3` is back; "), "{said}");
    assert!(s.chief.agent("agent#3").unwrap().left.is_none());
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

    // The lead cannot be absorbed, and an agent of a hamlet joins
    // only another.
    s.chief.refused(ada, "/merge agent#2 into agent#3").await;
    s.chief.refused(ada, "/merge agent#3 into agent#11").await;
}

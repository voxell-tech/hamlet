use hamlet_messages::realm::client::{AnswerRefusal, ReadWorld};
use hamlet_messages::realm::{
    AgentRefusal, Ask, Done, LetterRefusal, NeedsChief, Sent, Waiting,
};
use hamlet_messages::workspace::ConversationId;
use hamlet_protocol::client::Choice;

use super::asking::{chief_task, objection, open_waits};
use super::{
    MAYOR, THREE, answer, hamlet_with, needs, paid, tell, to, tool,
    words,
};

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_wait_that_needs_nothing_is_refused() {
    let (realm, _) = hamlet_with(1).await;

    let asked = realm
        .call_as_workspace(&NeedsChief {
            needs: Vec::new(),
            ..needs(THREE, "c1")
        })
        .await;
    let seen = realm.ask(&ReadWorld).await.unwrap();

    assert!(asked.is_err(), "{asked:?}");
    assert_eq!(seen.waits, []);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hires_letter_on_its_withdrawn_task_is_refused() {
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    let wait = open_waits(&realm, THREE).await[0].0.to_string();
    realm
        .ask(&answer(&wait, Choice::Withdraw, None))
        .await
        .unwrap();
    let ask = Ask {
        to: to(MAYOR),
        text: "Which file?".to_owned(),
        conversation: ConversationId("three".to_owned()),
        paths: Vec::new(),
    };

    let told = realm
        .tool(THREE, &paid("t1", &task, tell(to(MAYOR), "done")))
        .await
        .unwrap();
    let asked =
        realm.tool(THREE, &paid("q1", &task, ask)).await.unwrap();

    let why = AgentRefusal::Act(LetterRefusal::Withdrawn);
    assert_eq!(told, Sent::Refused { why: why.clone() });
    assert_eq!(asked, Waiting::Refused { why });
    assert_eq!(
        words(told),
        "The chief withdrew the instruction, so this call did not \
         run."
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_to_no_wait_is_refused_in_its_words() {
    let (realm, _) = hamlet_with(0).await;

    let answered = realm
        .ask(&answer("w-none", Choice::Go, None))
        .await
        .unwrap();

    assert_eq!(
        answered,
        Done::Refused {
            why: AnswerRefusal::NothingWaits {
                wait: "w-none".to_owned()
            }
        }
    );
    assert_eq!(words(answered), "nothing waits here with that id");
    realm.host.shutdown().await.unwrap();
}

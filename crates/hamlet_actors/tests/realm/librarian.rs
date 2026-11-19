use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::realm::profiles::AskLibrarian;
use hamlet_messages::realm::workers::{Ended, ErrandDone};
use hamlet_messages::realm::{Done, Tool};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ErrandNumber, RealmTask,
};

use super::{
    Realm, THREE, hamlet_with, made_until, seen_submits, standing,
};

fn errand(n: u32) -> AgentNumber {
    ErrandNumber::new(n).unwrap().agent()
}

fn in_conversation<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        task: None,
        conversation: Some(ConversationId("c".to_owned())),
        act,
    }
}

fn ask(question: &str) -> AskLibrarian {
    AskLibrarian {
        question: question.to_owned(),
    }
}

/// The librarian's instructions before a question, as today words
/// them, with the stand-in library's one skill.
const LOOKING: &str =
    "You are the realm's librarian. You keep the library of skills \
     every agent shares: you know what is on its shelves, you take \
     a skill offered only when it is useful to others, distinct \
     from the shelves, clear enough to run and free of private \
     names, keys and paths, and you never invent a skill.\n\n# The \
     shelves\n- dig: Digs a well.\n\nName the skills on the shelves \
     that do what is asked, with their exact commands. If nothing \
     fits, say so.";

async fn ended(realm: &Realm, n: u32, ended: Ended) {
    realm.tool(errand(n), &ErrandDone { ended }).await.unwrap();
}

fn notice(n: u32, text: &str, task: &RealmTask) -> Submit {
    Submit {
        id: InputId(format!("librarian/{n}")),
        task: Some(task.clone()),
        input: Input::Notice {
            text: text.to_owned(),
            conversation: Some(ConversationId("c".to_owned())),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_librarians_answer_reaches_the_asker_after_the_call() {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);

    let answer = realm
        .tool(THREE, &in_conversation("a1", ask("Which skill digs?")))
        .await
        .unwrap();
    let repeat = realm
        .tool(THREE, &in_conversation("a1", ask("Which skill digs?")))
        .await
        .unwrap();

    assert_eq!((answer, repeat), (Done::Done, Done::Done));
    let made = made_until(&realm, errand(0)).await;
    assert_eq!(made.task, task);
    let brief = made.brief.unwrap();
    assert_eq!(brief.instructions.as_deref(), Some(LOOKING));
    assert_eq!(brief.text, "hire#3 asks: Which skill digs?");
    ended(
        &realm,
        0,
        Ended::Answered {
            text: "Use dig.".to_owned(),
        },
    )
    .await;
    assert_eq!(
        seen_submits(&realm, THREE, 1).await,
        [notice(0, "The librarian says: Use dig.", &task)]
    );
    assert!(super::made(&realm, errand(1)).is_none());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_librarian_that_fails_is_told_to_the_asker() {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    realm
        .tool(THREE, &in_conversation("a1", ask("Which skill digs?")))
        .await
        .unwrap();
    made_until(&realm, errand(0)).await;

    ended(
        &realm,
        0,
        Ended::Failed {
            why: "the model is gone".to_owned(),
        },
    )
    .await;

    assert_eq!(
        seen_submits(&realm, THREE, 1).await,
        [notice(
            0,
            "The librarian could not answer: the model is gone",
            &task
        )]
    );
    realm.host.shutdown().await.unwrap();
}

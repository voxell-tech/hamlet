use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::library::{OfferId, Verdict};
use hamlet_messages::realm::profiles::{AskLibrarian, SubmitSkill};
use hamlet_messages::realm::workers::{Ended, ErrandDone};
use hamlet_messages::realm::{Done, Tool};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, RealmTask,
};

use super::{
    Callee, JUDGED_OFFER, NO_OFFER, OTHERS_OFFER, Realm, SKILL_MD,
    THREE, errand, hamlet_with, made_until, seen_submits, standing,
    until,
};

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

/// The librarian's part and the stand-in library's shelves, as today
/// words them, before what it is asked.
const ROLE: &str =
    "You are the realm's librarian. You keep the library of skills \
     every agent shares: you know what is on its shelves, you take \
     a skill offered only when it is useful to others, distinct \
     from the shelves, clear enough to run and free of private \
     names, keys and paths, and you never invent a skill.\n\n# The \
     shelves\n- dig: Digs a well.";

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
    assert_eq!(
        brief.instructions,
        Some(format!(
            "{ROLE}\n\nName the skills on the shelves that do what \
             is asked, with their exact commands. If nothing fits, \
             say so."
        ))
    );
    assert_eq!(brief.text, "hire#3 (Hire 1) asks: Which skill digs?");
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

fn submit(offer: u32, skill: &str) -> SubmitSkill {
    SubmitSkill {
        offer: OfferId(offer),
        skill: skill.to_owned(),
        reason: Some("for digging".to_owned()),
    }
}

fn verdicts(realm: &Realm) -> Vec<Verdict> {
    realm.seen.verdicts.lock().unwrap().clone()
}

const TOOK: &str = "The librarian took `well`: ACCEPT Clear and \
                    useful. It goes on the shelves for every agent; \
                    use the library's copy.";

/// The library fails the first verdict as an actor not ready; the
/// realm calls it again a second later.
#[tokio::test(flavor = "multi_thread")]
async fn the_librarians_verdict_reaches_the_library_then_the_offerer()
{
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    realm.seen.fail_once(Callee::Verdict, AgentNumber(7));

    let answer = realm
        .tool(THREE, &in_conversation("s1", submit(7, "well")))
        .await
        .unwrap();
    let again = realm
        .tool(THREE, &in_conversation("s2", submit(7, "well")))
        .await
        .unwrap();

    assert_eq!((answer, again), (Done::Done, Done::Done));
    let brief = made_until(&realm, errand(0)).await.brief.unwrap();
    assert_eq!(
        brief.instructions,
        Some(format!(
            "{ROLE}\n\nAnswer with ACCEPT or REFUSE as your first \
             word, then say why in a sentence or two."
        ))
    );
    assert_eq!(
        brief.text,
        format!(
            "hire#3 (Hire 1) offers the skill `well`: for \
             digging\n\nIts files: SKILL.md, dig.sh\n\nIts \
             SKILL.md:\n{SKILL_MD}"
        )
    );
    ended(
        &realm,
        0,
        Ended::Answered {
            text: "ACCEPT Clear and useful.".to_owned(),
        },
    )
    .await;
    assert_eq!(
        seen_submits(&realm, THREE, 1).await,
        [notice(0, TOOK, &task)]
    );
    let called = verdicts(&realm);
    let [failed, taken] = called.as_slice() else {
        panic!("one verdict, called twice: {called:?}");
    };
    assert_eq!(failed, taken, "the call again is the same verdict");
    assert_eq!(
        (taken.offer, taken.accepted, taken.words.as_str()),
        (OfferId(7), true, TOOK)
    );
    assert!(
        super::made(&realm, errand(1)).is_none(),
        "an offer is judged once"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_or_failed_judging_is_told_in_todays_words() {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    for (call, offer) in [("s1", 7), ("s2", 8)] {
        realm
            .tool(
                THREE,
                &in_conversation(call, submit(offer, "well")),
            )
            .await
            .unwrap();
    }
    made_until(&realm, errand(1)).await;

    ended(
        &realm,
        0,
        Ended::Answered {
            text: "Refuse: it names a private path.".to_owned(),
        },
    )
    .await;
    ended(
        &realm,
        1,
        Ended::Failed {
            why: "the model is gone".to_owned(),
        },
    )
    .await;

    let refused = "The librarian did not take `well`: Refuse: it \
                   names a private path. It stays in your lodge.";
    let failed =
        "The librarian could not judge `well`: the model is gone";
    let mut told = seen_submits(&realm, THREE, 2).await;
    told.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    assert_eq!(
        told,
        [notice(0, refused, &task), notice(1, failed, &task)]
    );
    until("both verdicts", || verdicts(&realm).len() == 2).await;
    let mut called = verdicts(&realm);
    called.sort_by_key(|verdict| verdict.offer.0);
    assert_eq!(
        called
            .iter()
            .map(|verdict| (verdict.accepted, verdict.words.as_str()))
            .collect::<Vec<_>>(),
        [(false, refused), (false, failed)]
    );
    assert_ne!(
        called[0].id, called[1].id,
        "each call has its own id"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verdict_the_library_refuses_is_told_to_the_offerer() {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    realm
        .tool(
            THREE,
            &in_conversation("s1", submit(JUDGED_OFFER, "well")),
        )
        .await
        .unwrap();
    made_until(&realm, errand(0)).await;

    ended(
        &realm,
        0,
        Ended::Answered {
            text: "ACCEPT Clear and useful.".to_owned(),
        },
    )
    .await;

    let unjudged = "The librarian could not judge `well`: the offer \
                    406 was judged already";
    assert_eq!(
        seen_submits(&realm, THREE, 1).await,
        [notice(0, unjudged, &task)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_not_the_callers_own_or_not_held_is_not_judged() {
    let (realm, _) = hamlet_with(1).await;

    for (call, offer) in [("s1", OTHERS_OFFER), ("s2", NO_OFFER)] {
        let answer = realm
            .tool(
                THREE,
                &in_conversation(call, submit(offer, "well")),
            )
            .await
            .unwrap();
        assert_eq!(answer, Done::Done);
    }

    let told = |call: &str, why: &str| Submit {
        id: InputId(format!("submit_skill/{call}")),
        task: None,
        input: Input::Notice {
            text: format!(
                "The librarian could not judge `well`: {why}"
            ),
            conversation: Some(ConversationId("c".to_owned())),
        },
    };
    let mut seen = seen_submits(&realm, THREE, 2).await;
    seen.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    assert_eq!(
        seen,
        [
            told("s1", "the offer 405 is not yours"),
            told("s2", "there is no offer 404"),
        ]
    );
    assert!(super::made(&realm, errand(0)).is_none(), "no errand");
    realm.host.shutdown().await.unwrap();
}

/// The asker's agent fails the first notice of each as an actor not
/// ready; the realm gives it again a second later.
#[tokio::test(flavor = "multi_thread")]
async fn the_librarians_answer_and_verdict_are_told_again_until_taken()
 {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    let asked = realm
        .tool(THREE, &in_conversation("a1", ask("Which skill digs?")))
        .await
        .unwrap();
    let submitted = realm
        .tool(THREE, &in_conversation("s1", submit(7, "well")))
        .await
        .unwrap();
    assert_eq!((asked, submitted), (Done::Done, Done::Done));
    made_until(&realm, errand(1)).await;

    realm.seen.fail_once(Callee::Submit, THREE);
    let said = Ended::Answered {
        text: "Use dig.".to_owned(),
    };
    ended(&realm, 0, said).await;
    let says = notice(0, "The librarian says: Use dig.", &task);
    assert_eq!(
        seen_submits(&realm, THREE, 1).await,
        std::slice::from_ref(&says)
    );

    realm.seen.fail_once(Callee::Submit, THREE);
    let judged = Ended::Answered {
        text: "ACCEPT Clear and useful.".to_owned(),
    };
    ended(&realm, 1, judged).await;
    assert_eq!(
        seen_submits(&realm, THREE, 2).await,
        [says, notice(1, TOOK, &task)]
    );
    realm.host.shutdown().await.unwrap();
}

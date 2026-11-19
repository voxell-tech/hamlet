use std::sync::atomic::Ordering;

use hamlet_messages::agent::{AgentInput, AgentKind, BuiltIn};
use hamlet_messages::profile::{
    DreamId, DreamOpen, EndDream, PurgeAnswer,
};
use hamlet_messages::realm::budget::{Budget, Payer, SpendRefusal};
use hamlet_messages::realm::profiles::{
    Dream, DreamAskedRefusal, DreamRefusal, Dreamer, PurgeProfile,
};
use hamlet_messages::realm::workers::{BUILT_IN, Ended, ErrandDone};
use hamlet_messages::realm::{
    Done, ProfileId, Refusable as _, SetTools, ToolName,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{AgentNumber, ErrandNumber};
use hamlet_protocol::client::RequestId;

use super::chiefs::chief;
use super::profiles::{asked, create};
use super::{
    Callee, Realm, THREE, charge, hamlet_with, made, made_until,
    may_spend, to, until,
};

/// The most a dream may spend, in millionths of a dollar, as today's
/// default.
const DREAM_COST: u64 = 200_000;

fn errand(n: u32) -> AgentNumber {
    ErrandNumber::new(n).unwrap().agent()
}

fn of_hire(hire: AgentNumber) -> Dream {
    Dream {
        of: Dreamer::Hire { hire },
    }
}

fn dream_ends(realm: &Realm) -> Vec<(ProfileId, EndDream)> {
    realm.seen.dream_ends.lock().unwrap().clone()
}

fn ended(done: bool, summary: &str) -> EndDream {
    EndDream {
        dream: DreamId(1),
        done,
        summary: summary.to_owned(),
    }
}

async fn errand_ends(realm: &Realm, n: u32, ended: Ended) {
    realm.tool(errand(n), &ErrandDone { ended }).await.unwrap();
}

async fn ends(realm: &Realm) -> Vec<EndDream> {
    until("the dream ends", || !dream_ends(realm).is_empty()).await;
    dream_ends(realm).into_iter().map(|(_, end)| end).collect()
}

fn reaches(realm: &Realm, agent: AgentNumber) -> Vec<Reach> {
    realm
        .seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .filter(|set| set.agent == agent)
        .map(|set| set.reach.clone())
        .collect()
}

fn built_ins() -> Vec<BuiltIn> {
    BUILT_IN
        .iter()
        .map(|name| BuiltIn {
            name: (*name).to_owned(),
        })
        .collect()
}

/// The hire's dream runs as an errand with today's instructions over
/// the two folders, works in the dream's folder before it is made,
/// and ends the dream done with the first line of its answer.
#[tokio::test(flavor = "multi_thread")]
async fn an_answered_dream_errand_ends_the_dream_done_with_its_first_line()
 {
    let (realm, _) = hamlet_with(1).await;

    assert_eq!(chief(&realm, of_hire(THREE)).await, Done::Done);
    let made = made_until(&realm, errand(0)).await;
    let folder = realm.scratch.join("dreams/1");
    let set_before_made = reaches(&realm, errand(0));
    errand_ends(
        &realm,
        0,
        Ended::Answered {
            text: "Brought home the notes.\nAnd tidied.".to_owned(),
        },
    )
    .await;

    assert_eq!(made.kind, AgentKind::Errand);
    assert_eq!(made.place.folder, Some(folder.clone()));
    let brief = made.brief.unwrap();
    let label = "scholar#3 (Hire 1)";
    assert_eq!(
        brief.instructions,
        Some(format!(
            "You are dreaming, between your work as `hire#3`, a \
             hire of the scholar `{label}`. Your lodge at {} holds \
             what you learned as this hire: your memory/ and \
             skills/. `{label}`'s shared knowledge, its main, is \
             checked out at {}; every hire of `{label}` starts from \
             it and takes it in.\n\nBring home what any hire of \
             `{label}` would want to know: move or merge it into \
             the checkout's memory/ and skills/, written for a \
             reader who was not there. Keep in your lodge what only \
             this hire needs. Tidy as you go: one fact in one \
             place, nothing stale, the root memory files short, and \
             a true `description:` on every memory file. Do not \
             change CHARACTER.md. When done, answer in one line \
             what you brought home, or `Nothing to bring home.`",
            folder.join("hire").display(),
            folder.join("main").display(),
        ))
    );
    assert_eq!(brief.text, "Dream now.");
    assert_eq!(brief.tools, built_ins());
    assert!(
        matches!(
            set_before_made.as_slice(),
            [Reach::Dream { folder: set, .. }, ..] if *set == folder
        ),
        "{set_before_made:?}"
    );
    assert_eq!(
        ends(&realm).await,
        [ended(true, "Brought home the notes.")]
    );
    until("the errand's reach ends", || {
        reaches(&realm, errand(0)).last() == Some(&Reach::Ended)
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_dream_errand_ends_the_dream_not_done() {
    let (realm, _) = hamlet_with(1).await;

    assert_eq!(chief(&realm, of_hire(THREE)).await, Done::Done);
    made_until(&realm, errand(0)).await;
    errand_ends(
        &realm,
        0,
        Ended::Failed {
            why: "its turn was stopped".to_owned(),
        },
    )
    .await;

    assert_eq!(ends(&realm).await, [ended(false, "")]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dream_with_nothing_to_go_over_runs_no_errand() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.nothing_to_dream.store(true, Ordering::SeqCst);

    assert_eq!(chief(&realm, of_hire(THREE)).await, Done::Done);

    assert_eq!(ends(&realm).await, [ended(false, "")]);
    assert!(made(&realm, errand(0)).is_none());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profiles_dream_tidies_its_main_in_todays_words() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;

    let asked = Dream {
        of: Dreamer::Profile {
            profile: ProfileId(3),
        },
    };
    assert_eq!(chief(&realm, asked).await, Done::Done);
    let AgentInput { brief, .. } =
        made_until(&realm, errand(0)).await;

    let main = realm.scratch.join("dreams/1/main");
    assert_eq!(
        brief.unwrap().instructions,
        Some(format!(
            "You are tidying the knowledge of the scholar \
             `scholar#3 (Mason)`, checked out at {}: its memory/ \
             and skills/. Merge what is said twice, correct or \
             remove what is stale, keep the root memory files short \
             and every memory file's `description:` true, and make \
             each memory folder's MEMORY.md describe it. Do not \
             change CHARACTER.md. Answer in one line what you \
             changed, or `Nothing to tidy.`",
            main.display()
        ))
    );
    realm.host.shutdown().await.unwrap();
}

/// A dream's errand may spend today's cost of a dream, and no more.
#[tokio::test(flavor = "multi_thread")]
async fn a_dreams_errand_spends_no_more_than_a_dream_may() {
    let (realm, hamlet) = hamlet_with(1).await;

    assert_eq!(chief(&realm, of_hire(THREE)).await, Done::Done);
    let task = made_until(&realm, errand(0)).await.task;
    let before = may_spend(&realm, errand(0), &task).await;
    realm
        .tool(
            errand(0),
            &charge(
                "d1",
                &task,
                u32::try_from(DREAM_COST).unwrap(),
                1,
            ),
        )
        .await
        .unwrap();
    let after = may_spend(&realm, errand(0), &task).await;

    assert_eq!(before, Done::Done);
    assert_eq!(
        after,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::OverCost {
                payer: Payer::Hamlet { hamlet },
                spent_micros: DREAM_COST,
                budget_micros: DREAM_COST,
            }),
        }
    );
    realm.host.shutdown().await.unwrap();
}

/// A second dream and a purge of the dream's profile are refused
/// while its errand runs, and a dream starts again once it ended.
#[tokio::test(flavor = "multi_thread")]
async fn a_dream_stays_open_while_its_errand_runs() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let of_mason = || Dream {
        of: Dreamer::Profile {
            profile: ProfileId(3),
        },
    };
    assert_eq!(chief(&realm, of_mason()).await, Done::Done);
    made_until(&realm, errand(0)).await;

    let second = chief(&realm, of_mason()).await;
    let purge = PurgeProfile {
        profile: ProfileId(3),
        path: "memory/notes.md".to_owned(),
    };
    let purged = asked(&realm, &RequestId::new(), purge).await;
    errand_ends(
        &realm,
        0,
        Ended::Failed {
            why: "its turn was stopped".to_owned(),
        },
    )
    .await;
    ends(&realm).await;
    let third = chief(&realm, of_mason()).await;

    assert_eq!(
        second,
        Done::Refused {
            why: DreamAskedRefusal::Dream(DreamRefusal::Running),
        }
    );
    assert_eq!(
        purged,
        PurgeAnswer::refused(
            DreamOpen::Dreaming { dream: DreamId(1) }.into()
        )
    );
    assert_eq!(third, Done::Done);
    made_until(&realm, errand(1)).await;
    realm.host.shutdown().await.unwrap();
}

/// A dream whose folders cannot be checked out ends not done, and the
/// next dream starts.
#[tokio::test(flavor = "multi_thread")]
async fn a_dream_that_cannot_be_prepared_ends_not_done() {
    let (realm, _) = hamlet_with(1).await;
    realm
        .seen
        .fail_for_good(Callee::PrepareDream, AgentNumber(3));

    assert_eq!(chief(&realm, of_hire(THREE)).await, Done::Done);
    assert_eq!(ends(&realm).await, [ended(false, "")]);
    let next = chief(&realm, of_hire(THREE)).await;

    assert_eq!(next, Done::Done);
    assert!(made(&realm, errand(0)).is_none());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dreams_errand_has_no_tool_its_hires_lead_turned_off() {
    let (realm, _) = hamlet_with(1).await;
    let off = SetTools {
        hire: to(THREE),
        off: vec![ToolName {
            name: "bash".to_owned(),
        }],
        on: Vec::new(),
        why: None,
    };
    assert_eq!(chief(&realm, off).await, Done::Done);

    assert_eq!(chief(&realm, of_hire(THREE)).await, Done::Done);
    let brief = made_until(&realm, errand(0)).await.brief.unwrap();

    let mut kept = built_ins();
    kept.retain(|tool| tool.name != "bash");
    assert_eq!(brief.tools, kept);
    realm.host.shutdown().await.unwrap();
}

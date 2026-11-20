use hamlet_messages::realm::budget::{
    Budget, BudgetRead, ReadBudget, ReadBudgetRefusal, SpendRefusal,
};
use hamlet_messages::realm::documents::ReadCopies;
use hamlet_messages::realm::models::{
    Cause, Credential, KeepCredential, Kept, OauthLogin, Settings,
    TurnFailed,
};
use hamlet_messages::realm::workers::{
    Ended, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, Secrets,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use hamlet_protocol::client::RequestId;

use super::{
    FOUR, MAYOR, Realm, THREE, charge, error_code, hamlet_with,
    may_spend, needs, paid, standing, tell, to, tool, worker_shown,
};

const NO_ROW: AgentNumber = AgentNumber(77);

fn oauth(access: &str) -> Kept {
    Kept::Oauth {
        login: OauthLogin {
            access: access.to_owned(),
            refresh: format!("{access}-refresh"),
            expires: 1_700_000_000,
            extra: None,
        },
    }
}

fn plain(key: &str) -> Kept {
    Kept::Key {
        key: key.to_owned(),
        env: Settings::new(),
    }
}

fn keep(provider: &str, value: Kept) -> KeepCredential {
    KeepCredential {
        provider: provider.to_owned(),
        value,
    }
}

fn credential(provider: &str) -> Credential {
    Credential {
        provider: provider.to_owned(),
    }
}

fn turn_failed(task: &RealmTask) -> TurnFailed {
    TurnFailed {
        task: task.clone(),
        conversation: ConversationId("c".to_owned()),
        words: "The model failed.".to_owned(),
        cause: Cause::Model,
    }
}

async fn end(realm: &Realm, hire: AgentNumber) {
    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: End {
                hire: to(hire),
                why: None,
            },
        })
        .await
        .unwrap();
    assert_eq!(ended, Done::Done);
}

/// The error code of each call an agent makes for its credentials,
/// its copies, a charge and a failed turn, as `agent`.
async fn codes(
    realm: &Realm,
    agent: AgentNumber,
    task: &RealmTask,
) -> Vec<String> {
    vec![
        error_code(realm.tool(agent, &credential("anthropic")).await),
        error_code(
            realm.tool(agent, &keep("anthropic", oauth("a"))).await,
        ),
        error_code(realm.tool(agent, &turn_failed(task)).await),
        error_code(realm.tool(agent, &ReadCopies::default()).await),
        error_code(
            realm.tool(agent, &charge("c1", task, 1, 1)).await,
        ),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_agents_reads_and_stores_are_refused() {
    // hire 3 reads its copies; the chief ends it: its read of a
    // credential, a store, a failed turn, a read of its copies and a
    // charge are each refused, as are a number with no row's.
    let (realm, _) = hamlet_with(1).await;
    realm
        .tool(Hand::NUMBER, &keep("anthropic", plain("sk-a")))
        .await
        .unwrap();
    let task = standing(&realm, THREE);
    let before = realm.tool(THREE, &ReadCopies::default()).await;

    end(&realm, THREE).await;
    let ended = codes(&realm, THREE, &task).await;
    let no_row = codes(&realm, NO_ROW, &task).await;

    assert!(before.is_ok(), "{before:?}");
    let refused = vec!["message.wrong_caller".to_owned(); 5];
    assert_eq!(ended, refused);
    assert_eq!(no_row, refused);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_workers_read_of_its_copies_is_refused() {
    let (realm, _) = hamlet_with(1).await;
    let started = realm
        .tool(
            THREE,
            &tool(
                "w",
                StartWorker {
                    brief: "count the files".to_owned(),
                    tools: Vec::new(),
                    task: None,
                    hamlet: None,
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        started,
        Started::Started {
            worker: worker_shown(FOUR)
        }
    );
    let running = realm.tool(FOUR, &ReadCopies::default()).await;

    realm
        .tool(
            FOUR,
            &WorkerDone {
                ended: Ended::Answered {
                    text: "3".to_owned(),
                },
            },
        )
        .await
        .unwrap();
    let finished = realm.tool(FOUR, &ReadCopies::default()).await;

    assert!(running.is_ok(), "{running:?}");
    assert_eq!(error_code(finished), "message.wrong_caller");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_secret_list_answers_an_ended_agent_and_refuses_a_number_with_no_row()
 {
    let (realm, _) = hamlet_with(1).await;
    realm
        .tool(Hand::NUMBER, &keep("anthropic", plain("sk-a")))
        .await
        .unwrap();
    end(&realm, THREE).await;

    let ended = realm.tool(THREE, &Secrets).await.unwrap();
    let workspace = realm.call_as_workspace(&Secrets).await.unwrap();
    let no_row = realm.tool(NO_ROW, &Secrets).await;

    assert!(ended.values.contains(&"sk-a".to_owned()));
    assert_eq!(ended.values, workspace.values);
    assert_eq!(error_code(no_row), "message.wrong_caller");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hires_login_refresh_is_kept_and_its_other_stores_are_refused()
 {
    // the Hand signs in to chatgpt by login and to openai by key.
    // Hire 3 refreshes the chatgpt login: kept. Its plain key for
    // chatgpt, its login for openai and its login for a provider not
    // signed in are refused, and the file keeps the Hand's.
    let (realm, _) = hamlet_with(1).await;
    for (provider, value) in
        [("chatgpt", oauth("one")), ("openai", plain("sk-o"))]
    {
        realm
            .tool(Hand::NUMBER, &keep(provider, value))
            .await
            .unwrap();
    }

    let refreshed =
        realm.tool(THREE, &keep("chatgpt", oauth("two"))).await;
    let refused = [
        realm.tool(THREE, &keep("chatgpt", plain("sk-c"))).await,
        realm.tool(THREE, &keep("openai", oauth("three"))).await,
        realm.tool(THREE, &keep("anthropic", oauth("four"))).await,
    ];
    let stored = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(realm.scratch.join("credentials.json"))
            .unwrap(),
    )
    .unwrap();

    assert!(refreshed.is_ok(), "{refreshed:?}");
    assert_eq!(
        refused.map(error_code),
        ["message.wrong_caller"; 3].map(str::to_owned)
    );
    assert_eq!(stored["models"]["chatgpt"]["access"], "two");
    assert_eq!(stored["models"]["openai"]["key"], "sk-o");
    assert!(stored["models"].get("anthropic").is_none());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_spends_on_its_own_task_and_on_a_letters_but_on_no_other()
 {
    // the mayor tells hire 3 on the mayor's own task. Hire 3 may
    // spend on its own task and on the letter's; the Hand's task is
    // refused for a spend and for a read of its budget.
    let (realm, _) = hamlet_with(1).await;
    let own = standing(&realm, THREE);
    let mayors = standing(&realm, MAYOR);
    let hands = standing(&realm, Hand::NUMBER);
    let before = may_spend(&realm, THREE, &mayors).await;
    realm
        .tool(MAYOR, &paid("t1", &mayors, tell(to(THREE), "hello")))
        .await
        .unwrap();

    let spends = [
        may_spend(&realm, THREE, &own).await,
        may_spend(&realm, THREE, &mayors).await,
        may_spend(&realm, THREE, &hands).await,
    ];
    let read = realm
        .tool(
            THREE,
            &tool(
                "b1",
                ReadBudget {
                    task: Some(hands.clone()),
                },
            ),
        )
        .await
        .unwrap();

    let no_task = Done::Refused {
        why: SpendRefusal::Budget(Budget::NoSuchTask),
    };
    assert_eq!(before, no_task);
    assert_eq!(spends, [Done::Done, Done::Done, no_task]);
    assert_eq!(
        read,
        BudgetRead::Refused {
            why: AgentRefusal::Act(ReadBudgetRefusal::NoSuchTask)
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_of_a_number_with_no_row_opens_no_wait() {
    let (realm, _) = hamlet_with(0).await;

    realm.call_as_workspace(&needs(NO_ROW, "c1")).await.unwrap();
    let waits = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM wait WHERE agent = ?1",
                [NO_ROW.0],
                |row| row.get::<_, i64>(0),
            )
        })
        .await;

    assert_eq!(waits, 0);
    realm.host.shutdown().await.unwrap();
}

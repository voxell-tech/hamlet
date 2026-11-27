use std::path::PathBuf;
use std::sync::Arc;

use hamlet_actor_error::ActorError;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::Input;
use hamlet_messages::realm::choices::SetNickname;
use hamlet_messages::realm::client::ReadState;
use hamlet_messages::realm::documents::{
    CardSetting, EditCatalogue, EditSettings, Endpoint,
    RemoteWorkspace, Setting, SettingRefusal,
};
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::models::{
    Credential, CredentialAnswer, CredentialRefusal, KeepCredential,
    Kept, OauthLogin, Settings,
};
use hamlet_messages::realm::oversight::{AskChief, Question};
use hamlet_messages::realm::workers::{StartWorker, Started};
use hamlet_messages::realm::{Done, SecretList, Secrets};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::SecretsVersion;
use hamlet_messages::workspace::{AgentNumber, ConversationId};
use hamlet_protocol::client::{self as protocol, Choice, RequestId};
use hamlet_testing::Scratch;
use serde_json::{Value, json};

use super::asking::chief_task;
use super::chiefs::chief;
use super::stand_ins::{Seen, open};
use super::{
    MAYOR, Realm, THREE, answer, hamlet_with, made_until, say,
    seen_submits, submits_to, tell, to, tool, until,
};

fn file(realm: &Realm) -> PathBuf {
    realm.scratch.join("credentials.json")
}

fn stored(realm: &Realm) -> Value {
    serde_json::from_slice(&std::fs::read(file(realm)).unwrap())
        .unwrap()
}

fn key(key: &str) -> Kept {
    Kept::Key {
        key: key.to_owned(),
        env: Settings::new(),
    }
}

fn settings() -> Settings {
    [("ACCOUNT_ID", "acc-1"), ("GATEWAY_ID", "gateway-1")]
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .into()
}

fn login(access: &str) -> OauthLogin {
    OauthLogin {
        access: access.to_owned(),
        refresh: format!("{access}-refresh"),
        expires: 1_700_000_000,
        extra: Some(r#"{"accountId":"acc-1"}"#.to_owned()),
    }
}

async fn keep(realm: &Realm, provider: &str, value: Kept) {
    let keep = KeepCredential {
        provider: provider.to_owned(),
        value,
    };
    realm.tool(Hand::NUMBER, &keep).await.unwrap();
}

async fn credential(
    realm: &Realm,
    agent: AgentNumber,
    provider: &str,
) -> CredentialAnswer {
    let asked = Credential {
        provider: provider.to_owned(),
    };
    realm.tool(agent, &asked).await.unwrap()
}

/// Writes `stored` as a realm's file, and starts the realm again on
/// it.
async fn restarted_on(realm: Realm, stored: &Value) -> Realm {
    std::fs::write(file(&realm), stored.to_string()).unwrap();
    realm.restart().await
}

fn stored_file() -> Value {
    json!({
        "installationId": "a3c1",
        "connection": { "token": "connection-token" },
        "models": {
            "openai": { "type": "api_key", "key": "sk-stored" },
            "chatgpt": {
                "type": "oauth",
                "access": "stored-access",
                "refresh": "stored-refresh",
                "expires": 1_700_000_000_123.0,
                "accountId": "acc-1",
            },
        },
        "workspace": {
            "url": "https://work.invalid",
            "token": "workspace-token",
        },
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kept_credential_is_0600_and_survives_a_restart() {
    let realm = Realm::start().await;
    keep(&realm, "anthropic", key("sk-first")).await;
    let realm = realm.restart().await;

    keep(
        &realm,
        "chatgpt",
        Kept::Oauth {
            login: login("one"),
        },
    )
    .await;

    assert_eq!(
        stored(&realm)["models"],
        json!({
            "anthropic": { "type": "api_key", "key": "sk-first" },
            "chatgpt": {
                "type": "oauth",
                "access": "one",
                "refresh": "one-refresh",
                "expires": 1_700_000_000_000_u64,
                "accountId": "acc-1",
            },
        })
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(file(&realm))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_keep_writes_the_same_bytes() {
    let realm = Realm::start().await;
    let oauth = || Kept::Oauth {
        login: login("one"),
    };
    keep(&realm, "chatgpt", oauth()).await;
    let first = std::fs::read(file(&realm)).unwrap();

    keep(&realm, "chatgpt", oauth()).await;

    assert_eq!(std::fs::read(file(&realm)).unwrap(), first);
    assert_eq!(
        stored(&realm)["version"],
        2,
        "one change, one version"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_keeps_at_once_keep_both() {
    let realm = Realm::start().await;

    tokio::join!(
        keep(&realm, "anthropic", key("sk-a")),
        keep(&realm, "openai", key("sk-b")),
    );

    let models = stored(&realm)["models"].clone();
    assert_eq!(models["anthropic"]["key"], "sk-a");
    assert_eq!(models["openai"]["key"], "sk-b");
    assert_eq!(stored(&realm)["version"], 3);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_file_keeps_what_the_realm_does_not_read() {
    let realm =
        restarted_on(Realm::start().await, &stored_file()).await;

    keep(&realm, "anthropic", key("sk-new")).await;

    let mut expected = stored_file();
    expected["models"]["anthropic"] =
        json!({ "type": "api_key", "key": "sk-new" });
    expected["version"] = json!(2);
    assert_eq!(stored(&realm), expected);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn keeping_a_credential_raises_every_reach() {
    let (realm, _) = hamlet_with(1).await;
    let living = [Hand::NUMBER, MAYOR, THREE];
    let newest = |agent| {
        realm
            .seen
            .reaches
            .lock()
            .unwrap()
            .iter()
            .filter(|set| set.agent == agent)
            .map(|set| set.version)
            .max()
    };
    let before = living.map(newest);

    keep(&realm, "anthropic", key("sk-a")).await;

    until("every living agent's reach is raised", || {
        living
            .iter()
            .zip(&before)
            .all(|(&agent, &was)| newest(agent) > was)
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_credential_is_given_only_to_an_agent() {
    let realm = Realm::start().await;
    keep(&realm, "anthropic", key("sk-a")).await;
    let realm = realm.restart().await;
    let asked = Credential {
        provider: "anthropic".to_owned(),
    };

    let given = credential(&realm, Hand::NUMBER, "anthropic").await;
    let unknown = credential(&realm, Hand::NUMBER, "openai").await;

    assert_eq!(
        given,
        CredentialAnswer::Key {
            key: "sk-a".to_owned(),
            env: Settings::new(),
        }
    );
    assert_eq!(
        unknown,
        CredentialAnswer::Refused {
            why: CredentialRefusal::NotSignedIn {
                provider: "openai".to_owned()
            }
        }
    );
    assert!(realm.ask(&asked).await.is_err(), "a client is refused");
    assert!(
        realm.call_as_workspace(&asked).await.is_err(),
        "the workspace is refused"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_is_kept_and_given_back_with_its_settings() {
    let realm = Realm::start().await;
    let with_settings = Kept::Key {
        key: "sk-a".to_owned(),
        env: settings(),
    };
    keep(&realm, "cloudflare", with_settings).await;
    let realm = realm.restart().await;

    let given = credential(&realm, Hand::NUMBER, "cloudflare").await;
    let list: SecretList =
        realm.call_as_workspace(&Secrets).await.unwrap();

    assert_eq!(
        given,
        CredentialAnswer::Key {
            key: "sk-a".to_owned(),
            env: settings(),
        }
    );
    assert_eq!(
        stored(&realm)["models"]["cloudflare"],
        json!({
            "type": "api_key",
            "key": "sk-a",
            "env": { "ACCOUNT_ID": "acc-1", "GATEWAY_ID": "gateway-1" },
        })
    );
    assert!(list.values.iter().any(|held| held == "sk-a"));
    for setting in ["acc-1", "gateway-1"] {
        assert!(
            !list.values.iter().any(|held| held == setting),
            "a setting is not a secret"
        );
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_key_of_settings_alone_reads_with_an_empty_key() {
    let mut file = stored_file();
    file["models"]["bedrock"] = json!({ "type": "api_key", "env": { "AWS_PROFILE": "work" } });
    file["models"]["blank"] = json!({ "type": "api_key" });
    let realm = restarted_on(Realm::start().await, &file).await;

    let given = credential(&realm, Hand::NUMBER, "bedrock").await;
    let blank = credential(&realm, Hand::NUMBER, "blank").await;

    assert_eq!(
        given,
        CredentialAnswer::Key {
            key: String::new(),
            env: [("AWS_PROFILE".to_owned(), "work".to_owned())]
                .into(),
        }
    );
    assert_eq!(
        blank,
        CredentialAnswer::Refused {
            why: CredentialRefusal::NotSignedIn {
                provider: "blank".to_owned()
            }
        },
        "a key of neither a key nor settings is none"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_file_reads_as_an_agent_keeps_it() {
    let realm =
        restarted_on(Realm::start().await, &stored_file()).await;

    let oauth = credential(&realm, Hand::NUMBER, "chatgpt").await;
    let api_key = credential(&realm, Hand::NUMBER, "openai").await;

    assert_eq!(
        oauth,
        CredentialAnswer::Oauth {
            login: OauthLogin {
                access: "stored-access".to_owned(),
                refresh: "stored-refresh".to_owned(),
                expires: 1_700_000_000,
                extra: Some(r#"{"accountId":"acc-1"}"#.to_owned()),
            }
        }
    );
    assert_eq!(
        api_key,
        CredentialAnswer::Key {
            key: "sk-stored".to_owned(),
            env: Settings::new(),
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_reads_each_kept_value_under_a_raised_version()
{
    let realm =
        restarted_on(Realm::start().await, &stored_file()).await;
    realm.live(Hand::NUMBER).await;
    let list = async || -> SecretList {
        realm.call_as_workspace(&Secrets).await.unwrap()
    };
    let first = list().await;

    keep(&realm, "openai", key("sk-second")).await;
    let second = list().await;

    assert_eq!(first.version, SecretsVersion(1));
    for value in [
        "connection-token",
        "workspace-token",
        "sk-stored",
        "stored-access",
        "stored-refresh",
    ] {
        assert!(
            first.values.iter().any(|held| held == value),
            "{value}"
        );
    }
    assert_eq!(second.version, SecretsVersion(2));
    assert!(second.values.iter().any(|held| held == "sk-second"));
    assert!(
        second.values.iter().any(|held| held == "sk-stored"),
        "a replaced value is still taken out"
    );
    until("the Hand's reach asks for the kept values", || {
        realm.seen.reaches.lock().unwrap().iter().any(|set| {
            set.agent == Hand::NUMBER
                && set.reach.footing().is_some_and(|footing| {
                    footing.secrets == SecretsVersion(2)
                })
        })
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

fn choose(workspace: Option<(&str, &str)>) -> EditSettings {
    EditSettings {
        set: vec![Setting::Workspace {
            workspace: workspace.map(|(url, token)| {
                RemoteWorkspace {
                    url: url.to_owned(),
                    token: token.to_owned(),
                }
            }),
        }],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chosen_workspace_is_kept_with_the_credentials() {
    let (realm, _) = hamlet_with(1).await;

    let first =
        chief(&realm, choose(Some((" https://one.invalid ", "t-1"))));
    let first = first.await;
    let second =
        chief(&realm, choose(Some(("https://two.invalid", "t-2"))));
    let second = second.await;
    let kept = stored(&realm);
    let list: SecretList =
        realm.call_as_workspace(&Secrets).await.unwrap();
    let removed = chief(&realm, choose(None)).await;

    assert_eq!(
        [first, second, removed],
        [Done::Done, Done::Done, Done::Done]
    );
    assert_eq!(
        kept["workspace"],
        json!({ "url": "https://two.invalid", "token": "t-2" })
    );
    assert_eq!(list.version, SecretsVersion(3));
    for token in ["t-1", "t-2"] {
        assert!(
            list.values.iter().any(|held| held == token),
            "{token}"
        );
    }
    assert_eq!(stored(&realm).get("workspace"), None);
    assert_eq!(stored(&realm)["version"], 4);
    until("every living reach asks for the last version", || {
        [Hand::NUMBER, MAYOR, THREE].iter().all(|&agent| {
            realm.seen.reaches.lock().unwrap().iter().any(|set| {
                set.agent == agent
                    && set.reach.footing().is_some_and(|footing| {
                        footing.secrets == SecretsVersion(4)
                    })
            })
        })
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_at_no_address_is_refused_and_not_kept() {
    let realm = Realm::start().await;

    let refused =
        chief(&realm, choose(Some(("ftp://w", "t-1")))).await;

    assert_eq!(
        refused,
        Done::Refused {
            why: SettingRefusal::NotHttp {
                address: "ftp://w".to_owned()
            }
        }
    );
    assert!(!file(&realm).exists());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_file_keeps_its_workspace() {
    let realm =
        restarted_on(Realm::start().await, &stored_file()).await;

    let other =
        chief(&realm, choose(Some(("https://w.invalid", "t-1"))));
    let list: SecretList =
        realm.call_as_workspace(&Secrets).await.unwrap();

    assert!(list.values.iter().any(|held| held == "workspace-token"));
    assert_eq!(other.await, Done::Done);
    assert_eq!(
        stored(&realm)["workspace"],
        json!({ "url": "https://w.invalid", "token": "t-1" })
    );
    realm.host.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_start_makes_the_file_0600() {
    use std::os::unix::fs::PermissionsExt as _;
    let realm = Realm::start().await;
    std::fs::write(file(&realm), stored_file().to_string()).unwrap();
    std::fs::set_permissions(
        file(&realm),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    let realm = realm.restart().await;

    let mode = std::fs::metadata(file(&realm)).unwrap().permissions();
    assert_eq!(mode.mode() & 0o777, 0o600);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_login_read_and_kept_back_unchanged_changes_nothing() {
    let realm =
        restarted_on(Realm::start().await, &stored_file()).await;
    let CredentialAnswer::Oauth { login } =
        credential(&realm, Hand::NUMBER, "chatgpt").await
    else {
        panic!("the stored login reads as one");
    };
    let before = stored(&realm);

    keep(&realm, "chatgpt", Kept::Oauth { login }).await;

    let after = stored(&realm);
    assert_eq!(
        after["models"]["chatgpt"],
        before["models"]["chatgpt"]
    );
    assert_eq!(after["version"], before["version"]);
    realm.host.shutdown().await.unwrap();
}

fn endpoints(names: &[(&str, &str)]) -> EditSettings {
    EditSettings {
        set: vec![Setting::Endpoints {
            endpoints: names
                .iter()
                .map(|&(name, address)| Endpoint {
                    name: name.to_owned(),
                    base_url: address.to_owned(),
                })
                .collect(),
        }],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn state_shows_which_endpoints_keep_a_key() {
    let realm = Realm::start().await;
    let both = [
        ("keyed", "https://k.invalid"),
        ("open", "https://o.invalid"),
    ];
    assert_eq!(chief(&realm, endpoints(&both)).await, Done::Done);
    keep(&realm, "keyed", key("sk-made-up")).await;

    let state: protocol::State = realm.ask(&ReadState).await.unwrap();
    let shown = state
        .settings
        .endpoints
        .iter()
        .map(|endpoint| (endpoint.name.as_str(), endpoint.key))
        .collect::<Vec<_>>();

    assert_eq!(shown, [("keyed", true), ("open", false)]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_endpoint_forgets_its_key() {
    let realm = Realm::start().await;
    let both = [
        ("mine", "https://old.invalid"),
        ("kept", "https://k.invalid"),
    ];
    assert_eq!(chief(&realm, endpoints(&both)).await, Done::Done);
    keep(&realm, "mine", key("sk-old-server")).await;
    keep(&realm, "kept", key("sk-kept")).await;

    let removed = chief(&realm, endpoints(&both[1..])).await;
    let again = chief(
        &realm,
        endpoints(&[("mine", "https://new.invalid"), both[1]]),
    )
    .await;

    assert_eq!([removed, again], [Done::Done, Done::Done]);
    assert_eq!(
        credential(&realm, Hand::NUMBER, "mine").await,
        CredentialAnswer::Refused {
            why: CredentialRefusal::NotSignedIn {
                provider: "mine".to_owned()
            }
        }
    );
    assert_eq!(
        credential(&realm, Hand::NUMBER, "kept").await,
        CredentialAnswer::Key {
            key: "sk-kept".to_owned(),
            env: Settings::new(),
        }
    );
    assert_eq!(stored(&realm)["models"].get("mine"), None);
    let list: SecretList =
        realm.call_as_workspace(&Secrets).await.unwrap();
    assert!(
        list.values.iter().any(|held| held == "sk-old-server"),
        "a forgotten key is still taken out"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_realm_whose_credentials_are_damaged_says_why_it_did_not_start()
 {
    let scratch = Scratch::new("realm-damaged-credentials");
    let key = "sk-made-up-in-a-damaged-file";
    std::fs::write(
        scratch.join("credentials.json"),
        format!(r#"{{"version": "{key}"}}"#),
    )
    .unwrap();
    let host = open(&scratch, &Arc::new(Seen::default()), None).await;

    let started =
        host.get_or_start(&address::Realm.address().unwrap()).await;

    let error = started.unwrap_err();
    let extracted = ActorError::extract(&error);
    assert_eq!(
        (extracted.group(), extracted.code()),
        ("realm", "not_started")
    );
    assert!(
        extracted.message().contains("reading credentials.json"),
        "{}",
        extracted.message()
    );
    let shown = format!(
        "{error:?} {} {:?}",
        extracted.message(),
        extracted.meta
    );
    assert!(!shown.contains(key), "{shown}");
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_credential_an_agent_keeps_is_in_the_realms_list_of_secrets()
 {
    let (realm, _) = hamlet_with(0).await;
    keep(&realm, "openai", key("sk-made-up-kept")).await;
    keep(
        &realm,
        "chatgpt",
        Kept::Oauth {
            login: login("made-up-first"),
        },
    )
    .await;
    let refreshed = KeepCredential {
        provider: "chatgpt".to_owned(),
        value: Kept::Oauth {
            login: login("made-up-second"),
        },
    };
    realm.tool(MAYOR, &refreshed).await.unwrap();

    let list: SecretList =
        realm.call_as_workspace(&Secrets).await.unwrap();

    for value in [
        "sk-made-up-kept",
        "made-up-first",
        "made-up-first-refresh",
        "made-up-second",
        "made-up-second-refresh",
    ] {
        assert!(
            list.values.iter().any(|held| held == value),
            "{value}"
        );
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kept_key_shows_as_removed_in_the_record_a_letter_and_a_note()
 {
    const KEY: &str = "sk-made-up-0451";
    let (realm, hamlet) = hamlet_with(1).await;
    keep(&realm, "cloudflare", key(KEY)).await;
    let with_key = format!("use {KEY} for it");
    let removed = "use [REDACTED] for it";

    let renamed = chief(
        &realm,
        SetNickname {
            hire: to(THREE),
            name: Some("Spade".to_owned()),
            why: Some(with_key.clone()),
        },
    )
    .await;
    realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), &with_key)))
        .await
        .unwrap();
    let noted = chief(
        &realm,
        EditCatalogue {
            model: "faux::faux-1".to_owned(),
            set: vec![CardSetting::Note {
                note: with_key.clone(),
            }],
        },
    )
    .await;
    let record = chief(
        &realm,
        ReadRecord {
            hamlet: Some(hamlet),
        },
    )
    .await;
    let told = seen_submits(&realm, THREE, 1).await;
    let catalogue = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT value FROM document WHERE name = 'catalogue'",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
        })
        .await;

    assert_eq!((renamed, noted), (Done::Done, Done::Done));
    let Record::Read { lines } = record else {
        panic!("a record: {record:?}");
    };
    assert_eq!(
        lines.last().map(|line| line.text.as_str()),
        Some(
            format!("- chief nicknamed hire#3 (Spade): {removed}")
                .as_str()
        )
    );
    assert!(
        matches!(&told[0].input, Input::Letter { text, .. } if text == removed),
        "{told:?}"
    );
    let holds = |text: &str| {
        catalogue
            .windows(text.len())
            .any(|window| window == text.as_bytes())
    };
    assert!(holds(removed) && !holds(KEY));
}

const KEPT: &str = "sk-made-up-0452";

fn notices(realm: &Realm, agent: AgentNumber) -> Vec<String> {
    submits_to(realm, agent)
        .into_iter()
        .filter_map(|submit| match submit.input {
            Input::Notice { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_answer_reaches_the_agent_with_a_kept_key_removed() {
    let (realm, _) = hamlet_with(0).await;
    keep(&realm, "anthropic", key(KEPT)).await;
    let task = chief_task(&realm, Hand::NUMBER).await;
    let ask = AskChief {
        task,
        conversation: ConversationId("c".to_owned()),
        message: None,
        questions: vec![Question {
            question: "Which key?".to_owned(),
            description: None,
            options: Vec::new(),
            text_input: true,
        }],
    };
    realm.tool(Hand::NUMBER, &tool("a1", ask)).await.unwrap();

    let reply = format!("use {KEPT} please");
    let ruled = realm
        .ask(&answer("1", Choice::Reply, Some(&reply)))
        .await
        .unwrap();
    until("the Hand is told", || {
        !notices(&realm, Hand::NUMBER).is_empty()
    })
    .await;
    let kept = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT ruling FROM wait WHERE id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
        })
        .await;

    assert_eq!(ruled, Done::Done);
    assert_eq!(
        notices(&realm, Hand::NUMBER),
        ["The chief answers: use [REDACTED] please"]
    );
    assert_eq!(kept.as_deref(), Some("use [REDACTED] please"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_line_reaches_the_agent_and_its_task_with_a_kept_key_removed()
 {
    let (realm, _) = hamlet_with(0).await;
    keep(&realm, "anthropic", key(KEPT)).await;

    realm
        .ask(&say(
            &RequestId::new(),
            MAYOR,
            &format!("deploy with {KEPT}"),
        ))
        .await
        .unwrap();
    let given = seen_submits(&realm, MAYOR, 1).await;
    let kept = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT text FROM task WHERE text IS NOT NULL",
                [],
                |row| row.get::<_, String>(0),
            )
        })
        .await;

    assert!(
        matches!(&given[0].input, Input::Line { text, .. } if text == "deploy with [REDACTED]"),
        "{given:?}"
    );
    assert_eq!(kept, "deploy with [REDACTED]");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_worker_brief_reaches_the_worker_with_a_kept_key_removed()
 {
    let (realm, hamlet) = hamlet_with(0).await;
    keep(&realm, "anthropic", key(KEPT)).await;

    let started = chief(
        &realm,
        StartWorker {
            brief: format!("count with {KEPT}"),
            hamlet: Some(hamlet),
            tools: Vec::new(),
            task: None,
        },
    )
    .await;
    let input = made_until(&realm, THREE).await;

    assert!(
        matches!(started, Started::Started { .. }),
        "{started:?}"
    );
    assert_eq!(
        input.brief.map(|brief| brief.text).as_deref(),
        Some("count with [REDACTED]")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_kept_after_a_reason_is_removed_from_the_record_read() {
    let (realm, hamlet) = hamlet_with(1).await;
    let renamed = chief(
        &realm,
        SetNickname {
            hire: to(THREE),
            name: Some("Spade".to_owned()),
            why: Some(format!("use {KEPT} for it")),
        },
    )
    .await;
    keep(&realm, "anthropic", key(KEPT)).await;

    let read = ReadRecord {
        hamlet: Some(hamlet),
    };
    let by_the_hand = realm
        .tool(Hand::NUMBER, &tool("r1", read.clone()))
        .await
        .unwrap();
    let by_a_chief = chief(&realm, read).await;

    assert_eq!(renamed, Done::Done);
    for record in [by_the_hand, by_a_chief] {
        let Record::Read { lines } = record else {
            panic!("a record: {record:?}");
        };
        assert_eq!(
            lines.last().map(|line| line.text.as_str()),
            Some(
                "- chief nicknamed hire#3 (Spade): use [REDACTED] \
                 for it"
            )
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_keeps_the_credentials_files_version_and_the_keys_the_realm_does_not_own()
 {
    let mut file = stored_file();
    file["version"] = json!(5);
    let realm = restarted_on(Realm::start().await, &file).await;
    let realm = realm.restart().await;
    let untouched = stored(&realm);

    keep(&realm, "anthropic", key("sk-new")).await;
    let realm = realm.restart().await;

    assert_eq!(untouched, file);
    file["models"]["anthropic"] =
        json!({ "type": "api_key", "key": "sk-new" });
    file["version"] = json!(6);
    assert_eq!(stored(&realm), file);
    realm.host.shutdown().await.unwrap();
}

use std::path::PathBuf;

use hamlet_messages::realm::documents::{
    EditSettings, RemoteWorkspace, Setting, SettingRefusal,
};
use hamlet_messages::realm::models::{
    Credential, CredentialAnswer, CredentialRefusal, KeepCredential,
    Kept, OauthLogin,
};
use hamlet_messages::realm::{Done, SecretList, Secrets};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::SecretsVersion;
use serde_json::{Value, json};

use super::chiefs::chief;
use super::{MAYOR, Realm, THREE, hamlet_with, until};

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
    }
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

/// Writes `stored` as today's server would have left it, and starts
/// the realm again on it.
async fn moved_over(realm: Realm, stored: &Value) -> Realm {
    std::fs::write(file(&realm), stored.to_string()).unwrap();
    realm.restart().await
}

fn todays_file() -> Value {
    json!({
        "installationId": "a3c1",
        "connection": { "token": "connection-token" },
        "models": {
            "openai": { "type": "api_key", "key": "sk-today" },
            "chatgpt": {
                "type": "oauth",
                "access": "today-access",
                "refresh": "today-refresh",
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
async fn a_file_from_todays_server_keeps_what_the_realm_does_not_read()
 {
    let realm =
        moved_over(Realm::start().await, &todays_file()).await;

    keep(&realm, "anthropic", key("sk-new")).await;

    let mut expected = todays_file();
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

    let given = credential(&realm, AgentNumber(7), "anthropic").await;
    let unknown = credential(&realm, AgentNumber(7), "openai").await;

    assert_eq!(
        given,
        CredentialAnswer::Key {
            key: "sk-a".to_owned()
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
async fn a_file_from_todays_server_reads_as_an_agent_keeps_it() {
    let realm =
        moved_over(Realm::start().await, &todays_file()).await;

    let oauth = credential(&realm, Hand::NUMBER, "chatgpt").await;
    let api_key = credential(&realm, Hand::NUMBER, "openai").await;

    assert_eq!(
        oauth,
        CredentialAnswer::Oauth {
            login: OauthLogin {
                access: "today-access".to_owned(),
                refresh: "today-refresh".to_owned(),
                expires: 1_700_000_000,
                extra: Some(r#"{"accountId":"acc-1"}"#.to_owned()),
            }
        }
    );
    assert_eq!(
        api_key,
        CredentialAnswer::Key {
            key: "sk-today".to_owned()
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_reads_each_kept_value_under_a_raised_version()
{
    let realm =
        moved_over(Realm::start().await, &todays_file()).await;
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
        "sk-today",
        "today-access",
        "today-refresh",
    ] {
        assert!(
            first.values.iter().any(|held| held == value),
            "{value}"
        );
    }
    assert_eq!(second.version, SecretsVersion(2));
    assert!(second.values.iter().any(|held| held == "sk-second"));
    assert!(
        second.values.iter().any(|held| held == "sk-today"),
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
    // each workspace the chief chooses is kept in credentials.json,
    // under a raised version that every reach asks for; the token it
    // replaced is still taken out, and a removed one leaves the file.
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
async fn a_file_from_todays_server_keeps_its_workspace() {
    let realm =
        moved_over(Realm::start().await, &todays_file()).await;

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
    std::fs::write(file(&realm), todays_file().to_string()).unwrap();
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
        moved_over(Realm::start().await, &todays_file()).await;
    let CredentialAnswer::Oauth { login } =
        credential(&realm, Hand::NUMBER, "chatgpt").await
    else {
        panic!("today's login reads as one");
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

//! Models: trying the chosen model and the connection, the grants,
//! and signing in with a key.

use hamlet_acceptance::*;
use hamlet_protocol::v0::*;
use serde_json::json;

fn links(c: &Client, from: usize) -> Vec<LinkState> {
    c.messages()[from..]
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Change(c) => Some(c.changes.iter()),
            _ => None,
        })
        .flatten()
        .filter_map(|u| match u {
            Update::Settings { settings } => {
                Some(settings.link.state.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn tries_the_model_and_the_connection_and_keeps_the_link() {
    let mut s =
        Session::start(Script::new().responses([words("Hello.")]))
            .await;
    assert_eq!(
        s.chief.realm().settings.link.state,
        LinkState::Untested
    );
    let from = s.chief.messages().len();
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Check(Check::Model { id, model: None })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "Hello.")
    );
    s.chief
        .until("the link connected", |c| {
            c.realm().settings.link.state == LinkState::Connected
        })
        .await;
    assert_eq!(
        links(&s.chief, from),
        [LinkState::Testing, LinkState::Connected]
    );
    let reply = s
        .chief
        .request(|id| ClientMessage::Check(Check::Connection { id }))
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "Connected, 1 model(s)")
    );
    let late = s.join().await.realm().settings.link;
    assert_eq!(
        (late.state, late.models),
        (LinkState::Connected, Some(1))
    );
}

#[tokio::test]
async fn says_plainly_when_the_model_answers_no_hello() {
    let mut s =
        Session::start(Script::new().responses([words("")])).await;
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Check(Check::Model {
                id,
                model: Some("faux::faux-1".into()),
            })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (
            ReplyKind::Notice,
            "That model did not answer a hello. Pick another one. \
             (Error: the model answered with nothing)"
        )
    );
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Check(Check::Model {
                id,
                model: Some("missing".into()),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Error, "{reply:?}");
}

#[tokio::test]
async fn sends_grants_from_limits_and_from_answers() {
    let mut s = Session::start(Script::new().responses([
        call(
            "bash",
            json!({"command": "echo once", "network": true}),
        ),
        words("Done."),
    ]))
    .await;
    let reply = s
        .chief
        .limit("home", "permissions", "unconfined", json!(["mayor"]))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief.tell(mayor(), "Once.").await;
    let wait = s.chief.wait_for("the network wait", |_| true).await;
    let reply = s
        .chief
        .choose(&realm_wait_id(&wait), Choice::Always, None)
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief.idle("home", "founder").await;
    let grants = s.join().await.realm().grants;
    let founder = AgentRef {
        hamlet: "home".into(),
        number: 0,
    };
    assert_eq!(
        grants,
        [
            Grant {
                agent: Some(founder.clone()),
                by: Some("chief".into()),
                name: "mayor".into(),
                permission: Permission::Network,
                source: GrantSource::Grant,
            },
            Grant {
                agent: Some(founder),
                by: None,
                name: "mayor".into(),
                permission: Permission::Unconfined,
                source: GrantSource::Limits,
            },
        ]
    );
}

fn realm_wait_id(wait: &Wait) -> String {
    hamlet_acceptance::realm::wait_id(wait).to_string()
}

async fn start_login(
    c: &mut Client,
    provider: &str,
) -> LoginProgress {
    let start = LoginStartStart {
        method: LoginMethod::Key,
        provider: provider.into(),
    };
    match c
        .ask(|id| ClientMessage::Login(Login::Start { id, start }))
        .await
    {
        ServerMessage::Login(progress) => progress,
        other => panic!("a login start got {other:?}"),
    }
}

/// Answers the login's prompt with `value`, and gives its last step.
async fn finish_login(
    c: &mut Client,
    login: &str,
    value: &str,
) -> LoginStep {
    let reply = LoginReplyReply {
        login: login.into(),
        value: value.into(),
    };
    c.send(ClientMessage::Login(Login::Reply { id: id(), reply }))
        .await;
    let ended = |c: &Client| {
        c.messages().iter().find_map(|m| match m {
            ServerMessage::Login(p)
                if p.login == login
                    && matches!(
                        p.step,
                        LoginStep::Done { .. }
                            | LoginStep::Failed { .. }
                    ) =>
            {
                Some(p.step.clone())
            }
            _ => None,
        })
    };
    c.until("the login's end", |c| ended(c).is_some()).await;
    ended(c).unwrap()
}

fn configured(c: &Client, provider: &str) -> bool {
    c.realm()
        .providers
        .into_iter()
        .any(|p| p.id == provider && p.configured)
}

#[tokio::test]
async fn signs_in_with_a_key_without_sending_it_to_any_client() {
    let mut endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    let mut other = s.join().await;
    let first = start_login(&mut s.chief, "local").await;
    assert_eq!(
        first.step,
        LoginStep::Prompt {
            prompt: "Paste your key.".into(),
            secret: true
        }
    );
    let key = "sk-acceptance-\"private";
    let done = finish_login(&mut s.chief, &first.login, key).await;
    assert_eq!(
        done,
        LoginStep::Done {
            done: "local".into()
        }
    );
    s.chief
        .until("the endpoint's key", |c| {
            c.realm()
                .settings
                .endpoints
                .iter()
                .any(|e| e.name == "local" && e.key)
        })
        .await;
    // The endpoint gets the key with the next request.
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Check(Check::Model { id, model: None })
        })
        .await;
    assert_eq!(reply.text, "Hello.");
    let bearer = format!("Bearer {key}");
    let keys = endpoint
        .requests()
        .iter()
        .filter(|r| r.path.ends_with("/chat/completions"))
        .map(|r| r.authorization.clone())
        .collect::<Vec<_>>();
    assert_eq!(keys.last(), Some(&Some(bearer)), "{keys:?}");
    let openai = start_login(&mut s.chief, "openai").await;
    let done =
        finish_login(&mut s.chief, &openai.login, "sk-openai-own")
            .await;
    assert_eq!(
        done,
        LoginStep::Done {
            done: "openai".into()
        }
    );
    other
        .until("openai configured elsewhere", |c| {
            configured(c, "openai")
        })
        .await;
    for c in [&s.chief, &other] {
        let seen = format!("{:?}", c.messages());
        assert!(!seen.contains("sk-acceptance"), "a key was sent");
        assert!(!seen.contains("sk-openai-own"), "a key was sent");
    }
    let stored =
        std::fs::read_to_string(s.home.file("credentials.json"))
            .unwrap();
    let stored =
        serde_json::from_str::<serde_json::Value>(&stored).unwrap();
    assert_eq!(stored["models"]["openai"]["key"], "sk-openai-own");
    // A login answers only the connection that started it.
    let again = start_login(&mut s.chief, "openai").await;
    let reply = LoginReplyReply {
        login: again.login.clone(),
        value: "sk-other".into(),
    };
    let foreign = other
        .request(|id| {
            ClientMessage::Login(Login::Reply { id, reply })
        })
        .await;
    assert_eq!(
        (foreign.kind, foreign.text.as_str()),
        (ReplyKind::Error, "There is no sign-in here with that id.")
    );
    let cancelled = s
        .chief
        .request(|id| {
            ClientMessage::Login(Login::Cancel {
                cancel: again.login.clone(),
                id,
            })
        })
        .await;
    assert_eq!(
        (cancelled.kind, cancelled.text.as_str()),
        (ReplyKind::Notice, "Sign-in cancelled.")
    );
    let unknown = start_login(&mut s.chief, "nowhere").await;
    assert_eq!(
        unknown.step,
        LoginStep::Failed {
            failed: "Unknown provider: nowhere".into()
        }
    );
    // Taking the endpoint away forgets its key.
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    model: None,
                    reflect_every: None,
                    allow_net: None,
                    endpoints: Some(vec![]),
                    workspace: None,
                },
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("no endpoints", |c| {
            c.realm().settings.endpoints.is_empty()
        })
        .await;
    let stored =
        std::fs::read_to_string(s.home.file("credentials.json"))
            .unwrap();
    let stored =
        serde_json::from_str::<serde_json::Value>(&stored).unwrap();
    assert!(
        stored["models"].get("local").is_none(),
        "a key was kept"
    );
    assert_eq!(stored["models"]["openai"]["key"], "sk-openai-own");
}

#[tokio::test]
async fn lists_prices_and_bills_an_endpoints_models() {
    let mut endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    let ServerMessage::Read(ReadResult::Models { models, .. }) =
        s.chief.read(|id| Read::Models { id, refresh: None }).await
    else {
        panic!("a models read")
    };
    let large = models.iter().find(|m| m.name == "local::org/large");
    assert_eq!(
        large.and_then(|m| m.context),
        Some(32000),
        "{large:?}"
    );
    let set = EditCatalogueSet {
        allowed: Some(true),
        note: Some("For careful work.".into()),
        price: Some(Some(Price {
            input: 2.0,
            output: 3.0,
        })),
    };
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Catalogue {
                id,
                model: "local::org/large".into(),
                set,
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    let ServerMessage::Read(ReadResult::Models { models, .. }) =
        s.chief.read(|id| Read::Models { id, refresh: None }).await
    else {
        panic!("a models read")
    };
    let large =
        models.into_iter().find(|m| m.name == "local::org/large");
    assert_eq!(
        large,
        Some(Listed {
            allowed: true,
            context: Some(32000),
            name: "local::org/large".into(),
            note: "For careful work.".into(),
            price: Some(Price {
                input: 2.0,
                output: 3.0
            }),
            priced: ListedPriced::Yours,
        })
    );
    let reply = s.chief.say(mayor(), "Hello").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief.idle("home", "founder").await;
    // The endpoint's own bill wins over the catalogue's price.
    let spent = s.chief.agent("home", "founder").unwrap().spent;
    assert_eq!(
        (spent.micros, spent.tokens),
        ((openai::COST * 1e6) as i64, 120)
    );
    let page = s.chief.open(&chat(mayor())).await;
    let texts = page
        .items
        .iter()
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Hello", "Hello."]);
    let asked = endpoint
        .requests()
        .iter()
        .filter_map(|r| {
            r.body.as_ref()?.get("model")?.as_str().map(String::from)
        })
        .collect::<Vec<_>>();
    assert_eq!(asked, ["org/large"]);
}

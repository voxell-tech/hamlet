//! Models: trying the chosen model and the connection, signing in
//! with a key, and an endpoint's listing, prices and refusals.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::openai;

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
    let endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
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

async fn start_login(
    c: &mut Client,
    provider: &str,
) -> LoginProgress {
    let start = LoginStart {
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
    let reply = LoginReply {
        login: login.into(),
        value: value.into(),
    };
    c.send(ClientMessage::Login(Login::Reply {
        id: RequestId::new(),
        reply,
    }))
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
    let reply = LoginReply {
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
        (ReplyKind::Error, "No sign-in of yours is open.")
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
    s.chief.until("the cancelled login's failed step", |c| c.messages().iter().any(|m| matches!(m, ServerMessage::Login(p) if p.login == again.login && matches!(&p.step, LoginStep::Failed { failed } if !failed.is_empty())))).await;
    let unknown = start_login(&mut s.chief, "nowhere").await;
    assert_eq!(
        unknown.step,
        LoginStep::Failed {
            failed: "Unknown provider: nowhere".into()
        }
    );
    let credentials = || {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(s.home.file("credentials.json"))
                .unwrap(),
        )
        .unwrap()
    };
    let endpoints = |names: &[&str]| {
        names
            .iter()
            .map(|name| Endpoint {
                name: (*name).into(),
                base_url: endpoint.url.clone(),
            })
            .collect::<Vec<_>>()
    };
    let edit = |endpoints: Vec<Endpoint>| {
        move |id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    model: None,
                    character: None,
                    effort: None,
                    newcomers: None,
                    reflect_every: None,
                    allow_net: None,
                    endpoints: Some(endpoints),
                    workspace: None,
                },
            })
        }
    };
    // Taking an endpoint away forgets its key.
    let reply =
        s.chief.request(edit(endpoints(&["local", "spare"]))).await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    let spare = start_login(&mut s.chief, "spare").await;
    let done =
        finish_login(&mut s.chief, &spare.login, "sk-spare").await;
    assert_eq!(
        done,
        LoginStep::Done {
            done: "spare".into()
        }
    );
    assert_eq!(credentials()["models"]["spare"]["key"], "sk-spare");
    let reply = s.chief.request(edit(endpoints(&["local"]))).await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("only local", |c| {
            c.realm().settings.endpoints.len() == 1
        })
        .await;
    let stored = credentials();
    assert!(stored["models"].get("spare").is_none(), "{stored}");
    assert_eq!(stored["models"]["local"]["key"], key);
    assert_eq!(stored["models"]["openai"]["key"], "sk-openai-own");
}

#[tokio::test]
async fn lists_prices_and_bills_an_endpoints_models() {
    let mut endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    // The listing arrives once the server has asked the endpoint.
    for _ in 0..100 {
        let ServerMessage::Read(ReadResult::Models {
            models, ..
        }) = s
            .chief
            .read(|id| Read::Models { id, refresh: None })
            .await
        else {
            panic!("a models read")
        };
        let large =
            models.iter().find(|m| m.name == "local::org/large");
        if large.and_then(|m| m.context) == Some(32000) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100))
            .await;
    }
    let set = EditCatalogueSet {
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
            context: Some(32000),
            default_effort: Some(hamlet_protocol::Effort::Off),
            efforts: vec![hamlet_protocol::Effort::Off],
            fast: false,
            name: "local::org/large".into(),
            note: "For careful work.".into(),
            price: Some(Price {
                input: 2.0,
                output: 3.0
            }),
            priced: ListedPriced::Yours,
            title: "org/large".into(),
        })
    );
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Hello").await;
    s.chief.shows(&now, "Hello.").await;
    s.chief.idle("hand").await;
    // The endpoint's own bill wins over the catalogue's price.
    let spent = s.chief.agent("hand").unwrap().spent;
    assert_eq!(
        (spent.micros, spent.tokens),
        ((openai::COST * 1e6) as i64, 120)
    );
    assert_eq!(s.chief.texts(&now), ["Hello", "Hello."]);
    let asked = endpoint
        .requests()
        .iter()
        .filter_map(|r| {
            r.body.as_ref()?.get("model")?.as_str().map(String::from)
        })
        .collect::<Vec<_>>();
    assert_eq!(asked, ["org/large"]);
}

#[tokio::test]
async fn gives_plain_words_for_http_refusals_and_charges_nothing() {
    let endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    for (status, expected) in [
        (
            401,
            "That key was not accepted. Check it was copied whole \
             and is still active.",
        ),
        (
            402,
            "Your provider account has no credit left. Add some, \
             then try again.",
        ),
        (
            403,
            "That key was not accepted. Check it was copied whole \
             and is still active.",
        ),
    ] {
        endpoint.enqueue([openai::Response {
            status,
            text: "nope".into(),
            ..Default::default()
        }]);
        let reply = s
            .chief
            .request(|id| {
                ClientMessage::Check(Check::Model { id, model: None })
            })
            .await;
        assert_eq!(reply.text, expected);
        let link = s.join().await.realm().settings.link;
        assert_eq!(link.state, LinkState::Failed);
        assert_eq!(link.words.as_deref(), Some(expected));
    }
    endpoint.enqueue([openai::Response {
        status: 401,
        ..Default::default()
    }]);
    let hand = s.hand();
    s.chief.tell(hand, "Fail.").await;
    s.chief
        .wait_for("the authentication fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    let spent = s.join().await.agent("hand").unwrap().spent;
    assert_eq!((spent.micros, spent.tokens), (0, 0));
}

#[tokio::test]
async fn the_faux_model_answers_every_agents_check() {
    let mut s = Session::start(Faux::new()).await;
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Check(Check::Model { id, model: None })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "Every agent runs the faux model here.")
    );
}

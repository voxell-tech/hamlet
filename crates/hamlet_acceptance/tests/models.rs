//! Models: trying the chosen model and the connection, the grants,
//! and signing in with a key.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{Home, Script, call, openai, words};
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
    let reply = s.chief.choose(&wait.id, Choice::Always, None).await;
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
                    reflect_every: None,
                    allow_net: None,
                    endpoints: Some(endpoints),
                    workspace: None,
                },
            })
        }
    };
    // HAMLET_BASE_URL is for `local`, so it cannot be taken away.
    let before = (s.chief.realm().settings, credentials());
    let reply = s.chief.request(edit(Vec::new())).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (
            ReplyKind::Error,
            "HAMLET_BASE_URL is for the endpoint of the model \
             `local::org/large`, which the settings do not list. \
             Add the endpoint in Settings, or unset HAMLET_BASE_URL."
        )
    );
    assert_eq!((s.chief.realm().settings, credentials()), before);
    // Taking another endpoint away forgets its key.
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
    assert!(
        stored["models"].get("spare").is_none(),
        "a key was kept"
    );
    assert_eq!(stored["models"]["local"]["key"], key);
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

#[tokio::test]
async fn chats_with_the_model_chosen_in_settings_when_none_is_given()
{
    let mut endpoint = openai::Endpoint::start().await;
    let mut home = Home::new();
    let server =
        home.serve_providers(&[("HAMLET_REFLECT_EVERY", "0")]).await;
    let mut chief = Client::join(&server).await;
    let reply = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    model: Some("local::org/large".into()),
                    reflect_every: None,
                    allow_net: None,
                    endpoints: Some(vec![Endpoint {
                        name: "local".into(),
                        base_url: endpoint.url.clone(),
                    }]),
                    workspace: None,
                },
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    let reply = chief.say(mayor(), "Hello").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    chief.idle("home", "founder").await;
    let page = chief.open(&chat(mayor())).await;
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

#[tokio::test]
async fn gives_plain_words_for_http_refusals_and_charges_nothing() {
    let endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    for (status, expected) in [
        (
            401,
            "That key was not accepted. Check that you copied all \
             of it and that it is still active.",
        ),
        (
            402,
            "Your provider account has no credit left. Add some, \
             then try again.",
        ),
        (
            403,
            "That key was not accepted. Check that you copied all \
             of it and that it is still active.",
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
    s.chief.say(mayor(), "Fail.").await;
    s.chief
        .wait_for("the authentication fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    let spent =
        s.join().await.agent("home", "founder").unwrap().spent;
    assert_eq!((spent.micros, spent.tokens), (0, 0));
}

#[tokio::test]
async fn retries_a_503_and_charges_only_the_answer() {
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.enqueue([openai::Response {
        status: 503,
        text: "temporarily unavailable".into(),
        ..Default::default()
    }]);
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "Hello.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert_eq!(page.items.last().unwrap().text, "Hello.");
    let requests = endpoint.completions(2).await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body);
    let spent =
        s.join().await.agent("home", "founder").unwrap().spent;
    assert_eq!((spent.micros, spent.tokens), (1500, 120));
    assert!(s.chief.waits().is_empty());
}

#[tokio::test]
async fn reads_fixed_usage_and_preserves_an_explicit_zero_bill() {
    let endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    assert_eq!(
        s.chief.open(&chat(mayor())).await.usage.summary,
        "No model calls yet"
    );
    endpoint.enqueue([openai::Response { usage: Some(json!({"prompt_tokens": 17000, "completion_tokens": 1240, "cost": 0.4249})), ..Default::default() }]);
    s.chief.say(mayor(), "Count.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert_eq!(
        (
            page.usage.calls,
            page.usage.input,
            page.usage.output,
            page.usage.micros
        ),
        (1, 17000, 1240, 424900)
    );
    assert_eq!(page.usage.summary, "18.2k tok, $0.42");
    assert!(s.chief.operations(&chat(mayor())).iter().any(|operation| matches!(operation, TranscriptOperation::Usage { usage } if usage.summary == page.usage.summary)));
    s.chief
        .request(|id| {
            ClientMessage::Edit(Edit::Catalogue {
                id,
                model: "local::org/large".into(),
                set: EditCatalogueSet {
                    allowed: None,
                    note: None,
                    price: Some(Some(Price {
                        input: 10.0,
                        output: 20.0,
                    })),
                },
            })
        })
        .await;
    endpoint.enqueue([openai::Response { usage: Some(json!({"prompt_tokens": 100, "completion_tokens": 20, "cost": 0})), ..Default::default() }]);
    s.chief.say(mayor(), "Free.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert_eq!((page.usage.calls, page.usage.micros), (2, 424900));
}

async fn listing(c: &mut Client, refresh: bool) -> Vec<Listed> {
    match c
        .read(|id| Read::Models {
            id,
            refresh: Some(refresh),
        })
        .await
    {
        ServerMessage::Read(ReadResult::Models {
            models, ..
        }) => models,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn keeps_catalogue_edits_when_listings_change_and_the_server_restarts()
 {
    let endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief
        .request(|id| {
            ClientMessage::Edit(Edit::Catalogue {
                id,
                model: "local::org/large".into(),
                set: EditCatalogueSet {
                    allowed: Some(true),
                    note: Some("Careful work".into()),
                    price: Some(Some(Price {
                        input: 2.0,
                        output: 3.0,
                    })),
                },
            })
        })
        .await;
    endpoint.list(json!([{"id":"org/large", "context_length":64000}, {"id":"small", "context_length":16000}]));
    let refreshed = listing(&mut s.chief, true).await;
    let large = refreshed
        .iter()
        .find(|m| m.name == "local::org/large")
        .unwrap();
    assert_eq!(
        (
            large.context,
            large.allowed,
            large.note.as_str(),
            &large.priced
        ),
        (Some(64000), true, "Careful work", &ListedPriced::Yours)
    );
    assert_eq!(
        large.price,
        Some(Price {
            input: 2.0,
            output: 3.0
        })
    );
    assert!(refreshed.iter().any(|m| m.name == "local::small"));
    assert_eq!(s.server.stop().await, Some(0));
    let server = s
        .home
        .serve_providers(&[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", &endpoint.url),
        ])
        .await;
    let mut chief = Client::join(&server).await;
    assert_eq!(listing(&mut chief, false).await, refreshed);
    endpoint.list(json!([{"id":"org/large", "context_length":64000}, {"id":"newcomer"}]));
    let ended =
        Cli::run(s.home.path(), &["models", "refresh"], "", &[])
            .await;
    assert_eq!(ended.code(), Some(0), "{}", ended.err);
    assert!(
        listing(&mut chief, false)
            .await
            .iter()
            .any(|m| m.name == "local::newcomer")
    );
}

#[tokio::test]
async fn the_chiefs_pin_wins_and_survives_a_held_request_and_restart()
{
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.list(json!([{"id":"org/large", "context_length":32000}, {"id":"small", "context_length":16000}]));
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "/found mill miller Work").await;
    let target = to("mill", "mayor");
    s.chief.say(target.clone(), "Default.").await;
    s.chief
        .request(|id| {
            ClientMessage::Edit(Edit::Limits(EditLimits::Whole {
                id,
                hamlet: "mill".into(),
                reason: None,
                whole: [("model".into(), json!("local::small"))]
                    .into(),
            }))
        })
        .await;
    s.chief.say(target.clone(), "Hamlet model.").await;
    assert_eq!(
        s.chief
            .limit(
                "mill",
                "pinned",
                "mayor",
                json!("local::org/large")
            )
            .await
            .kind,
        ReplyKind::Notice
    );
    endpoint.enqueue([openai::Response {
        hold_until: Some(s.home.side().join("release")),
        ..Default::default()
    }]);
    s.chief.tell(target.clone(), "Pinned and held.").await;
    let requests = endpoint.completions(3).await;
    assert_eq!(
        requests
            .iter()
            .map(|r| r.body.as_ref().unwrap()["model"]
                .as_str()
                .unwrap())
            .collect::<Vec<_>>(),
        ["org/large", "small", "org/large"]
    );
    s.server.kill();
    let server = s
        .home
        .serve_providers(&[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::small"),
            ("HAMLET_BASE_URL", &endpoint.url),
        ])
        .await;
    let mut chief = Client::join(&server).await;
    let requests = endpoint.completions(4).await;
    assert_eq!(
        requests[3].body.as_ref().unwrap()["model"],
        "org/large"
    );
    chief.open(&chat(target.clone())).await;
    chief
        .until("recovered answer", |c| {
            c.items(&chat(target.clone()))
                .iter()
                .filter(|i| i.kind == ItemKind::Assistant)
                .count()
                == 3
        })
        .await;
    chief.idle("mill", "miller").await;
    let spent = Client::join(&server)
        .await
        .agent("mill", "miller")
        .unwrap()
        .spent;
    assert_eq!((spent.micros, spent.tokens), (4500, 360));
    let record = chief.prompt(target.clone()).await;
    assert!(
        record.contains("local::small")
            && record.contains("local::org/large"),
        "{record}"
    );
    chief.limit("mill", "pinned", "mayor", json!(null)).await;
    chief.say(target.clone(), "Unpinned.").await;
    assert_eq!(
        endpoint.completions(5).await[4].body.as_ref().unwrap()
            ["model"],
        "small"
    );
    let release = s.home.side().join("round");
    let mut held = endpoint_call("read", json!({"path":"NOTES.md"}));
    held.hold_until = Some(release.clone());
    endpoint.enqueue([held]);
    let request = chief.tell(target, "Change between rounds.").await;
    assert_eq!(
        endpoint.completions(6).await[5].body.as_ref().unwrap()
            ["model"],
        "small"
    );
    chief
        .limit("mill", "pinned", "mayor", json!("local::org/large"))
        .await;
    std::fs::write(release, "go").unwrap();
    chief.reply(&request).await;
    assert_eq!(
        endpoint.completions(7).await[6].body.as_ref().unwrap()
            ["model"],
        "org/large"
    );
}

#[tokio::test]
async fn scrubs_successful_and_failed_hellos_before_publishing_the_link()
 {
    let endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    let login = start_login(&mut s.chief, "local").await;
    let secret = "acceptance-private-hello";
    finish_login(&mut s.chief, &login.login, secret).await;
    for status in [200, 400] {
        endpoint.enqueue([openai::Response {
            status,
            text: format!("Hello {secret}"),
            ..Default::default()
        }]);
        let reply = s
            .chief
            .request(|id| {
                ClientMessage::Check(Check::Model { id, model: None })
            })
            .await;
        assert!(reply.text.contains("[REDACTED]"), "{reply:?}");
        let late = s.join().await;
        assert!(!format!("{:?}", late.messages()).contains(secret));
        assert!(
            !format!("{:?}", s.chief.messages()).contains(secret)
        );
    }
}

#[tokio::test]
async fn applies_allowed_model_choices_and_refuses_a_pinned_choice() {
    let mut s = Session::start(Script::new().models(&["faux-1", "faux-2"]).responses([
        call("create_agent", json!({"name":"scout","purpose":"Explore","reason":"Needed","character":"Keep careful notes."})), words("Created."),
        call("set_model", json!({"name":"scout","model":"faux::faux-2","reason":"Careful work"})), words("Refused."),
        call("set_model", json!({"name":"scout","model":"faux::faux-2","reason":"Careful work"})), words("Chosen."),
        call("set_model", json!({"name":"scout","model":"faux::faux-2","reason":"Try past pin"})), words("Pinned."),
    ])).await;
    s.chief.say(mayor(), "Hire scout.").await;
    assert_eq!(
        s.chief.papers(to("home", "scout")).await.character,
        "Keep careful notes."
    );
    s.chief.say(mayor(), "Choose a forbidden model.").await;
    assert_eq!(
        s.join().await.agent("home", "scout").unwrap().model,
        "faux::faux-1"
    );
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Limits(EditLimits::Whole {
                id,
                hamlet: "home".into(),
                reason: None,
                whole: [("models".into(), json!(["faux::faux-2"]))]
                    .into(),
            }))
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief.say(mayor(), "Choose an allowed model.").await;
    assert_eq!(
        s.join().await.agent("home", "scout").unwrap().model,
        "faux::faux-2"
    );
    s.chief
        .limit("home", "pinned", "scout", json!("faux::faux-1"))
        .await;
    s.chief.say(mayor(), "Try changing a pinned model.").await;
    assert_eq!(
        s.join().await.agent("home", "scout").unwrap().model,
        "faux::faux-1"
    );
    s.chief.limit("home", "pinned", "scout", json!(null)).await;
    assert_eq!(
        s.join().await.agent("home", "scout").unwrap().model,
        "faux::faux-2"
    );
    s.chief
        .request(|id| {
            ClientMessage::Edit(Edit::Limits(EditLimits::Whole {
                id,
                hamlet: "home".into(),
                reason: None,
                whole: [("models".into(), json!([]))].into(),
            }))
        })
        .await;
    assert_eq!(
        s.join().await.agent("home", "scout").unwrap().model,
        "faux::faux-1"
    );
}

fn endpoint_call(
    name: &str,
    args: serde_json::Value,
) -> openai::Response {
    openai::Response {
        text: String::new(),
        finish: "tool_calls".into(),
        calls: vec![
            json!({"index":0,"id":"tool-1","type":"function","function":{"name":name,"arguments":args.to_string()}}),
        ],
        ..Default::default()
    }
}

fn system(request: &openai::Request) -> String {
    request.body.as_ref().unwrap()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "system")
        .map(|m| m["content"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[tokio::test]
async fn sends_the_displayed_prompt_and_holds_budget_words_for_one_turn()
 {
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.enqueue([
        endpoint_call("read", json!({"path":"NOTES.md"})),
        openai::Response::default(),
        openai::Response::default(),
    ]);
    let mut s = Session::with_endpoint(&endpoint).await;
    let shown = s.chief.prompt(mayor()).await;
    s.chief.say(mayor(), "Read.").await;
    let first = endpoint.completions(2).await;
    assert_eq!(system(&first[0]), shown);
    let budget = |text: String| {
        text.split("# Budget\n")
            .nth(1)
            .unwrap()
            .split("\n# ")
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(budget(system(&first[0])), budget(system(&first[1])));
    assert!(budget(shown).contains("the hamlet's own model"));
    s.chief.say(mayor(), "Next turn.").await;
    let third = endpoint.completions(3).await;
    assert_ne!(budget(system(&third[2])), budget(system(&third[0])));
}

async fn cutoff(twice: bool, summary: &str) {
    let mut endpoint = openai::Endpoint::start().await;
    let cut = openai::Response {
        text: String::new(),
        finish: "length".into(),
        ..Default::default()
    };
    endpoint.enqueue([
        openai::Response {
            text: "x".repeat(100_000),
            ..Default::default()
        },
        openai::Response {
            text: "Earlier.".into(),
            ..Default::default()
        },
        cut.clone(),
        openai::Response {
            text: summary.into(),
            ..Default::default()
        },
        if twice {
            cut
        } else {
            openai::Response {
                text: "There is room now.".into(),
                ..Default::default()
            }
        },
    ]);
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "Long.").await;
    s.chief.say(mayor(), "Earlier.").await;
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "Keep this\n  word for word.").await;
    if twice || summary.is_empty() {
        let wait = s
            .chief
            .wait_for("the cut-off fault", |w| {
                matches!(w.detail, WaitDetail::Fault { .. })
            })
            .await;
        let Wait { about, .. } = wait;
        assert_eq!(
            about,
            if summary.is_empty() {
                "the conversation is longer than the model can hold, \
                 and the model could not summarise it. Start a new \
                 session, or choose a model that holds more"
            } else {
                "the model has no room left to answer, even with the \
                 conversation summarised. Start a new session, or \
                 choose a model that holds more"
            }
        );
    } else {
        s.chief.shows(&chat(mayor()), "There is room now.").await;
    }
    s.chief.idle("home", "founder").await;
    let count = if summary.is_empty() { 4 } else { 5 };
    let requests = endpoint.completions(count).await;
    assert_eq!(requests.len(), count);
    assert!(system(&requests[3]).contains("summarization assistant"));
    if !summary.is_empty() {
        assert!(
            requests[4].body.as_ref().unwrap()["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "user"
                    && m["content"] == "Keep this\n  word for word.")
        );
        assert!(s.chief.texts(&chat(mayor())).iter().any(|t| t
            == "The conversation grew longer than the model can \
                hold, so the earlier part of it was summarised."));
    }
}

#[tokio::test]
async fn summarises_a_cutoff_and_retries_the_exact_line_once() {
    cutoff(false, "Summary.").await;
}

#[tokio::test]
async fn faults_when_a_summarised_reply_is_cut_off_again() {
    cutoff(true, "Summary.").await;
}

#[tokio::test]
async fn faults_when_the_model_cannot_summarise() {
    cutoff(false, "").await;
}

#[tokio::test]
async fn reports_a_cutoff_without_history_without_trying_a_summary() {
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.enqueue([openai::Response {
        text: String::new(),
        finish: "length".into(),
        ..Default::default()
    }]);
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "Too long.").await;
    let wait = s
        .chief
        .wait_for("a no-room fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    let Wait { about, .. } = wait;
    assert_eq!(
        about,
        "the message is longer than the model can hold"
    );
    assert_eq!(endpoint.completions(1).await.len(), 1);
}

#[tokio::test]
async fn summarises_at_eighty_percent_before_requesting_the_next_answer()
 {
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.enqueue([
        openai::Response { text: "x".repeat(100_000), ..Default::default() },
        openai::Response { text: "Earlier.".into(), usage: Some(json!({"prompt_tokens":26000,"completion_tokens":20,"cost":0.0015})), ..Default::default() },
        openai::Response { text: "Summary of earlier work.".into(), ..Default::default() },
        openai::Response { text: "Exact answer.".into(), ..Default::default() },
    ]);
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "Long.").await;
    s.chief.say(mayor(), "Earlier.").await;
    assert_eq!(endpoint.completions(2).await.len(), 2);
    s.chief.say(mayor(), "Keep this\n  spacing.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items.iter().any(|i| i.text == "Exact answer."),
        "{page:?}"
    );
    let requests = endpoint.completions(4).await;
    assert_eq!(requests.len(), 4);
    assert!(system(&requests[2]).contains("summarization assistant"));
    let messages = requests[3].body.as_ref().unwrap()["messages"]
        .as_array()
        .unwrap();
    assert!(messages.iter().any(|m| m["role"] == "user"
        && m["content"] == "Keep this\n  spacing."));
    assert!(
        messages.iter().any(|m| m
            .to_string()
            .contains("Summary of earlier work."))
    );
}

#[tokio::test]
async fn does_not_summarise_or_retry_a_cut_off_reflection() {
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.enqueue([
        openai::Response {
            text: "x".repeat(100_000),
            ..Default::default()
        },
        openai::Response::default(),
        openai::Response {
            text: String::new(),
            finish: "length".into(),
            ..Default::default()
        },
    ]);
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "Long.").await;
    s.chief.say(mayor(), "Earlier.").await;
    s.chief.say(mayor(), "/reflect").await;
    s.chief.idle("home", "founder").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items.iter().any(|i| i
            .text
            .contains("the model answered with nothing")),
        "{page:?}"
    );
    assert_eq!(endpoint.completions(3).await.len(), 3);
}

#[tokio::test]
async fn estimates_unreported_usage_from_history_and_tool_definitions()
 {
    let endpoint = openai::Endpoint::start().await;
    endpoint
        .list(json!([{"id":"org/large", "context_length":128000}]));
    endpoint.enqueue(vec![
        openai::Response {
            usage: None,
            ..Default::default()
        };
        2
    ]);
    let mut s = Session::with_endpoint(&endpoint).await;
    let tool = s.home.file("tools/large");
    std::fs::create_dir_all(&tool).unwrap();
    std::fs::write(tool.join("manifest.json"), json!({"name":"large_tool", "description":"t".repeat(40000), "exec":["true"]}).to_string()).unwrap();
    // The server reads the tools folder every two seconds.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    s.chief.say(mayor(), &"a".repeat(20000)).await;
    let first =
        s.join().await.agent("home", "founder").unwrap().spent;
    s.chief.say(mayor(), &"b".repeat(20000)).await;
    let second =
        s.join().await.agent("home", "founder").unwrap().spent;
    let usage = s.chief.open(&chat(mayor())).await.usage;
    assert_eq!((usage.calls, usage.unreported), (2, 2));
    assert_eq!((usage.input, usage.output), (0, 0));
    assert!(
        second.tokens - first.tokens > 20000,
        "{first:?} {second:?}"
    );
}

#[tokio::test]
async fn charges_the_librarian_to_its_callers_hamlet_and_task() {
    for budget in [None, Some(0.002)] {
        let mut endpoint = openai::Endpoint::start().await;
        endpoint.enqueue([
            endpoint_call(
                "ask_librarian",
                json!({"question":"Which skill reads a paper?"}),
            ),
            openai::Response {
                text: "Read the shelves.".into(),
                ..Default::default()
            },
            openai::Response {
                text: "Consulted.".into(),
                ..Default::default()
            },
        ]);
        let mut s = Session::with_endpoint(&endpoint).await;
        s.chief.say(mayor(), "/found mill miller Work").await;
        s.chief.limit("mill", "budget", "cost", json!(budget)).await;
        s.chief.limit("mill", "task", "cost", json!(0.002)).await;
        s.chief.say(to("mill", "mayor"), "Consult.").await;
        let state = s.join().await.realm();
        let home =
            state.hamlets.iter().find(|h| h.name == "home").unwrap();
        let mill =
            state.hamlets.iter().find(|h| h.name == "mill").unwrap();
        assert_eq!(home.budget.spent.micros, 0);
        assert_eq!(mill.budget.spent.micros, 3000);
        let page = s.chief.open(&chat(to("mill", "mayor"))).await;
        let reason = if budget.is_some() { "budget" } else { "task" };
        assert!(
            page.items.iter().any(|i| i.kind == ItemKind::Notice
                && i.text.contains(reason)),
            "{page:?}"
        );
        assert!(!page.items.iter().any(|i| i.text == "Consulted."));
        assert_eq!(endpoint.completions(2).await.len(), 2);
        s.chief.limit("mill", "budget", "cost", json!(null)).await;
        s.chief.say(to("mill", "mayor"), "New task.").await;
        let page = s.chief.open(&chat(to("mill", "mayor"))).await;
        assert!(page.items.iter().any(|i| i.text == "Consulted."));
    }
}

#[tokio::test]
async fn charges_a_summary_to_the_cut_off_task_not_the_queued_line() {
    let mut endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    let release = s.home.side().join("release");
    endpoint.enqueue([
        openai::Response { text: "x".repeat(100_000), ..Default::default() },
        openai::Response::default(),
        openai::Response { text: String::new(), finish: "length".into(), hold_until: Some(release.clone()), ..Default::default() },
        openai::Response { text: "Summary.".into(), usage: Some(json!({"prompt_tokens":100,"completion_tokens":20,"cost":0.004})), ..Default::default() },
        openai::Response { text: "The queued task answers.".into(), ..Default::default() },
    ]);
    s.chief.limit("home", "task", "cost", json!(0.005)).await;
    s.chief.say(mayor(), "Long.").await;
    s.chief.say(mayor(), "Earlier.").await;
    s.chief.open(&chat(mayor())).await;
    let first = s.chief.tell(mayor(), "Cut off.").await;
    endpoint.completions(3).await;
    let queued = s.chief.tell(mayor(), "Queued.").await;
    s.chief.until("the queued task", |c| c.messages().iter().any(|m| matches!(m, ServerMessage::Turn(t) if t.operations.iter().any(|o| matches!(o, TranscriptOperation::Status { pending, .. } if *pending == 1))))).await;
    std::fs::write(release, "").unwrap();
    s.chief.reply(&first).await;
    s.chief.reply(&queued).await;
    s.chief
        .until("the exhausted original task", |c| {
            c.texts(&chat(mayor()))
                .iter()
                .any(|t| t.contains("This task has spent its cap"))
        })
        .await;
    s.chief.idle("home", "founder").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items
            .iter()
            .any(|i| i.text.contains("This task has spent its cap")),
        "{page:?}"
    );
    assert!(
        page.items
            .iter()
            .any(|i| i.text == "The queued task answers.")
    );
    assert_eq!(page.usage.micros, 10000);
    let requests = endpoint.completions(5).await;
    assert_eq!(requests.len(), 5);
    assert!(system(&requests[3]).contains("summarization assistant"));
    assert!(requests[4].body.as_ref().unwrap()["messages"].as_array().unwrap().iter().any(|m| m["role"] == "user" && m["content"] == "Queued."));
}

#[tokio::test]
async fn lowering_the_budget_prevents_cutoff_compaction() {
    let mut endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    let release = s.home.side().join("release");
    endpoint.enqueue([
        openai::Response {
            text: "x".repeat(100_000),
            ..Default::default()
        },
        openai::Response::default(),
        openai::Response {
            text: String::new(),
            finish: "length".into(),
            hold_until: Some(release.clone()),
            ..Default::default()
        },
        openai::Response {
            text: "After the raise.".into(),
            ..Default::default()
        },
    ]);
    s.chief.say(mayor(), "Long.").await;
    s.chief.say(mayor(), "Earlier.").await;
    let first = s.chief.tell(mayor(), "Cut off.").await;
    endpoint.completions(3).await;
    s.chief.limit("home", "budget", "cost", json!(0)).await;
    std::fs::write(release, "").unwrap();
    s.chief.reply(&first).await;
    assert_eq!(endpoint.completions(3).await.len(), 3);
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items.iter().any(|i| i
            .text
            .contains("the model could not summarise it")),
        "{page:?}"
    );
    s.chief.limit("home", "budget", "cost", json!(5)).await;
    s.chief.say(mayor(), "Now answer.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert_eq!(page.items.last().unwrap().text, "After the raise.");
}

#[tokio::test]
async fn gives_only_a_resident_the_civic_reflection_question() {
    let mut endpoint = openai::Endpoint::start().await;
    endpoint.list(json!([{"id":"org/large"}, {"id":"small"}]));
    endpoint.enqueue([endpoint_call(
        "create_agent",
        json!({"name":"constructor", "purpose":"Build", "reason":"Needed"}),
    )]);
    let mut s = Session::with_endpoint(&endpoint).await;
    s.chief.say(mayor(), "Hire a builder.").await;
    s.chief.say(mayor(), "/reflect").await;
    s.chief.idle("home", "founder").await;
    s.chief.say(to("home", "constructor"), "/reflect").await;
    s.chief.idle("home", "constructor").await;
    let requests = endpoint.completions(4).await;
    let civic = "how the hamlet is run get in the way";
    assert!(
        !requests[2]
            .body
            .as_ref()
            .unwrap()
            .to_string()
            .contains(civic)
    );
    assert!(
        requests[3]
            .body
            .as_ref()
            .unwrap()
            .to_string()
            .contains(civic)
    );
    assert_eq!(
        requests[3].body.as_ref().unwrap()["model"],
        "org/large"
    );
    s.chief
        .limit("home", "pinned", "constructor", json!("local::small"))
        .await;
    s.chief.say(to("home", "constructor"), "Build.").await;
    assert_eq!(
        endpoint.completions(5).await[4].body.as_ref().unwrap()
            ["model"],
        "small"
    );
    s.chief
        .limit("home", "pinned", "constructor", json!(null))
        .await;
    assert_eq!(s.server.stop().await, Some(0));
    let server = s
        .home
        .serve_providers(&[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::small"),
            ("HAMLET_BASE_URL", &endpoint.url),
        ])
        .await;
    let mut chief = Client::join(&server).await;
    chief.say(to("home", "constructor"), "New default.").await;
    assert_eq!(
        endpoint.completions(6).await[5].body.as_ref().unwrap()
            ["model"],
        "small"
    );
}

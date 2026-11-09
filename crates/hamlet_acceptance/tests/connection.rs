//! The connection: the hello, the token, malformed messages, sizes
//! and closing.

use std::time::{Duration, Instant};

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

fn fault(text: &str, to: Option<&RequestId>) -> ServerMessage {
    ServerMessage::Fault(Fault {
        text: text.into(),
        to: to.cloned(),
    })
}

#[tokio::test]
async fn refuses_a_wrong_token_and_closes() {
    let s = Session::start(Faux::new()).await;
    let mut wire = Wire::open(&s.server.url).await;
    let hello = json!({"type": "hello", "v": VERSION.to_string(), "token": "not-the-token"});
    wire.send_text(&hello.to_string()).await;
    let (got, ending) = wire.rest().await;
    assert_eq!(
        got,
        [fault("That token is not this server's.", None)]
    );
    assert_eq!(ending, Ending::Closed(4001));
}

#[tokio::test]
async fn refuses_a_message_before_hello_and_closes() {
    let s = Session::start(Faux::new()).await;
    let mut wire = Wire::open(&s.server.url).await;
    wire.send_text(
        &serde_json::to_string(
            &json!({"type": "read", "id": RequestId::new(), "what": "charter"}),
        )
        .unwrap(),
    )
    .await;
    let (got, ending) = wire.rest().await;
    assert_eq!(got, [fault("The first message is a hello.", None)]);
    assert_eq!(ending, Ending::Closed(1008));
}

#[tokio::test]
async fn refuses_another_protocol_version() {
    let s = Session::start(Faux::new()).await;
    let mut wire = Wire::open(&s.server.url).await;
    let hello = json!({"type": "hello", "v": "0.2.0", "token": s.server.token});
    wire.send_text(&hello.to_string()).await;
    let (got, ending) = wire.rest().await;
    assert_eq!(
        got,
        [fault(
            &format!(
                "This `hello` message does not fit the protocol: \
                 `v` 0.2.0 is not compatible with {VERSION}."
            ),
            None
        )]
    );
    assert_eq!(ending, Ending::Closed(4002));
}

#[tokio::test]
async fn closes_a_connection_that_never_says_hello() {
    let s = Session::start(Faux::new()).await;
    let mut wire = Wire::open(&s.server.url).await;
    let began = Instant::now();
    let (got, ending) = wire.rest().await;
    assert!(
        began.elapsed() >= Duration::from_secs(9),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(got, [fault("No hello came in time.", None)]);
    assert_eq!(ending, Ending::Closed(1008));
}

#[tokio::test]
async fn answers_malformed_messages_with_faults_and_stays_open() {
    let s = Session::start(Faux::new()).await;
    let mut chief = s.join().await;
    let hand = s.hand();
    let cases = [
        (
            json!({"type": "say", "id": "1", "to": hand, "text": "Hi"}),
            "This `say` message does not fit the protocol: `id` \
             must be a UUID.",
            false,
        ),
        (
            json!({"type": "say", "id": "ID", "to": hand}),
            "This `say` message does not fit the protocol: it lacks \
             `text`.",
            true,
        ),
        (
            json!({"type": "say", "id": "ID", "to": hand, "text": "Hi", "loud": true}),
            "This `say` message does not fit the protocol: it has \
             no field `loud`.",
            true,
        ),
        (
            json!({"type": "shout", "id": "ID"}),
            "There is no `shout` message.",
            true,
        ),
        (
            json!({"type": "edit", "id": "ID", "doc": "bylaws"}),
            "This `edit` message does not fit the protocol: `doc` \
             must be one of `charter`, `character`, `memory`, \
             `limits`, `settings`, `catalogue`.",
            true,
        ),
        (
            json!({"type": "read", "id": "ID"}),
            "This `read` message does not fit the protocol: `what` \
             must be one of `prompt`, `history`, `models`, \
             `papers`, `charter`.",
            true,
        ),
        (
            json!({"type": "edit", "id": "ID", "doc": "settings", "set": {"reflectEvery": "four"}}),
            "This `edit` message does not fit the protocol: \
             `set.reflectEvery` must be integer.",
            true,
        ),
        (
            json!([]),
            "A message is a JSON object with a `type`.",
            false,
        ),
    ];
    for (message, text, answered) in cases {
        let request = RequestId::new();
        let raw = message
            .to_string()
            .replace("\"ID\"", &format!("\"{request}\""));
        chief_send_raw(&mut chief, &raw).await;
        let to = answered.then_some(&request);
        chief
            .until(text, |c| c.messages().contains(&fault(text, to)))
            .await;
    }
    let reply = chief.say(hand, "/queue").await;
    assert_eq!(reply.text, "Nothing waits for you.");
    let reply = chief.say(hand, "/queue").await;
    assert_eq!(reply.text, "Nothing waits for you.");
}

async fn chief_send_raw(chief: &mut Client, text: &str) {
    chief.send_raw(text).await;
}

#[tokio::test]
async fn closes_a_connection_that_sends_more_than_a_mebibyte() {
    let s = Session::start(Faux::new()).await;
    let mut chief = s.join().await;
    let hand = s.hand();
    let text = "x".repeat(1_048_576);
    let message = json!({"type": "say", "id": RequestId::new(), "to": hand, "text": text});
    chief.send_raw(&message.to_string()).await;
    let (got, ending) = chief.into_wire().rest().await;
    assert!(
        got.iter().any(|m| matches!(m, ServerMessage::Fault(f)
            if f.text == "A message is larger than the 1048576 bytes this connection takes.")),
        "{got:?}"
    );
    assert_eq!(ending, Ending::Closed(1009));
}

#[tokio::test]
async fn sends_every_connection_the_realms_changes() {
    let mut s = Session::start(Faux::new()).await;
    let mut other = s.join().await;
    let hand = s.hand();
    s.chief.command(hand, "/scholar new ada").await;
    other
        .until("the change on the other connection", |c| {
            c.scholar("ada").is_some()
        })
        .await;
    let first = other.realm();
    let late = s.join().await;
    let realm = late.realm();
    assert_eq!(realm.scholars, first.scholars);
    assert_eq!(realm.chief, "chief");
    let revisions = other
        .messages()
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Change(c) => Some(c.revision),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        revisions.is_sorted() && !revisions.is_empty(),
        "{revisions:?}"
    );
}

#[tokio::test]
async fn closes_its_connections_with_going_away_when_it_stops() {
    let mut s = Session::start(Faux::new()).await;
    let chief = s.join().await;
    let mut wire = chief.into_wire();
    assert_eq!(s.server.stop().await, Some(0));
    let (_, ending) = wire.rest().await;
    assert_eq!(ending, Ending::Closed(1001));
}

#[tokio::test]
async fn publishes_its_address_without_the_token() {
    let s = Session::start(Faux::new()).await;
    let published =
        std::fs::read_to_string(s.home.file("server.json")).unwrap();
    assert!(!published.contains(&s.server.token));
    let value = serde_json::from_str::<serde_json::Value>(&published)
        .unwrap();
    let started = value["started"].as_str().unwrap();
    // An ISO time in UTC, to the millisecond.
    assert_eq!(started.len(), 24, "{started}");
    assert!(started.ends_with('Z'), "{started}");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(s.home.file("credentials.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn keeps_its_token_and_refuses_a_second_server_on_one_home() {
    let mut home = Home::new();
    let mut first = home.serve(&Faux::new(), QUIET).await;
    let token = first.token.clone();
    let second = home.try_serve(&Faux::new(), QUIET).await;
    assert!(
        second.contains(&format!(
            "A server already runs for {}",
            home.path().display()
        )),
        "{second}"
    );
    first.stop().await;
    let again = home.serve(&Faux::new(), QUIET).await;
    assert_eq!(again.token, token);
}

#[tokio::test]
async fn refuses_a_second_hello_and_stays_open() {
    let mut s = Session::start(Faux::new()).await;
    let hello = ClientMessage::Hello(Hello {
        token: s.server.token.clone(),
        v: VERSION,
    });
    s.chief.send(hello).await;
    let again = fault("This connection said hello already.", None);
    s.chief
        .until("a second hello refused", |c| {
            c.messages().contains(&again)
        })
        .await;
    let hand = s.hand();
    s.chief.command(hand, "/queue").await;
}

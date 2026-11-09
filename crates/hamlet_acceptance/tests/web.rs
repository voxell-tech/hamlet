//! The agents' web tools: the server asks the window the chief used
//! last that draws Web panels to act on the agent's page, and gives
//! the agent the window's answer.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

/// The tools the chat of `chat` shows, with how each ended.
fn tools(
    chief: &Client,
    chat: &ChatRef,
) -> Vec<(String, Option<String>)> {
    chief
        .items(chat)
        .into_iter()
        .filter_map(|item| item.tool)
        .map(|tool| (tool.name, tool.failure))
        .collect()
}

/// The asks the server sent this client.
fn asks(chief: &Client) -> Vec<WebAsk> {
    chief
        .messages()
        .iter()
        .filter_map(|message| match message {
            ServerMessage::Web(ask) => Some(ask.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn web_open_is_refused_with_no_window_that_draws_web_panels() {
    let faux = Faux::new().agent(
        2,
        [
            call("web_open", json!({"address": "localhost:5173"})),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Look at the site.").await;
    s.chief.shows(&now, "Done.").await;
    let tools = tools(&s.chief, &now);
    assert_eq!(tools.len(), 1, "{tools:?}");
    assert_eq!(tools[0].0, "web_open");
    assert!(
        tools[0]
            .1
            .as_deref()
            .is_some_and(|words| words.contains("No window")),
        "{tools:?}"
    );
    assert!(asks(&s.chief).is_empty());
}

#[tokio::test]
async fn web_open_asks_the_window_in_front_and_gives_its_answer() {
    let faux = Faux::new().agent(
        2,
        [
            call(
                "web_open",
                json!({"address": "localhost:5173/app"}),
            ),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.send(ClientMessage::WebWindow(WebWindow {})).await;
    s.chief.tell(ada, "Look at the site.").await;
    s.chief
        .until("the server to ask for a page", |c| {
            !asks(c).is_empty()
        })
        .await;
    let ask = asks(&s.chief).remove(0);
    assert_eq!(ask.agent, ada);
    assert!(!ask.net, "a page on this machine needs no network");
    assert_eq!(
        ask.act,
        WebAct::Open {
            address: "http://localhost:5173/app".into(),
        }
    );
    s.chief
        .send(ClientMessage::Web(WebAnswer {
            to: ask.id,
            text: "Loaded http://localhost:5173/app.".into(),
            failed: false,
        }))
        .await;
    s.chief.shows(&now, "Done.").await;
    assert_eq!(
        tools(&s.chief, &now),
        [("web_open".to_owned(), None)]
    );
}

#[tokio::test]
async fn a_page_on_another_machine_waits_for_the_chief_without_the_network()
 {
    let faux = Faux::new().agent(
        2,
        [
            call("web_open", json!({"address": "example.com"})),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief.send(ClientMessage::WebWindow(WebWindow {})).await;
    s.chief.tell(ada, "Look at example.com.").await;
    s.chief
        .until("the call to wait for the chief", |c| {
            !c.realm().waits.is_empty()
        })
        .await;
    assert!(asks(&s.chief).is_empty());
}

#[tokio::test]
async fn web_read_asks_for_the_page_and_gives_what_the_window_read() {
    let faux = Faux::new().agent(
        2,
        [call("web_read", json!({"html": true})), words("Done.")],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.send(ClientMessage::WebWindow(WebWindow {})).await;
    s.chief.tell(ada, "Read the page.").await;
    s.chief
        .until("the server to ask for a read", |c| {
            !asks(c).is_empty()
        })
        .await;
    let ask = asks(&s.chief).remove(0);
    assert_eq!(ask.act, WebAct::Read { html: true });
    s.chief
        .send(ClientMessage::Web(WebAnswer {
            to: ask.id,
            text: "Title: Docs".into(),
            failed: false,
        }))
        .await;
    s.chief.shows(&now, "Done.").await;
    assert_eq!(
        tools(&s.chief, &now),
        [("web_read".to_owned(), None)]
    );
}

#[tokio::test]
async fn web_console_asks_for_the_page_console() {
    let faux = Faux::new()
        .agent(2, [call("web_console", json!({})), words("Done.")]);
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.send(ClientMessage::WebWindow(WebWindow {})).await;
    s.chief.tell(ada, "Look at the console.").await;
    s.chief
        .until("the server to ask for the console", |c| {
            !asks(c).is_empty()
        })
        .await;
    let ask = asks(&s.chief).remove(0);
    assert_eq!(ask.act, WebAct::Console {});
    s.chief
        .send(ClientMessage::Web(WebAnswer {
            to: ask.id,
            text: "[error] boom (app.js:3)".into(),
            failed: false,
        }))
        .await;
    s.chief.shows(&now, "Done.").await;
    assert_eq!(
        tools(&s.chief, &now),
        [("web_console".to_owned(), None)]
    );
}

#[tokio::test]
async fn web_wait_asks_for_text_and_holds_to_thirty_seconds() {
    let faux = Faux::new().agent(
        2,
        [
            call("web_wait", json!({"text": "Saved", "seconds": 90})),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.send(ClientMessage::WebWindow(WebWindow {})).await;
    s.chief.tell(ada, "Wait for it.").await;
    s.chief
        .until("the server to ask for a wait", |c| {
            !asks(c).is_empty()
        })
        .await;
    let ask = asks(&s.chief).remove(0);
    assert_eq!(
        ask.act,
        WebAct::Wait {
            text: Some("Saved".into()),
            selector: None,
            seconds: WEB_WAIT_MOST,
        }
    );
    s.chief
        .send(ClientMessage::Web(WebAnswer {
            to: ask.id,
            text: "The page shows \"Saved\".".into(),
            failed: false,
        }))
        .await;
    s.chief.shows(&now, "Done.").await;
    assert_eq!(
        tools(&s.chief, &now),
        [("web_wait".to_owned(), None)]
    );
}

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use hamlet_acceptance::{
    Home, QUIET, Script, Server, Wire, chat, fold, id, mayor, realm,
};
use hamlet_protocol::v0::{
    ClientMessage, Item, ServerMessage, State,
};
use serde_json::{Value, json};

pub const MAYOR: &str = "mayor";

/// The recorder owns the process and captures both directions in
/// observed order, using the same checked wire as acceptance tests.
pub struct Capture {
    pub home: Home,
    server: Server,
    wire: Wire,
    frames: Vec<Value>,
    messages: Vec<ServerMessage>,
    pages: HashSet<String>,
    secrets: Vec<String>,
}

impl Capture {
    pub async fn start(script: Script) -> Self {
        let mut home = Home::new();
        let server = home.serve(&script, QUIET).await;
        Self::join(home, server).await
    }

    pub async fn join(home: Home, server: Server) -> Self {
        let wire = Wire::open(&server.url).await;
        let token = server.token.clone();
        let mut result = Self {
            home,
            server,
            wire,
            frames: vec![],
            messages: vec![],
            pages: HashSet::new(),
            secrets: vec![token.clone()],
        };
        result
            .send(json!({"type":"hello","v":0,"token":token}))
            .await;
        result.next().await;
        assert_eq!(result.frames[1]["message"]["type"], "state");
        result
    }

    pub async fn send(&mut self, mut value: Value) -> String {
        if !matches!(value["type"].as_str(), Some("hello" | "close"))
        {
            value["id"] = id().into();
        }
        if value["type"] == "login"
            && let Some(secret) = value["reply"]["value"].as_str()
        {
            self.secrets.push(secret.into());
        }
        let request = value["id"].as_str().unwrap_or("").to_owned();
        if value["type"] == "open" && !value["before"].is_null() {
            self.pages.insert(request.clone());
        }
        let message =
            serde_json::from_value::<ClientMessage>(value.clone())
                .unwrap();
        assert_eq!(serde_json::to_value(&message).unwrap(), value);
        self.wire.send_json(&message).await;
        self.frames.push(json!({"from":"client","message":value}));
        request
    }

    async fn next(&mut self) {
        let message = tokio::time::timeout(
            Duration::from_secs(20),
            self.wire.message(),
        )
        .await
        .unwrap_or_else(|_| {
            panic!("recording stalled: {}", self.server.log())
        })
        .expect("the server stays connected");
        self.frames.push(json!({"from":"server","message":message}));
        self.messages.push(message);
    }

    pub async fn until(&mut self, condition: impl Fn(&Self) -> bool) {
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(20);
        while !condition(self) {
            tokio::time::timeout_at(deadline, self.next())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "recording did not reach its state: {}",
                        self.server.log()
                    )
                });
        }
    }

    pub async fn ask(&mut self, value: Value) -> Value {
        let request = self.send(value).await;
        self.until(|c| {
            c.messages.iter().any(|m| m.to() == Some(&request))
        })
        .await;
        serde_json::to_value(
            self.messages
                .iter()
                .find(|m| m.to() == Some(&request))
                .unwrap(),
        )
        .unwrap()
    }

    pub async fn say(&mut self, agent: &str, text: &str) {
        let reply = self.ask(json!({"type":"say","to":{"hamlet":"home","agent":agent},"text":text})).await;
        assert_eq!(reply["kind"], "notice", "{reply}");
    }

    pub async fn open(&mut self) -> Value {
        self.ask(json!({"type":"open","chat":{"with":{"hamlet":"home","agent":"mayor"}}})).await
    }

    pub fn items(&self) -> Vec<Item> {
        fold::items(&self.messages, &chat(mayor()), &self.pages)
    }

    pub fn state(&self) -> State {
        realm::fold(&self.messages)
    }

    pub fn has(&self, matches: impl Fn(&Value) -> bool) -> bool {
        self.frames
            .iter()
            .any(|f| f["from"] == "server" && matches(&f["message"]))
    }

    pub async fn shows(&mut self, text: &str) {
        self.until(|c| c.items().iter().any(|i| i.text == text))
            .await;
    }

    pub async fn idle(&mut self) {
        self.until(|c| c.state().agents.iter().all(|a| !a.busy))
            .await;
    }

    pub async fn finish(mut self, name: &str) {
        // Drain messages already in flight, without adding frames of
        // our own or changing real IDs and timestamps.
        let end =
            tokio::time::Instant::now() + Duration::from_millis(100);
        while let Ok(Some(message)) =
            tokio::time::timeout_at(end, self.wire.message()).await
        {
            self.frames
                .push(json!({"from":"server","message":message}));
        }
        self.wire.close().await;
        assert_eq!(self.server.stop().await, Some(0));
        self.secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for frame in &mut self.frames {
            scrub(frame, &self.secrets);
            let value = frame["message"].clone();
            let encoded = if frame["from"] == "client" {
                serde_json::to_value(
                    serde_json::from_value::<ClientMessage>(
                        value.clone(),
                    )
                    .unwrap(),
                )
                .unwrap()
            } else {
                serde_json::to_value(
                    serde_json::from_value::<ServerMessage>(
                        value.clone(),
                    )
                    .unwrap(),
                )
                .unwrap()
            };
            assert_eq!(encoded, value);
        }
        let text = self
            .frames
            .iter()
            .map(|f| serde_json::to_string(f).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        for secret in &self.secrets {
            assert!(
                !text.contains(secret),
                "a credential reached the capture"
            );
        }
        let dir = std::env::var_os("HAMLET_FRAME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../hamlet_protocol/frames")
            });
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.jsonl")), text)
            .unwrap();
    }
}

fn scrub(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets.iter().filter(|s| !s.is_empty()) {
                *text = text.replace(secret, "[REDACTED]");
            }
        }
        Value::Array(items) => {
            items.iter_mut().for_each(|v| scrub(v, secrets))
        }
        Value::Object(fields) => {
            fields.values_mut().for_each(|v| scrub(v, secrets))
        }
        _ => {}
    }
}

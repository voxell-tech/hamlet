//! A scripted loopback OpenAI endpoint for the server's providers.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};

/// One request the endpoint received.
#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    /// The `Authorization` header, if any.
    pub authorization: Option<String>,
    /// The JSON body of a completion.
    pub body: Option<Value>,
}

/// The endpoint, stopped on drop.
pub struct Endpoint {
    /// The base URL, ending in `/v1`.
    pub url: String,
    received: mpsc::UnboundedReceiver<Request>,
    seen: Vec<Request>,
    responses: mpsc::UnboundedSender<Response>,
    listing: watch::Sender<Value>,
    task: JoinHandle<()>,
}

/// One completion, consumed when its request arrives, before any
/// hold.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub text: String,
    pub finish: String,
    pub usage: Option<Value>,
    pub hold_until: Option<PathBuf>,
    /// OpenAI tool calls in the assistant delta.
    pub calls: Vec<Value>,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            status: 200,
            text: "Hello.".into(),
            finish: "stop".into(),
            usage: Some(json!({"prompt_tokens": 100,
                "completion_tokens": 20, "cost": COST})),
            hold_until: None,
            calls: vec![],
        }
    }
}

/// The cost the endpoint bills for every answer, in dollars.
pub const COST: f64 = 0.0015;

impl Endpoint {
    /// Listens on a free loopback port, listing the model `org/large`
    /// with a context of 32000 tokens.
    pub async fn start() -> Self {
        let listener =
            TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, received) = mpsc::unbounded_channel();
        let (responses, mut plans) = mpsc::unbounded_channel();
        let (listing, listings) = watch::channel(json!({
            "data": [{"id": "org/large", "context_length": 32000}]
        }));
        let (incoming, mut requests) = mpsc::unbounded_channel::<(
            Request,
            oneshot::Sender<(Response, Option<Value>)>,
        )>();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        connections.spawn(answer(stream, incoming.clone()));
                    }
                    Some((request, reply)) = requests.recv() => {
                        let list = request.path.ends_with("/models")
                            .then(|| listings.borrow().clone());
                        let response = if list.is_some() {
                            Response::default()
                        } else {
                            plans.try_recv().unwrap_or_default()
                        };
                        let _ = sender.send(request);
                        let _ = reply.send((response, list));
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/v1"),
            received,
            seen: vec![],
            responses,
            listing,
            task,
        }
    }

    /// Queues completion responses; an empty queue answers "Hello.".
    pub fn enqueue(
        &self,
        responses: impl IntoIterator<Item = Response>,
    ) {
        for response in responses {
            self.responses.send(response).unwrap();
        }
    }

    /// Replaces the next and subsequent model listings.
    pub fn list(&self, data: Value) {
        self.listing.send_replace(json!({"data": data}));
    }

    /// Waits for `count` completion requests, including held
    /// requests.
    pub async fn completions(
        &mut self,
        count: usize,
    ) -> Vec<Request> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let completions = self
                    .requests()
                    .iter()
                    .filter(|r| r.path.ends_with("/chat/completions"))
                    .cloned()
                    .collect::<Vec<_>>();
                if completions.len() >= count {
                    return completions;
                }
                self.seen.push(self.received.recv().await.unwrap());
            }
        })
        .await
        .expect("completion requests arrive")
    }

    /// Every request received so far.
    pub fn requests(&mut self) -> &[Request] {
        while let Ok(request) = self.received.try_recv() {
            self.seen.push(request);
        }
        &self.seen
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn answer(
    stream: TcpStream,
    incoming: mpsc::UnboundedSender<(
        Request,
        oneshot::Sender<(Response, Option<Value>)>,
    )>,
) -> Option<()> {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line).await.ok()?;
    let path = line.split_whitespace().nth(1)?.to_string();
    let mut length = 0;
    let mut authorization = None;
    loop {
        let mut header = String::new();
        stream.read_line(&mut header).await.ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().ok()?,
            "authorization" => {
                authorization = Some(value.to_string())
            }
            _ => {}
        }
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.ok()?;
    let body = serde_json::from_slice::<Value>(&body).ok();
    let (send, receive) = oneshot::channel();
    incoming
        .send((
            Request {
                path,
                authorization,
                body,
            },
            send,
        ))
        .ok()?;
    let (response, listing) = receive.await.ok()?;
    if let Some(file) = &response.hold_until {
        while !file.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let (kind, text) = if let Some(listing) = listing {
        ("application/json", listing.to_string())
    } else if response.status != 200 {
        (
            "application/json",
            json!({"error": {"message": response.text}}).to_string(),
        )
    } else {
        let mut delta =
            json!({"role": "assistant", "content": response.text});
        if !response.calls.is_empty() {
            delta["tool_calls"] = json!(response.calls);
        }
        let mut chunks = vec![
            json!({"id": "call-1", "choices": [{"index": 0, "delta":
                delta,
                "finish_reason": null}]}),
            json!({"choices": [{"index": 0, "delta": {},
                "finish_reason": response.finish}]}),
        ];
        if let Some(usage) = response.usage {
            chunks.push(json!({"choices": [], "usage": usage}));
        }
        let mut text = chunks
            .iter()
            .map(|c| format!("data: {c}\n\n"))
            .collect::<String>();
        text.push_str("data: [DONE]\n\n");
        ("text/event-stream", text)
    };
    let head = format!(
        "HTTP/1.1 {} Reply\r\nContent-Type: \
         {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        text.len()
    );
    let stream = stream.get_mut();
    stream.write_all(head.as_bytes()).await.ok()?;
    stream.write_all(text.as_bytes()).await.ok()?;
    stream.shutdown().await.ok()?;
    Some(())
}

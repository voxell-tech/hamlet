//! A loopback endpoint that speaks enough of the OpenAI API for a
//! server's real providers: a model listing, and an answer of
//! "Hello." to every chat completion.

use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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
    task: JoinHandle<()>,
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
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let sender = sender.clone();
                tokio::spawn(async move {
                    answer(stream, sender).await;
                });
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/v1"),
            received,
            seen: vec![],
            task,
        }
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
    received: mpsc::UnboundedSender<Request>,
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
    let (kind, text) = if path.ends_with("/models") {
        let listing = json!({
            "data": [{"id": "org/large", "context_length": 32000}]
        });
        ("application/json", listing.to_string())
    } else {
        let chunks = [
            json!({"id": "call-1", "choices": [{"index": 0, "delta":
                {"role": "assistant", "content": "Hello."},
                "finish_reason": null}]}),
            json!({"choices": [{"index": 0, "delta": {},
                "finish_reason": "stop"}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 100,
                "completion_tokens": 20, "cost": COST}}),
        ];
        let mut text = chunks
            .iter()
            .map(|c| format!("data: {c}\n\n"))
            .collect::<String>();
        text.push_str("data: [DONE]\n\n");
        ("text/event-stream", text)
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: \
         {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    received
        .send(Request {
            path,
            authorization,
            body,
        })
        .ok()?;
    let stream = stream.get_mut();
    stream.write_all(head.as_bytes()).await.ok()?;
    stream.write_all(text.as_bytes()).await.ok()?;
    stream.shutdown().await.ok()?;
    Some(())
}

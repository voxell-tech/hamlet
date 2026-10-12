//! The client for servers that speak the OpenAI chat protocol, and
//! the endpoints of a home.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use futures::StreamExt;
use futures::future::BoxFuture;
use reqwest::StatusCode;
use serde_json::{Value, json};

use super::stream::{LineSplitter, StreamAcc};
use super::{CallRecord, Completion, Delta, Provider};
use crate::config::{Config, host};

/// The wait for a server to take the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The waits before the further tries of a call that another try may
/// mend: a server that cannot be reached, is busy or failed.
const RETRY_WAITS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// The longest wait a server may ask for before another try.
const RETRY_LONGEST: Duration = Duration::from_secs(30);

/// The most of a server's error text that is shown, in characters.
const ERROR_SHOWN: usize = 500;

/// Client for any OpenAI Chat Completions compatible endpoint
/// (Ollama, DeepSeek, ...).
pub struct OpenAiCompat {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiCompat {
    pub fn new(
        base_url: String,
        api_key: Option<String>,
        model: String,
    ) -> Self {
        // No overall timeout: streams can run for minutes.
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("reqwest client config is static");
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        }
    }

    async fn run(
        &self,
        messages: &[Value],
        tools: &[Value],
        model: Option<&str>,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Completion> {
        let model = model.unwrap_or(&self.model);
        // Without `include_usage` a streamed reply carries no token
        // counts at all.
        let mut body = json!({"model": model, "messages": messages, "stream": true,
                              "stream_options": {"include_usage": true}});
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        let mut waits = RETRY_WAITS.iter();
        loop {
            let failed = match self.attempt(&body, on_delta).await {
                Ok(done) => return Ok(done),
                Err(failed) => failed,
            };
            let wait = failed.again.zip(waits.next());
            let Some((asked, wait)) = wait else {
                return Err(failed.error);
            };
            let wait = asked.unwrap_or(*wait).min(RETRY_LONGEST);
            tokio::time::sleep(wait).await;
        }
    }

    /// Asks the server once.
    async fn attempt(
        &self,
        body: &Value,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Completion, Failed> {
        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let started = Instant::now();
        let resp = req
            .send()
            .await
            .with_context(|| {
                format!("could not reach {}", self.base_url)
            })
            .map_err(Failed::passing)?;
        let status = resp.status();
        if !status.is_success() {
            let asked = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
                .map(Duration::from_secs);
            let text = resp.text().await.unwrap_or_default();
            let shown =
                text.chars().take(ERROR_SHOWN).collect::<String>();
            let ellipsis =
                if shown.len() < text.len() { "..." } else { "" };
            let error = anyhow!("HTTP {status}: {shown}{ellipsis}");
            let busy = status.is_server_error()
                || [
                    StatusCode::REQUEST_TIMEOUT,
                    StatusCode::TOO_MANY_REQUESTS,
                ]
                .contains(&status);
            return Err(Failed {
                error,
                again: busy.then_some(asked),
            });
        }

        let mut stream = resp.bytes_stream();
        let mut lines = LineSplitter::default();
        let mut acc = StreamAcc::since(started);
        let mut done = false;
        // A reply that has begun is shown, so it is not asked for
        // again.
        let mut begun = false;
        let mut hear = |delta: Delta<'_>| {
            begun = true;
            on_delta(delta)
        };
        'outer: while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(e) if begun => {
                    return Err(Failed::last(e.into()));
                }
                Err(e) => return Err(Failed::passing(e.into())),
            };
            for line in lines.push(&chunk) {
                if acc
                    .feed_line(&line, &mut hear)
                    .map_err(Failed::last)?
                {
                    done = true;
                    break 'outer;
                }
            }
        }
        if let Some(line) = lines.finish() {
            done |= acc
                .feed_line(&line, &mut hear)
                .map_err(Failed::last)?;
        }
        if !acc.ended(done) {
            let error = anyhow!(
                "the model's reply was cut off before it finished; \
                 send the message again"
            );
            return Err(match begun {
                true => Failed::last(error),
                false => Failed::passing(error),
            });
        }
        let mut call = CallRecord {
            model: body["model"].as_str().unwrap_or_default().into(),
            endpoint: host(&self.base_url).to_string(),
            first_token_ms: acc.first_token_ms,
            duration_ms: started.elapsed().as_millis() as u64,
            finish_reason: acc.finish_reason.take(),
            ..CallRecord::default()
        };
        if let Some(usage) = acc.usage.take() {
            call.read_usage(&usage);
        }
        Ok(Completion {
            message: acc.into_message(),
            call,
        })
    }
}

/// A try that failed.
struct Failed {
    error: anyhow::Error,
    /// Another try may mend it, after the wait the server asked for
    /// where it asked for one.
    again: Option<Option<Duration>>,
}

impl Failed {
    fn passing(error: anyhow::Error) -> Self {
        Failed {
            error,
            again: Some(None),
        }
    }

    fn last(error: anyhow::Error) -> Self {
        Failed { error, again: None }
    }
}

impl Provider for OpenAiCompat {
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        model: Option<&'a str>,
        on_delta: &'a mut (dyn FnMut(Delta<'_>) + Send),
    ) -> BoxFuture<'a, Result<Completion>> {
        Box::pin(self.run(messages, tools, model, on_delta))
    }
}

/// Every endpoint of a home. A model is run where its name says:
/// `far::large` on the endpoint `far`.
pub struct Endpoints {
    all: Vec<(String, OpenAiCompat)>,
    /// The model that runs where nobody chose one.
    model: String,
}

impl Endpoints {
    pub fn new(config: &Config) -> Self {
        let all = config.endpoints.iter().map(|endpoint| {
            let client = OpenAiCompat::new(
                endpoint.base_url.clone(),
                endpoint.api_key.clone(),
                String::new(),
            );
            (endpoint.name.clone(), client)
        });
        Endpoints {
            all: all.collect(),
            model: config.model.clone(),
        }
    }

    /// The client that serves the model `name` and what it calls
    /// it there.
    fn route<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<(&'a OpenAiCompat, &'a str)> {
        let (endpoint, id) = crate::catalogue::parts(name)
            .with_context(|| {
                format!(
                    "`{name}` names no endpoint. A model is named \
                     `endpoint::model`"
                )
            })?;
        self.all
            .iter()
            .find(|(known, _)| known == endpoint)
            .map(|(_, client)| (client, id))
            .with_context(|| {
                format!(
                    "`{name}` is served by the endpoint \
                     `{endpoint}`, which the settings do not have"
                )
            })
    }
}

impl Provider for Endpoints {
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        model: Option<&'a str>,
        on_delta: &'a mut (dyn FnMut(Delta<'_>) + Send),
    ) -> BoxFuture<'a, Result<Completion>> {
        Box::pin(async move {
            let name = model.unwrap_or(&self.model);
            let (client, id) = self.route(name)?;
            let mut done = client
                .run(messages, tools, Some(id), on_delta)
                .await?;
            // The records name a model as the home does.
            done.call.model = name.to_string();
            Ok(done)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_run_where_its_name_says() {
        use crate::config::Endpoint;
        let all = Endpoints::new(&Config {
            endpoints: vec![Endpoint {
                name: "far".into(),
                base_url: "https://far/v1/".into(),
                api_key: None,
            }],
            ..Config::default()
        });
        let at = |model| {
            all.route(model)
                .map(|(client, id)| (&*client.base_url, id))
        };
        assert_eq!(
            at("far::org/large").unwrap(),
            ("https://far/v1", "org/large")
        );
        assert!(at("gone::large").is_err());
        // A name with no endpoint in it is run nowhere.
        assert!(at("org/small:8b").is_err());
    }

    #[tokio::test]
    async fn a_busy_server_is_asked_again() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let address =
            format!("http://{}/v1", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let said = "data: {\"choices\":[{\"delta\":{\"content\":\
                        \"hi\"},\"finish_reason\":\"stop\"}]}\n\n";
            let answers = [
                "HTTP/1.1 503 Busy\r\nretry-after: \
                 0\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    .to_string(),
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: \
                     {}\r\nconnection: close\r\n\r\n{said}",
                    said.len()
                ),
            ];
            for answer in answers {
                let (mut socket, _) =
                    listener.accept().await.unwrap();
                let mut asked = Vec::new();
                // The request is headers, then a JSON object.
                while !(asked.windows(4).any(|w| w == b"\r\n\r\n")
                    && asked.ends_with(b"}"))
                {
                    let mut part = [0; 4096];
                    let read = socket.read(&mut part).await.unwrap();
                    asked.extend_from_slice(&part[..read]);
                }
                socket.write_all(answer.as_bytes()).await.unwrap();
            }
        });
        let client = OpenAiCompat::new(address, None, "m".into());
        let done = client
            .complete(&[], &[], None, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(done.message["content"], "hi");
    }

    #[tokio::test]
    #[cfg(feature = "live-tests")]
    async fn live_ollama_tool_call() {
        let local = crate::config::Config::default();
        let (_, model) =
            crate::catalogue::parts(&local.model).unwrap();
        let p = OpenAiCompat::new(
            local.endpoints[0].base_url.clone(),
            None,
            model.to_string(),
        );
        let messages = vec![
            json!({"role": "system", "content": "You are terse. Use the bash tool when needed."}),
            json!({"role": "user", "content": "How many lines are in /etc/hosts? Use bash."}),
        ];
        let tools = vec![json!({"type": "function", "function": {
            "name": "bash", "description": "Run a shell command and return stdout.",
            "parameters": {"type": "object",
                "properties": {"command": {"type": "string"}}, "required": ["command"]}}})];
        let mut cb = |_: Delta<'_>| {};
        let done = p
            .complete(&messages, &tools, None, &mut cb)
            .await
            .unwrap();
        // Ollama reports usage only because the request asks for it.
        assert!(
            done.call.input_tokens.unwrap() > 0,
            "{:?}",
            done.call
        );
        assert!(done.call.output_tokens.unwrap() > 0);
        assert!(done.call.first_token_ms.is_some());
        assert_eq!(done.call.endpoint, "localhost");
        let msg = done.message;
        let calls = msg["tool_calls"]
            .as_array()
            .expect("expected tool_calls");
        assert_eq!(calls[0]["function"]["name"], "bash");
        assert!(!calls[0]["id"].as_str().unwrap().is_empty());
        let args = serde_json::from_str::<Value>(
            calls[0]["function"]["arguments"].as_str().unwrap(),
        )
        .unwrap();
        assert!(args["command"].is_string());
    }
}

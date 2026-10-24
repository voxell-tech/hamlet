//! Bounded HTTP ingress to an owned service event loop.

use std::convert::Infallible;

use anyhow::Result;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use crate::wire::{Code, MAX_BODY, Reply};

pub struct Call {
    pub authorization: String,
    pub body: Vec<u8>,
    pub reply: oneshot::Sender<Reply>,
}

#[expect(
    clippy::print_stdout,
    reason = "it prints the address it listens on"
)]
pub async fn listen(address: &str) -> Result<mpsc::Receiver<Call>> {
    let listener = TcpListener::bind(address).await?;
    println!("LISTEN http://{}", listener.local_addr()?);
    let (send, receive) = mpsc::channel::<Call>(32);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let send = send.clone();
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(
                        TokioIo::new(stream),
                        service_fn(move |request| {
                            serve(request, send.clone())
                        }),
                    )
                    .await;
            });
        }
    });
    Ok(receive)
}

async fn serve(
    request: Request<Incoming>,
    send: mpsc::Sender<Call>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = request.into_parts();
    let reply = if parts.method != "POST" || parts.uri.path() != "/v1"
    {
        Reply::error(Code::Invalid, "Use POST /v1.", false)
    } else {
        let authorization = parts
            .headers
            .get(hyper::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Limited::new(body, MAX_BODY).collect(),
        )
        .await
        {
            Ok(Ok(bytes)) => {
                let (reply, receive) = oneshot::channel();
                let call = Call {
                    authorization,
                    body: bytes.to_bytes().to_vec(),
                    reply,
                };
                if send.send(call).await.is_err() {
                    Reply::uncertain()
                } else {
                    receive
                        .await
                        .unwrap_or_else(|_| Reply::uncertain())
                }
            }
            _ => Reply::error(
                Code::Invalid,
                "Request exceeds its size or read-time limit.",
                false,
            ),
        }
    };
    let bytes = serde_json::to_vec(&reply).unwrap_or_default();
    let mut response = Response::new(Full::new(Bytes::from(bytes)));
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

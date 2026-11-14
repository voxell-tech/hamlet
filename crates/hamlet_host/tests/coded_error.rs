//! A handler's coded error reaches its caller whole: its group, code,
//! message and metadata, whether an actor, a client or the program
//! made the call.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use at::at;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Caller, Host, HostRef};
use hamlet_testing::Scratch;
use kind::kind;
use serde::{Deserialize, Serialize};

#[path = "support/at.rs"]
mod at;
#[path = "support/kind.rs"]
mod kind;

#[derive(Debug, ActorError, Serialize)]
#[error(
    "message",
    "wrong_caller",
    "The caller may not send this message.",
    "{caller} may not send '{action}'."
)]
struct Refusal {
    action: &'static str,
    #[serde(skip_serializing_if = "Stamp::is_none")]
    caller: Stamp,
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
struct Stamp(Option<Caller>);

impl Stamp {
    fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

impl std::fmt::Display for Stamp {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match &self.0 {
            Some(caller) => caller.fmt(f),
            None => f.write_str("a call with no caller"),
        }
    }
}

/// An error as its caller reads it with [`ActorError::extract`].
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Seen {
    group: String,
    code: String,
    message: String,
    metadata: Option<String>,
}

impl Seen {
    fn of(error: &anyhow::Error) -> Self {
        let error = ActorError::extract(error);
        Self {
            group: error.group().to_owned(),
            code: error.code().to_owned(),
            message: error.message().to_owned(),
            metadata: error
                .metadata()
                .map(|metadata| metadata.to_string()),
        }
    }
}

/// Refuses every call with [`Refusal`].
struct Callee;

#[derive(Serialize, Deserialize)]
struct Refuse;

impl Action for Refuse {
    type Output = ();
    const NAME: &'static str = "refuse";
}

#[async_trait::async_trait]
impl Actor for Callee {
    type Input = ();
    type Actions = (Refuse,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl Handler<Refuse> for Callee {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Refuse,
    ) -> anyhow::Result<()> {
        let caller = ctx
            .conn()
            .context("the call has no connection")?
            .params()?;
        Err(Refusal {
            action: Refuse::NAME,
            caller: Stamp(caller),
        }
        .build())
    }
}

/// Calls the [`Callee`] at `to` and answers the error it read.
struct Asker;

#[derive(Serialize, Deserialize)]
struct Ask {
    to: Address,
}

impl Action for Ask {
    type Output = Option<Seen>;
    const NAME: &'static str = "ask";
}

#[async_trait::async_trait]
impl Actor for Asker {
    type Input = ();
    type Actions = (Ask,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl Handler<Ask> for Asker {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        ask: Ask,
    ) -> anyhow::Result<Option<Seen>> {
        let answer = HostRef::of(ctx.inner())?
            .call(
                &ask.to,
                Refuse::NAME,
                hamlet_actor::action::encode_positional(&Refuse)?,
                Duration::from_secs(10),
            )
            .await;
        Ok(answer.err().as_ref().map(Seen::of))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coded_error_reaches_each_caller_whole() {
    let dir = Scratch::new("coded-error");
    let mut registry = Registry::new();
    kind::<Asker>(&mut registry, "asker");
    kind::<Callee>(&mut registry, "callee");
    let host = Host::open(&dir, registry.into_factories()).unwrap();
    let (callee, asker) = (at("callee", "x"), at("asker", "a"));
    host.get_or_start(&callee).await.unwrap();
    host.get_or_start(&asker).await.unwrap();
    let deadline = Duration::from_secs(10);
    let ask = hamlet_actor::action::encode_positional(&Ask {
        to: callee.clone(),
    })
    .unwrap();
    let by_actor = host.call(&asker, Ask::NAME, ask, deadline).await;
    let refuse =
        hamlet_actor::action::encode_positional(&Refuse).unwrap();
    let by_program = host
        .call(&callee, Refuse::NAME, refuse.clone(), deadline)
        .await
        .unwrap_err();
    let by_client = host
        .call_for_client(&callee, Refuse::NAME, refuse, deadline)
        .await
        .unwrap_err();
    host.shutdown().await.unwrap();

    let seen = |message: &str, metadata: &str| Seen {
        group: "message".into(),
        code: "wrong_caller".into(),
        message: message.into(),
        metadata: Some(metadata.into()),
    };
    assert_eq!(
        ciborium::from_reader::<Option<Seen>, _>(
            by_actor.unwrap().as_slice()
        )
        .unwrap(),
        Some(seen(
            "asker/a may not send 'refuse'.",
            r#"{"action":"refuse","caller":{"actor":{"key":"a","kind":"asker"}}}"#,
        ))
    );
    assert_eq!(
        Seen::of(&by_program),
        seen(
            "a call with no caller may not send 'refuse'.",
            r#"{"action":"refuse"}"#,
        )
    );
    assert_eq!(
        Seen::of(&by_client),
        seen(
            "client may not send 'refuse'.",
            r#"{"action":"refuse","caller":"client"}"#,
        )
    );
}

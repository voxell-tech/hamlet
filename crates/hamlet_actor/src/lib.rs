// This file was changed for Hamlet. See NOTICE in this crate.
#![recursion_limit = "256"]

pub mod action;
pub mod actor;
pub mod context;
pub mod event;
pub mod prelude;
pub mod registry;
mod serverless_listener;
pub mod sqlite;
pub mod start;
pub mod telemetry;
pub mod test;
pub mod typed_client;

pub use hamlet_actor_client as client;
pub use hamlet_actor_core::actor::schedule::{
    CronFire, ScheduleErrorInfo, ScheduleKind, ScheduledEventInfo,
    ScheduledFireInfo,
};
pub use hamlet_actor_core::inspector::InspectorTabEntry;
pub use hamlet_actor_core::metrics::RenderedMetrics;
pub use hamlet_actor_core::serverless::{
    CoreServerlessRuntime, ServerlessRequest, ServerlessResponse,
    ServerlessStreamError,
};
pub use hamlet_actor_core::sqlite::{
    BindParam, ColumnValue, ExecResult, QueryResult,
};
pub use hamlet_actor_core::{
    ActorConfig, ActorHttpResponse, ActorKey, ActorKeySegment,
    ActorKv, ActorTracingConfig, CanHibernateWebSocket, ConnHandle,
    ConnId, EngineSpawnMode, HTTP_BODY_STREAM_CHANNEL_CAPACITY,
    KeepAwakeRegion, ListOpts, Request, Response, ResponseChunk,
    RuntimeMode, ServeConfig, SqliteBatchStatement, SqliteDb,
    SqliteTransaction, StreamingResponse, WebSocket, WsMessage,
    serverless_http,
};

pub use crate::action::{
    Action, ActionEntry, ActionSet, Handler, Raw,
};
pub use crate::actor::{
    Actor, HttpCallbackAdmission, HttpCallbackClass,
};
pub use crate::context::{
    ConnCtx, ConnIter, Cron, CronEveryOptions, CronJobInfo,
    CronSetOptions, Ctx, Schedule,
};
pub use crate::event::{
    ActionCall, ConnClosed, ConnOpen, Destroy, Event, EventEntry,
    EventSet, HttpCall, HttpReply, RuntimeEvent, Sleep, Subscribe,
    WsOpen,
};
pub use crate::registry::Registry;
pub use crate::sqlite::{SqliteDbExt, SqliteTransactionOptions};
pub use crate::start::{Events, Hibernated, Input, Start, run_actor};
pub use crate::typed_client::{
    IntoActorKey, TypedActorConnection, TypedActorHandle,
    TypedClientExt,
};

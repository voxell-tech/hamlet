// This file was changed for Hamlet. See NOTICE in this package.
export type { ActorKey } from "@/client/query";
export { ALLOWED_PUBLIC_HEADERS } from "@/common/actor-router-consts";
export type { Encoding } from "@/common/encoding";
export type {
    UniversalErrorEvent,
    UniversalEvent,
    UniversalEventSource,
    UniversalMessageEvent,
} from "@/common/eventsource-interface";
export type {
    SocketCloseEvent,
    SocketEvent,
    SocketMessageEvent,
    UniversalWebSocket,
} from "@/common/websocket-interface";
export type * from "./config";
export { defineRunHandler } from "./config";
export type {
    ActionContextOf,
    BeforeActionResponseContextOf,
    BeforeConnectContextOf,
    ConnContextOf,
    ConnectContextOf,
    ConnInitContextOf,
    CreateConnStateContextOf,
    CreateContextOf,
    CreateVarsContextOf,
    DestroyContextOf,
    DisconnectContextOf,
    MigrateContextOf,
    RequestContextOf,
    RunContextOf,
    SleepContextOf,
    StateChangeContextOf,
    WakeContextOf,
    WebSocketContextOf,
} from "./contexts";
export type {
    AnyActorDefinition,
    AnyActorInstance,
    AnyStaticActorDefinition,
    AnyStaticActorInstance,
    BaseActorDefinition,
    BaseActorInstance,
} from "./definition";
export {
    ActorDefinition,
    actor,
    isStaticActorDefinition,
    isStaticActorInstance,
    lookupInRegistry,
} from "./definition";
export {
    ActorError,
    ActorError,
    type ActorErrorLike,
    type ActorErrorOptions,
    UserError,
    type UserErrorOptions,
} from "./errors";
export {
    type EventSchemaConfig,
    event,
    type InferEventArgs,
    type InferSchemaMap,
    type QueueSchemaConfig,
    queue,
    type Type,
} from "./schema";
export type {
    ActionSampleRates,
    ActionTraceSamplers,
    ActorTracingOptions,
} from "./tracing";

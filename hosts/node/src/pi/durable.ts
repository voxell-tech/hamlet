// This file was changed for Hamlet. See NOTICE in this package.
import type { EventSchemaConfig, QueueSchemaConfig } from "@hamlet/actor";
import type { PiActions, PiPromptOptions, PiPromptResult } from "./actions.js";
import {
    type PiAccess,
    type PiActorConfigInput,
    type PiEvents,
    type PiUserActions,
    pi,
} from "./actor.js";
import type { PiOptions } from "./runtime.js";

export * from "./index.js";

/** @deprecated Use `pi()` from `@hamlet/pi`. `piDurable()` is the same actor under its old name. */
export const piDurable = pi;

/** @deprecated Use `PiActions` from `@hamlet/pi`. */
export type PiDurableActions = PiActions;
/** @deprecated Use `PiPromptOptions` from `@hamlet/pi`. */
export type PiDurablePromptOptions = PiPromptOptions;
/** @deprecated Use `PiPromptResult` from `@hamlet/pi`. */
export type PiDurablePromptResult = PiPromptResult;
/** @deprecated Use `PiAccess` from `@hamlet/pi`. */
export type PiDurableAccess = PiAccess;
/** @deprecated Use `PiActorConfigInput` from `@hamlet/pi`. */
export type PiDurableActorConfigInput<
    TState = undefined,
    TConnParams = undefined,
    TConnState = undefined,
    TVars = undefined,
    TInput = undefined,
    TEvents extends EventSchemaConfig = Record<never, never>,
    TQueues extends QueueSchemaConfig = Record<never, never>,
    TUserActions extends PiUserActions<
        TState,
        TConnParams,
        TConnState,
        TVars,
        TInput,
        TEvents,
        TQueues
    > = Record<never, never>,
> = PiActorConfigInput<
    TState,
    TConnParams,
    TConnState,
    TVars,
    TInput,
    TEvents,
    TQueues,
    TUserActions
>;
/** @deprecated Use `PiEvents` from `@hamlet/pi`. */
export type PiDurableEvents = PiEvents;
/** @deprecated Use `PiUserActions` from `@hamlet/pi`. */
export type PiDurableUserActions<
    TState,
    TConnParams,
    TConnState,
    TVars,
    TInput,
    TEvents extends EventSchemaConfig,
    TQueues extends QueueSchemaConfig,
> = PiUserActions<
    TState,
    TConnParams,
    TConnState,
    TVars,
    TInput,
    TEvents,
    TQueues
>;
/** @deprecated Use `PiOptions` from `@hamlet/pi`. */
export type PiDurableOptions = PiOptions;

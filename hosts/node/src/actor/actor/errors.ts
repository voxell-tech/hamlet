// This file was changed for Hamlet. See NOTICE in this package.
import type { DeconstructedError } from "@/common/utils";

export const INTERNAL_ERROR_CODE = "internal_error";
export const INTERNAL_ERROR_DESCRIPTION = "An internal error occurred";
export type InternalErrorMetadata = Record<string, never>;

export const USER_ERROR_CODE = "user_error";
export const BRIDGE_HAMLET_ACTOR_ERROR_PREFIX = "__HAMLET_ACTOR_ERROR_JSON__:";

export interface ActorErrorOptions extends ErrorOptions {
    /** Error data can safely be serialized in a response to the client. */
    public?: boolean;
    /** Metadata associated with this error. */
    metadata?: unknown;
    /** Request identifier used to correlate this error with engine logs. */
    rayId?: string;
    /** Explicit HTTP status override for router responses. */
    statusCode?: number;
    /** Actor context associated with this error. */
    actor?: ActorSpecifier;
}

/** Identifies the actor that was handling work when an error was produced. */
export interface ActorSpecifier {
    actorId: string;
    generation: number;
    key?: string;
}

export interface ActorErrorLike {
    __type?: "ActorError" | "ActorError";
    group: string;
    code: string;
    message: string;
    metadata?: unknown;
    rayId?: string;
    public?: boolean;
    statusCode?: number;
    actor?: ActorSpecifier;
}

export interface BridgeActorErrorPayload extends ActorErrorLike {}

/**
 * Shape as it arrives from the native bridge, where an absent ray ID serializes
 * to null. Normalized to a BridgeActorErrorPayload before validation.
 */
interface NativeBridgeErrorPayload
    extends Omit<BridgeActorErrorPayload, "rayId"> {
    rayId?: string | null;
}

export interface UserErrorOptions extends ErrorOptions {
    /**
     * Machine readable code for this error. Useful for catching different types of
     * errors in try-catch.
     */
    code?: string;
    /**
     * Additional metadata related to the error. Useful for understanding context
     * about the error.
     */
    metadata?: unknown;
}

function looksLikeActorErrorOptions(
    value: unknown,
): value is ActorErrorOptions {
    return (
        typeof value === "object" &&
        value !== null &&
        ("public" in value ||
            "metadata" in value ||
            "rayId" in value ||
            "statusCode" in value ||
            "actor" in value ||
            "cause" in value)
    );
}

function isTypedErrorTag(value: unknown): value is "ActorError" | "ActorError" {
    return value === "ActorError" || value === "ActorError";
}

function errorMessage(error: unknown, fallback = String(error)): string {
    if (
        error &&
        typeof error === "object" &&
        "message" in error &&
        typeof error.message === "string"
    ) {
        return error.message;
    }

    return fallback;
}

export function isActorErrorLike(
    error: unknown,
): error is ActorError | DeconstructedError | ActorErrorLike {
    return (
        typeof error === "object" &&
        error !== null &&
        "group" in error &&
        typeof error.group === "string" &&
        "code" in error &&
        typeof error.code === "string" &&
        "message" in error &&
        typeof error.message === "string" &&
        (!("rayId" in error) ||
            error.rayId === undefined ||
            typeof error.rayId === "string") &&
        (!("__type" in error) || isTypedErrorTag(error.__type))
    );
}

// Matches the `actor`/`aborted` error that core raises when an in-flight queue
// wait is interrupted because the actor is going to sleep. This is the expected
// way a parked `run` handler unwinds during sleep, not a failure.
export function isActorAbortedError(error: unknown): boolean {
    return (
        isActorErrorLike(error) &&
        error.group === "actor" &&
        error.code === "aborted"
    );
}

function isActorSpecifier(value: unknown): value is ActorSpecifier {
    return (
        typeof value === "object" &&
        value !== null &&
        "actorId" in value &&
        typeof value.actorId === "string" &&
        "generation" in value &&
        typeof value.generation === "number" &&
        (!("key" in value) ||
            value.key === undefined ||
            typeof value.key === "string")
    );
}

export class ActorError extends Error {
    __type = "ActorError" as const;

    public public: boolean;
    public metadata?: unknown;
    public readonly rayId?: string;
    public statusCode: number;
    public actor?: ActorSpecifier;
    public readonly group: string;
    public readonly code: string;

    public static isActorError(
        error: unknown,
    ): error is ActorError | DeconstructedError {
        return isActorErrorLike(error);
    }

    constructor(
        group: string,
        code: string,
        message: string,
        options?: ActorErrorOptions | unknown,
    ) {
        const normalized = looksLikeActorErrorOptions(options)
            ? options
            : { metadata: options };

        super(message, { cause: normalized.cause });
        this.name = "ActorError";
        this.group = group;
        this.code = code;
        this.public = normalized.public ?? false;
        this.metadata = normalized.metadata;
        this.rayId = normalized.rayId ?? undefined;
        this.statusCode = normalized.statusCode ?? (this.public ? 400 : 500);
        this.actor = normalized.actor;
    }

    toString() {
        return this.message;
    }
}

export class UserError extends ActorError {
    constructor(message: string, options?: UserErrorOptions) {
        super("user", options?.code ?? USER_ERROR_CODE, message, {
            public: true,
            metadata: options?.metadata,
            cause: options?.cause,
        });
    }
}

export function toActorError(
    error: unknown,
    fallback?: Partial<ActorErrorLike>,
): ActorError {
    if (typeof error === "string") {
        const bridged = decodeBridgeActorError(error);
        if (bridged) {
            return bridged;
        }
    }

    if (error instanceof Error) {
        const bridged = decodeBridgeActorError(error.message);
        if (bridged) {
            return bridged;
        }
    }

    if (isActorErrorLike(error)) {
        return new ActorError(error.group, error.code, error.message, {
            public: error.public,
            statusCode: error.statusCode,
            metadata: error.metadata,
            rayId: error.rayId,
            actor: error.actor,
            cause: error instanceof Error ? error.cause : undefined,
        });
    }

    return new ActorError(
        fallback?.group ?? "actor",
        fallback?.code ?? INTERNAL_ERROR_CODE,
        errorMessage(error, fallback?.message ?? "Unknown error"),
        {
            public: fallback?.public,
            statusCode: fallback?.statusCode,
            metadata: fallback?.metadata,
            rayId: fallback?.rayId,
            actor: fallback?.actor,
            cause: error instanceof Error ? error : undefined,
        },
    );
}

/** A structured error encoded for the bridge to Core. */
export interface EncodedBridgeError {
    readonly encoded: string;
    /** Set when JSON could not encode the metadata, which was dropped. */
    readonly droppedMetadataCause?: unknown;
}

/**
 * Encodes a structured error for the bridge to Core. Metadata that JSON cannot
 * encode, such as a `bigint`, is dropped so the error itself still crosses.
 */
export function encodeBridgeActorError(
    error: ActorErrorLike,
): EncodedBridgeError {
    const payload = {
        group: error.group,
        code: error.code,
        message: error.message,
        metadata: error.metadata,
        rayId: error.rayId,
        public: error.public,
        statusCode: error.statusCode,
        actor: error.actor,
    };
    try {
        return {
            encoded: `${BRIDGE_HAMLET_ACTOR_ERROR_PREFIX}${JSON.stringify(payload)}`,
        };
    } catch (cause) {
        return {
            encoded: `${BRIDGE_HAMLET_ACTOR_ERROR_PREFIX}${JSON.stringify({ ...payload, metadata: undefined })}`,
            droppedMetadataCause: cause,
        };
    }
}

/**
 * Encodes an error for the telemetry bridge. A structured error crosses with
 * its group and code. Anything else crosses as text so Core classifies it,
 * which is the same rule that keeps raw messages out of every span.
 */
export function encodeErrorForBridge(error: unknown): string {
    if (isCanonicalStructuredActorError(error)) {
        return encodeBridgeActorError(error).encoded;
    }
    return String(error);
}

/** A `ActorError` instance, or a plain object carrying its full tagged shape. */
export function isCanonicalStructuredActorError(
    error: unknown,
): error is ActorErrorLike {
    return (
        error instanceof ActorError ||
        (typeof error === "object" &&
            error !== null &&
            "__type" in error &&
            error.__type === "ActorError" &&
            "group" in error &&
            typeof error.group === "string" &&
            "code" in error &&
            typeof error.code === "string" &&
            "message" in error &&
            typeof error.message === "string")
    );
}

export function decodeBridgeActorErrorPayload(
    value: string,
): BridgeActorErrorPayload | undefined {
    if (!value.startsWith(BRIDGE_HAMLET_ACTOR_ERROR_PREFIX)) {
        return undefined;
    }

    try {
        const raw = JSON.parse(
            value.slice(BRIDGE_HAMLET_ACTOR_ERROR_PREFIX.length),
        ) as NativeBridgeErrorPayload;
        // Coerce the native bridge's null ray ID to undefined before validation.
        const payload: BridgeActorErrorPayload = {
            ...raw,
            rayId: raw.rayId ?? undefined,
        };
        if (!isActorErrorLike(payload)) {
            return undefined;
        }
        if (
            payload.actor !== undefined &&
            payload.actor !== null &&
            !isActorSpecifier(payload.actor)
        ) {
            return undefined;
        }

        return payload;
    } catch {
        return undefined;
    }
}

export function decodeBridgeActorError(value: string): ActorError | undefined {
    const payload = decodeBridgeActorErrorPayload(value);
    if (!payload) {
        return undefined;
    }

    return new ActorError(payload.group, payload.code, payload.message, {
        metadata: payload.metadata,
        rayId: payload.rayId,
        public: payload.public,
        statusCode: payload.statusCode,
        actor: payload.actor ?? undefined,
    });
}

export function isActorErrorCode(
    error: unknown,
    group: string,
    code: string,
): error is ActorError {
    return (
        isActorErrorLike(error) && error.group === group && error.code === code
    );
}

export function internalError(
    message: string,
    options?: Partial<ActorErrorOptions> & {
        group?: string;
        code?: string;
    },
): ActorError {
    return new ActorError(
        options?.group ?? "actor",
        options?.code ?? INTERNAL_ERROR_CODE,
        message,
        {
            public: options?.public,
            statusCode: options?.statusCode,
            metadata: options?.metadata,
            rayId: options?.rayId,
            actor: options?.actor,
            cause: options?.cause,
        },
    );
}

export function invalidEncoding(format?: string): ActorError {
    return new ActorError(
        "encoding",
        "invalid",
        `Invalid encoding \`${format}\`.`,
        {
            public: true,
        },
    );
}

export function invalidRequest(error?: unknown): ActorError {
    return new ActorError(
        "request",
        "invalid",
        `Invalid request: ${errorMessage(error, String(error))}`,
        {
            public: true,
            cause: error instanceof Error ? error : undefined,
        },
    );
}

export function actorNotFound(identifier?: string): ActorError {
    return new ActorError(
        "actor",
        "not_found",
        identifier ? `Actor not found: ${identifier}` : "Actor not found",
        { public: true },
    );
}

export function actorStopping(identifier?: string): ActorError {
    return new ActorError(
        "actor",
        "stopping",
        identifier ? `Actor stopping: ${identifier}` : "Actor stopping",
        { public: true },
    );
}

export interface ActorRestartingOptions {
    phase?: "stopping" | "sleeping" | "waking" | "runner_shutdown";
    retryAfterMs?: number;
}

export function actorRestarting(opts?: ActorRestartingOptions): ActorError {
    return new ActorError(
        "actor",
        "restarting",
        "Actor is restarting. Retry the request.",
        {
            public: true,
            statusCode: 503,
            metadata: {
                retryable: true,
                ...(opts?.phase ? { phase: opts.phase } : {}),
                ...(opts?.retryAfterMs !== undefined
                    ? { retryAfterMs: opts.retryAfterMs }
                    : {}),
            },
        },
    );
}

export function forbiddenError(): ActorError {
    return new ActorError("auth", "forbidden", "Forbidden", {
        public: true,
        statusCode: 403,
    });
}

export function unsupportedFeature(feature: string): ActorError {
    return new ActorError(
        "feature",
        "unsupported",
        `Unsupported feature: ${feature}`,
    );
}

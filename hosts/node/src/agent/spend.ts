/**
 * The model gate: every provider request asks the realm before it is sent
 * and is charged after it ends.
 *
 * It is a `Models` proxy, because nothing else can refuse a request: a
 * hook's throw is reported and the request goes out anyway, and an
 * extension's `wraps` cover tools and sections only. The actor builds one
 * at each wake around `AgentConfig.models`, so it knows its agent and maps a
 * provider session to a conversation only.
 *
 * | `Models` method | Asks `may_spend` | Charges |
 * |---|---|---|
 * | `streamSimple` (a generation) | before | when the stream ends |
 * | `completeSimple` (a compaction) | before | on the answer |
 * | every other method | no | no |
 *
 * pi-durable sends every request through the two `Simple` methods, so
 * `stream` and `complete` pass through as the other methods do.
 *
 * A deferred poll (`fetchDeferred`, `streamDeferred`) passes through: no
 * provider P5 runs defers.
 */
import {
    type Api,
    type AssistantMessage,
    createAssistantMessageEventStream,
    type Model,
    type Models,
} from "@earendil-works/pi-ai";
import type { RealmTask } from "./messages.ts";
import { Closing, cutOff, modelCall, type Peers, reasonOf } from "./turn.ts";

/**
 * `models` with the gate around it.
 *
 * P6's seam: `models.ts` builds the `Models` given here, and the notice and
 * the wait for a chief after a refusal are P6's.
 */
export function gated(models: Models, peers: Peers): Models {
    const ask = async (
        model: Model<Api>,
        asked: { sessionId?: string; signal?: AbortSignal } | undefined,
    ): Promise<{ task: RealmTask } | { ended: AssistantMessage }> => {
        try {
            const { task } = (await peers.turns.ofSession(asked?.sessionId))
                .task;
            const may = await cutOff(
                asked?.signal,
                peers.reach.may_spend({ task }),
            );
            if (may.type === "refused") {
                return { ended: refusalOf(model, may.words) };
            }
            return { task };
        } catch (error) {
            if (error instanceof Closing) {
                // The close aborts this run and drops what the request ends
                // with. A refusal here would be wrong: the next wake asks again.
                await abortOf(asked?.signal);
                return { ended: brokenOf(model, error) };
            }
            return {
                ended: refusalOf(
                    model,
                    `it could not be asked (${reasonOf(error)})`,
                ),
            };
        }
    };
    const charge = async (
        task: RealmTask,
        message: AssistantMessage,
        started: number,
    ): Promise<void> => {
        try {
            await peers.reach.charge({
                call: modelCall(),
                task,
                micros: u32(message.usage.cost.total * 1_000_000),
                took_ms: u32(message.durationMs ?? Date.now() - started),
            });
        } catch (error) {
            peers.report("a model request's charge was not taken", error);
        }
    };
    const streamed =
        (send: Models["streamSimple"]): Models["streamSimple"] =>
        (model, context, options) => {
            const out = createAssistantMessageEventStream();
            void (async () => {
                const allowed = await ask(model, options);
                if ("ended" in allowed) {
                    out.end(allowed.ended);
                    return;
                }
                const started = Date.now();
                const source = send(model, context, options);
                for await (const event of source) {
                    // The stream ends for pi-durable with this event, so the
                    // charge goes out before it.
                    if (event.type === "done") {
                        await charge(allowed.task, event.message, started);
                    } else if (event.type === "error") {
                        await charge(allowed.task, event.error, started);
                    }
                    out.push(event);
                }
                out.end(await source.result());
            })().catch((error: unknown) => {
                // The provider's stream broke after the request was sent.
                out.end(brokenOf(model, error));
            });
            return out;
        };
    const completed =
        (send: Models["completeSimple"]): Models["completeSimple"] =>
        async (model, context, options) => {
            const allowed = await ask(model, options);
            if ("ended" in allowed) return allowed.ended;
            const started = Date.now();
            const message = await send(model, context, options);
            await charge(allowed.task, message, started);
            return message;
        };
    return new Proxy(models, {
        get(target, key, receiver) {
            if (key === "streamSimple") {
                return streamed((...args) => target.streamSimple(...args));
            }
            if (key === "completeSimple") {
                return completed((...args) => target.completeSimple(...args));
            }
            const value: unknown = Reflect.get(target, key, receiver);
            return typeof value === "function" ? value.bind(target) : value;
        },
    });
}

/**
 * The message a refused request ends with: `stopReason: "error"`, no
 * content and no usage. pi-durable retries an error whose words match
 * pi-ai's transient patterns, and the realm's words can: "500 of 500 ms
 * spent" holds a status code. So the words always start with a phrase
 * pi-ai never retries, and `isRetryableAssistantError` is false for every
 * message this returns.
 */
export function refusalOf(model: Model<Api>, why: string): AssistantMessage {
    return {
        role: "assistant",
        content: [],
        api: model.api,
        provider: model.provider,
        model: model.id,
        usage: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 0,
            totalTokens: 0,
            cost: {
                input: 0,
                output: 0,
                cacheRead: 0,
                cacheWrite: 0,
                total: 0,
            },
        },
        stopReason: "error",
        errorMessage: `${NEVER_RETRIED}: ${why}`,
        timestamp: Date.now(),
    };
}

/**
 * The message a request ends with when its provider's stream threw, or when
 * the harness closed under it: the error's own words, with nothing of the
 * gate's in them, so pi-durable treats it as it would without the gate.
 */
function brokenOf(model: Model<Api>, error: unknown): AssistantMessage {
    return { ...refusalOf(model, ""), errorMessage: reasonOf(error) };
}

/**
 * Resolves when `signal` aborts. pi-durable gives every provider request its
 * run's signal, which a close aborts; with none, it resolves at once.
 */
function abortOf(signal: AbortSignal | undefined): Promise<void> {
    if (signal === undefined || signal.aborted) return Promise.resolve();
    return new Promise((resolve) =>
        signal.addEventListener("abort", () => resolve(), { once: true }),
    );
}

/** Holds "billing", which pi-ai's `isRetryableAssistantError` never retries. */
const NEVER_RETRIED =
    "The realm's billing did not allow this model request, and it was not sent";

/** A whole number that a `u32` holds: rounded, and held at the ends. */
function u32(value: number): number {
    if (!Number.isFinite(value) || value <= 0) return 0;
    return Math.min(Math.round(value), 2 ** 32 - 1);
}

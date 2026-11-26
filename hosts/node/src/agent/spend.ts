/**
 * A `Models` proxy, because nothing else can refuse a request: a hook's
 * throw is reported and the request goes out anyway, and an extension's
 * `wraps` cover tools and sections only.
 *
 * pi-durable sends every request through the two `Simple` methods, so
 * `stream` and `complete` pass through as the other methods do.
 */
import {
    type Api,
    type AssistantMessage,
    createAssistantMessageEventStream,
    type Model,
    type Models,
} from "@earendil-works/pi-ai";
import { type RealmTask, u32 } from "./messages/shared.ts";
import type { Placing } from "./prompt.ts";
import { Closing, cutOff, modelCall, type Peers, reasonOf } from "./turn.ts";

/** A request the realm refused or held, which the gate ended unsent. */
export interface Unsent {
    readonly held: boolean;
    readonly words: string;
}

/**
 * pi-durable hands the message the gate ended with to `afterResponse` as
 * it is, so the message itself says why.
 */
const UNSENT = new WeakMap<AssistantMessage, Unsent>();

export function unsentOf(message: AssistantMessage): Unsent | undefined {
    return UNSENT.get(message);
}

export function gated(
    models: Models,
    peers: Peers,
    placing: Pick<Placing, "missing">,
): Models {
    const ask = async (
        model: Model<Api>,
        asked: { sessionId?: string; signal?: AbortSignal } | undefined,
    ): Promise<{ task: RealmTask } | { ended: AssistantMessage }> => {
        try {
            const turn = await peers.turns.ofSession(asked?.sessionId);
            const unplaced =
                turn.conversation === undefined
                    ? undefined
                    : placing.missing(turn.conversation);
            if (unplaced !== undefined) {
                return { ended: unaskedOf(model, unplaced) };
            }
            const { task } = turn.task;
            const may = await cutOff(
                asked?.signal,
                peers.reach.may_spend({ task }),
            );
            if (may.type === "refused") {
                // `Hold` in `crates/hamlet_messages/src/realm/oversight.rs`:
                // a withdrawn instruction's turn stops as today's does.
                const held =
                    may.why.type === "objection" ||
                    may.why.type === "withdrawn";
                const ended = held
                    ? heldOf(model, may.words)
                    : refusalOf(model, may.words);
                UNSENT.set(ended, { held, words: may.words });
                return { ended };
            }
            return { task };
        } catch (error) {
            if (error instanceof Closing) {
                // The close aborts this run and drops what the request ends
                // with. A refusal here would be wrong: the next wake asks again.
                await abortOf(asked?.signal);
                return { ended: brokenOf(model, error) };
            }
            return { ended: unaskedOf(model, reasonOf(error)) };
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
                tokens: u32(message.usage.input + message.usage.output),
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
 * pi-durable retries an error whose words match pi-ai's transient patterns,
 * and the realm's words can: "500 of 500 ms spent" holds a status code. So
 * the words always start with a phrase pi-ai never retries.
 */
export function refusalOf(model: Model<Api>, why: string): AssistantMessage {
    return {
        ...emptyAnswer(model),
        stopReason: "error",
        errorMessage: `${REFUSED}${why}`,
    };
}

/** A request the realm could not be asked about, for `why`. */
function unaskedOf(model: Model<Api>, why: string): AssistantMessage {
    return refusalOf(model, `it could not be asked (${why})`);
}

/** An answer of `model` with nothing in it yet. */
export function emptyAnswer(model: Model<Api>): AssistantMessage {
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
        stopReason: "stop",
        timestamp: Date.now(),
    };
}

/** The realm's words in the text of a failure the gate's refusal caused. */
export function realmWordsIn(text: string): string | undefined {
    const at = text.indexOf(REFUSED);
    return at === -1 ? undefined : text.slice(at + REFUSED.length);
}

/**
 * The realm's words alone. pi-durable tries again only an `error`, and a
 * held task's name can hold a status code.
 */
function heldOf(model: Model<Api>, words: string): AssistantMessage {
    return {
        ...refusalOf(model, ""),
        stopReason: "aborted",
        rawStopReason: HELD,
        errorMessage: words,
    };
}

/** Whether the realm held `message`'s request, which today's chat shows nothing of. */
export function isHeld(message: AssistantMessage): boolean {
    return message.stopReason === "aborted" && message.rawStopReason === HELD;
}

/**
 * The error's own words, with nothing of the gate's in them, so pi-durable
 * treats it as it would without the gate.
 */
function brokenOf(model: Model<Api>, error: unknown): AssistantMessage {
    return { ...refusalOf(model, ""), errorMessage: reasonOf(error) };
}

/**
 * pi-durable gives every provider request its run's signal, which a close
 * aborts.
 */
function abortOf(signal: AbortSignal | undefined): Promise<void> {
    if (signal === undefined || signal.aborted) return Promise.resolve();
    return new Promise((resolve) =>
        signal.addEventListener("abort", () => resolve(), { once: true }),
    );
}

/** The gate's stop reason for a request the realm held. */
const HELD = "held by the realm";

/** Holds "billing", which pi-ai's `isRetryableAssistantError` never retries. */
const REFUSED =
    "The realm's billing did not allow this model request, and it was not sent: ";

import {
    type Api,
    type AssistantMessage,
    createAssistantMessageEventStream,
    type Model,
    type ProviderStreams,
    type StreamFunction,
    type StreamOptions,
} from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import type { ModelRef, Usage } from "./link.ts";

/** The context an endpoint that lists no window size is given. */
const DEFAULT_WINDOW = 32_768;

/** The server's answer to a model call it was asked about. */
export type Permit =
    | {
          readonly ok: true;
          readonly key: string;
          readonly apiKey?: string;
          readonly model?: Record<string, unknown>;
      }
    | {
          readonly ok: false;
          readonly words: string;
          /** Ends the call's turn as stopped, resolving once `signal` says the call is aborted. */
          readonly stop?: (signal: AbortSignal | undefined) => Promise<void>;
      };

/** The server's say over model calls: each is allowed first, and its usage charged before its answer is kept. */
export interface Gate {
    /** Asks for the call of the conversation whose provider session is `sessionId`. */
    attempt(sessionId: string | undefined, model: ModelRef): Promise<Permit>;
    /** Resolves once the server has charged `usage` to the attempt `key`. */
    charge(key: string, usage: Usage): Promise<void>;
}

/** `streams` with every call allowed by `gate` first, made with the credential it hands over and charged after. */
export function allowedStreams(
    streams: ProviderStreams,
    gate: Gate,
): ProviderStreams {
    return {
        stream: allowed(streams.stream, gate),
        streamSimple: allowed(streams.streamSimple, gate),
    };
}

function allowed<O extends StreamOptions>(
    stream: StreamFunction<Api, O>,
    gate: Gate,
): StreamFunction<Api, O> {
    return (model, context, options) => {
        const out = createAssistantMessageEventStream();
        let secret: string | undefined;
        void (async () => {
            const permit = await gate.attempt(options?.sessionId, {
                provider: model.provider,
                id: model.id,
            });
            if (!permit.ok) {
                await permit.stop?.(options?.signal);
                fail(out, model, permit.words);
                return;
            }
            secret = permit.apiKey;
            const selected = { ...model, ...permit.model } as Model<Api>;
            const started = Date.now();
            let firstTokenMs: number | undefined;
            // What the endpoint says it billed, which stands over pi-ai's own price.
            let billed: number | undefined;
            const previous = options?.onProviderStreamEvent;
            // Options are absent only for a call of no conversation, which the gate refuses.
            const made =
                options === undefined
                    ? options
                    : {
                          ...options,
                          ...(permit.apiKey === undefined
                              ? {}
                              : { apiKey: permit.apiKey }),
                          onProviderStreamEvent: async (
                              event: unknown,
                              called: Model<Api>,
                          ) => {
                              billed = billedCost(event) ?? billed;
                              await previous?.(event, called);
                          },
                      };
            const source = stream(selected, context, made);
            for await (const raw of source) {
                const event = scrub(raw, secret);
                if (
                    event.type === "text_delta" ||
                    event.type === "thinking_delta" ||
                    event.type === "toolcall_delta"
                )
                    firstTokenMs ??= Date.now() - started;
                if (event.type === "done" || event.type === "error") {
                    const message =
                        event.type === "done" ? event.message : event.error;
                    if (billed !== undefined) message.usage.cost.total = billed;
                    const failed =
                        message.stopReason === "error" ||
                        message.stopReason === "aborted";
                    const estimatedTokens =
                        failed || message.usage.totalTokens > 0
                            ? 0
                            : Math.floor(
                                  [...context.messages, message].reduce(
                                      (sum, value) =>
                                          sum +
                                          Buffer.byteLength(
                                              JSON.stringify(value),
                                          ),
                                      0,
                                  ) / 4,
                              );
                    await gate.charge(permit.key, {
                        ...usageOf(message),
                        ms: Date.now() - started,
                        estimatedTokens,
                        ...(billed === undefined ? {} : { billed }),
                        ...(firstTokenMs === undefined ? {} : { firstTokenMs }),
                        reasoning: message.usage.reasoning ?? 0,
                        stop: message.rawStopReason ?? message.stopReason,
                    });
                }
                out.push(event);
            }
            out.end(scrub(await source.result(), secret));
        })().catch((error: unknown) => {
            // The answer is not kept, so the call is made again under a new attempt.
            fail(
                out,
                model,
                scrub(
                    `The server did not allow or charge this call: ${error instanceof Error ? error.message : String(error)}`,
                    secret,
                ),
            );
        });
        return out;
    };
}

/** Provider output stripped of this attempt's credential before pi can keep it (C32). */
function scrub<T>(value: T, secret: string | undefined): T {
    if (!secret) return value;
    return JSON.parse(
        JSON.stringify(value, (_key, item: unknown) => {
            if (typeof item === "string")
                return item.split(secret).join("[REDACTED]");
            if (
                typeof item === "object" &&
                item !== null &&
                !Array.isArray(item)
            )
                return Object.fromEntries(
                    Object.entries(item).map(([key, value]) => [
                        key.split(secret).join("[REDACTED]"),
                        value,
                    ]),
                );
            return item;
        }),
    ) as T;
}

function fail(
    out: ReturnType<typeof createAssistantMessageEventStream>,
    model: Model<Api>,
    words: string,
): void {
    const failed = refusedMessage(model, words);
    out.push({ type: "error", reason: "error", error: failed });
    out.end(failed);
}

/** The cost a provider's raw event reports in its usage, as OpenRouter-style endpoints bill. */
function billedCost(event: unknown): number | undefined {
    if (typeof event !== "object" || event === null || !("usage" in event))
        return undefined;
    const { usage } = event;
    if (typeof usage !== "object" || usage === null || !("cost" in usage))
        return undefined;
    const { cost } = usage;
    return typeof cost === "number" && Number.isFinite(cost) ? cost : undefined;
}

/** The usage the link reports of an answer. */
export function usageOf(message: AssistantMessage): Usage {
    const { usage } = message;
    return {
        input: usage.input,
        output: usage.output,
        cacheRead: usage.cacheRead,
        cacheWrite: usage.cacheWrite,
        cost: usage.cost.total,
    };
}

function refusedMessage(model: Model<Api>, words: string): AssistantMessage {
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
        errorMessage: words,
        timestamp: Date.now(),
    };
}

/** A provider whose calls use only the server's per-attempt model and credential. */
export function modelProvider(ref: ModelRef, gate: Gate): Provider {
    const baseUrl = ref.baseUrl ?? "";
    const builtins = builtinProviders();
    const compatible = openAICompletionsApi();
    const model: Model<"openai-completions"> = {
        id: ref.id,
        name: ref.id,
        api: "openai-completions",
        provider: ref.provider,
        baseUrl,
        reasoning: false,
        input: ["text"],
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        contextWindow: DEFAULT_WINDOW,
        maxTokens: Math.min(8192, Math.floor(DEFAULT_WINDOW * 0.2)),
        compat: {
            supportsStore: false,
            supportsDeveloperRole: false,
            supportsUsageInStreaming: true,
        },
    };
    return createProvider({
        id: ref.provider,
        baseUrl,
        auth: {
            apiKey: {
                name: `${ref.provider} key`,
                resolve: async () => ({
                    auth: { apiKey: "hamlet-keyless" },
                }),
            },
        },
        models: [
            builtins
                .find((p) => p.id === ref.provider)
                ?.getModels()
                .find((m) => m.id === ref.id) ?? model,
        ],
        api: allowedStreams(
            {
                stream: (selected, context, options) => {
                    const provider = builtins.find(
                        (p) => p.id === selected.provider,
                    );
                    return provider === undefined
                        ? compatible.stream(selected, context, options)
                        : provider.stream(selected, context, options);
                },
                streamSimple: (selected, context, options) => {
                    const provider = builtins.find(
                        (p) => p.id === selected.provider,
                    );
                    return provider === undefined
                        ? compatible.streamSimple(selected, context, options)
                        : provider.streamSimple(selected, context, options);
                },
            },
            gate,
        ),
    });
}

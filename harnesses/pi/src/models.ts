import {
    type Api,
    type AssistantMessage,
    type CredentialStore,
    createAssistantMessageEventStream,
    type Model,
    type ModelAuth,
    type ProviderStreams,
    type StreamFunction,
    type StreamOptions,
} from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import {
    createModels,
    createProvider,
    type Provider,
} from "@earendil-works/pi-ai/models";
import type { ModelRef, Selection, Usage } from "./protocol.ts";
import { providers } from "./provider-set.ts";

/** The context an endpoint that lists no window size is given. */
export const DEFAULT_WINDOW = 32_768;
/** The share of a model's window kept for its answer. */
export const ANSWER_SHARE = 0.2;
/** The most tokens an answer of an OpenAI-compatible model may take. */
const MOST_ANSWER = 8192;

/** The server's answer to a model call it was asked about. */
export type Permit =
    | {
          readonly ok: true;
          readonly key: string;
          readonly apiKey?: string;
          readonly selection?: Selection;
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
    /** Removes every credential known to the server before pi sees provider output. */
    scrub<T>(value: T): Promise<T>;
    /** The host's serialized store for this attempt's provider alone. */
    credentials?(key: string, provider: string): CredentialStore;
    /** Ends credential authority, without abandoning an already started rotation. */
    finished?(key: string): Promise<void>;
    /** Refreshes the context policy from the server's selected model. */
    selected?(model: Model<Api>): void;
}

/** `streams` with every call allowed by `gate` first, made with the credential it hands over and charged after. */
export function allowedStreams(
    streams: ProviderStreams,
    gate: Gate,
    available: readonly Provider[] = providers(),
): ProviderStreams {
    return {
        stream: allowed(streams.stream, gate, available),
        streamSimple: allowed(streams.streamSimple, gate, available),
    };
}

function allowed<O extends StreamOptions>(
    stream: StreamFunction<Api, O>,
    gate: Gate,
    available: readonly Provider[],
): StreamFunction<Api, O> {
    return (model, context, options) => {
        const out = createAssistantMessageEventStream();
        let secret: string | undefined;
        let allowedKey: string | undefined;
        const clean = <T>(value: T): Promise<T> =>
            // Provider partials are mutable; the request must own its snapshot.
            gate.scrub(structuredClone(scrub(value, secret)));
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
            const selected =
                permit.selection === undefined
                    ? { ...model }
                    : selectedModel(model, permit.selection, available);
            if (selected.provider !== "faux") allowedKey = permit.key;
            let auth: ModelAuth = {};
            let env: Record<string, string> = {};
            if (
                gate.credentials !== undefined &&
                selected.provider !== "faux"
            ) {
                const models = createModels({
                    credentials: gate.credentials(
                        permit.key,
                        selected.provider,
                    ),
                    authContext: {
                        env: async () => undefined,
                        fileExists: async () => false,
                    },
                });
                models.setProvider(
                    available.find((p) => p.id === selected.provider) ??
                        createProvider({
                            id: selected.provider,
                            models: [selected],
                            api: openAICompletionsApi(),
                            auth: {
                                apiKey: {
                                    name: selected.provider,
                                    resolve: async ({ credential }) => ({
                                        auth: {
                                            apiKey:
                                                credential?.key ??
                                                "hamlet-keyless",
                                        },
                                    }),
                                },
                            },
                        }),
                );
                const resolved = await models.getAuth(
                    selected,
                    options?.signal === undefined
                        ? {}
                        : { signal: options.signal },
                );
                auth = resolved?.auth ?? { apiKey: "hamlet-keyless" };
                env = resolved?.env ?? {};
                secret = auth.apiKey;
                if (auth.baseUrl !== undefined) selected.baseUrl = auth.baseUrl;
            }
            model.contextWindow = selected.contextWindow;
            model.maxTokens = selected.maxTokens;
            gate.selected?.(selected);
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
                          ...auth,
                          env,
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
                if (
                    raw.type === "text_delta" ||
                    raw.type === "thinking_delta" ||
                    raw.type === "toolcall_delta"
                )
                    firstTokenMs ??= Date.now() - started;
                const event = await clean(raw);
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
            out.end(await clean(await source.result()));
        })()
            .catch(async (error: unknown) => {
                // The answer is not kept, so the call is made again under a new attempt.
                let words = "The server could not scrub model output.";
                try {
                    words = await clean(
                        `The server did not allow or charge this call: ${error instanceof Error ? error.message : String(error)}`,
                    );
                } catch {}
                fail(out, model, words);
            })
            .finally(async () => {
                if (allowedKey !== undefined) await gate.finished?.(allowedKey);
            })
            .catch(() => {});
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

/** The usage the protocol reports of an answer. */
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

/** A model served by an OpenAI-compatible endpoint, as pi calls it, priced at `rate` for every kind of token. */
export function compatibleModel(
    provider: string,
    id: string,
    baseUrl: string,
    window = DEFAULT_WINDOW,
    rate = 0,
): Model<"openai-completions"> {
    return {
        id,
        name: id,
        api: "openai-completions",
        provider,
        baseUrl,
        reasoning: false,
        input: ["text"],
        cost: { input: rate, output: rate, cacheRead: rate, cacheWrite: rate },
        contextWindow: window,
        maxTokens: Math.min(MOST_ANSWER, Math.floor(window * ANSWER_SHARE)),
        compat: {
            supportsStore: false,
            supportsDeveloperRole: false,
            supportsUsageInStreaming: true,
        },
    };
}

/** The pi model `selection` names: `called` when it is that model, else as its provider lists it. */
export function selectedModel(
    called: Model<Api>,
    selection: Selection,
    available: readonly Provider[],
): Model<Api> {
    const { model: ref } = selection;
    const listed =
        ref.provider === called.provider && ref.id === called.id
            ? called
            : listedModel(ref, available);
    return {
        ...listed,
        ...(ref.baseUrl === undefined ? {} : { baseUrl: ref.baseUrl }),
        contextWindow: selection.contextWindow ?? listed.contextWindow,
        maxTokens: selection.maxTokens ?? listed.maxTokens,
        cost: { ...selection.price },
    };
}

/** `ref` as its provider lists it; a model the provider lists only once refreshed is served like its others. */
function listedModel(
    ref: ModelRef,
    available: readonly Provider[],
): Model<Api> {
    return servedModel(
        ref,
        available.find((p) => p.id === ref.provider),
    );
}

/** `ref` as `provider` lists it, else like its other models, else at the address the server gave. */
function servedModel(
    ref: ModelRef,
    provider: Provider | undefined,
): Model<Api> {
    const models = provider?.getModels() ?? [];
    const [like] = models;
    if (like !== undefined)
        return (
            models.find((m) => m.id === ref.id) ?? {
                ...like,
                id: ref.id,
                name: ref.id,
            }
        );
    if (ref.baseUrl === undefined)
        throw new Error(
            `The model ${ref.provider}::${ref.id} has no address: pi has no provider ${ref.provider} of its own, and the server gave no base URL.`,
        );
    return compatibleModel(ref.provider, ref.id, ref.baseUrl);
}

/** A provider whose calls use only the server's per-attempt model and credential. */
export function modelProvider(
    ref: ModelRef,
    gate: Gate,
    builtins: readonly Provider[] = providers(),
): Provider {
    const compatible = openAICompletionsApi();
    const model = servedModel(
        ref,
        builtins.find((p) => p.id === ref.provider),
    );
    return createProvider({
        id: ref.provider,
        ...(ref.baseUrl === undefined ? {} : { baseUrl: ref.baseUrl }),
        auth: {
            apiKey: {
                name: `${ref.provider} key`,
                resolve: async () => ({
                    auth: { apiKey: "hamlet-keyless" },
                }),
            },
        },
        models: [model],
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
            builtins,
        ),
    });
}

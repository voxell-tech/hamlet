import {
    type AssistantMessageEventStream,
    createAssistantMessageEventStream,
    type Model,
    type ProviderStreams,
    type StreamOptions,
} from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import type { HomeBase } from "../home.ts";
import { safeWords } from "../scrub.ts";
import type { Endpoint } from "../settings.ts";
import { estimateOf } from "../spend.ts";
import { type Card, readList, record } from "./catalogue.ts";
import { assumedRate, atEndpoint } from "./endpoints.ts";

/** The default context of an endpoint that lists no window size. */
export const DEFAULT_WINDOW = 32_768;

/** Builds an OpenAI-compatible provider with seeded models for an endpoint. */
export function compatible(
    endpoint: Endpoint,
    seeds: readonly string[],
): Provider {
    const make = (
        id: string,
        context = DEFAULT_WINDOW,
        price = assumedRate(endpoint.baseUrl),
    ): Model<"openai-completions"> => ({
        id,
        name: id,
        api: "openai-completions",
        provider: endpoint.name,
        baseUrl: endpoint.baseUrl,
        reasoning: false,
        input: ["text"],
        cost: {
            input: price,
            output: price,
            cacheRead: price,
            cacheWrite: price,
        },
        contextWindow: context,
        maxTokens: Math.min(8192, Math.floor(context * 0.2)),
        compat: {
            supportsStore: false,
            supportsDeveloperRole: false,
            supportsUsageInStreaming: true,
        },
    });
    const provider = createProvider({
        id: endpoint.name,
        baseUrl: endpoint.baseUrl,
        auth: {
            apiKey: {
                name: `${endpoint.name} key`,
                login: async (interaction) => ({
                    type: "api_key",
                    key: (
                        await interaction.prompt({
                            type: "secret",
                            message: "Paste your key.",
                        })
                    ).trim(),
                }),
                resolve: async ({ credential }) => ({
                    auth: {
                        apiKey: credential?.key ?? "hamlet-keyless",
                        baseUrl: endpoint.baseUrl,
                    },
                }),
            },
        },
        models: seeds.map((seed) => make(seed)),
        fetchModels: async ({ signal, credential }) => {
            const key =
                credential?.type === "api_key"
                    ? credential.key
                    : credential?.access;
            let response: Response;
            try {
                response = await fetch(
                    `${endpoint.baseUrl.replace(/\/+$/, "")}/models`,
                    {
                        signal,
                        headers: key ? { Authorization: `Bearer ${key}` } : {},
                    },
                );
            } catch (error) {
                if (signal.aborted) throw error;
                throw new Error(`could not reach ${endpoint.baseUrl}`);
            }
            if (!response.ok)
                throw new Error(
                    `HTTP ${response.status}: ${await response.text()}`,
                );
            return readList(await response.json()).map((listed) => {
                const model = make(listed.id, listed.context ?? DEFAULT_WINDOW);
                if (listed.price !== null)
                    model.cost = {
                        ...listed.price,
                        cacheRead: listed.price.input,
                        cacheWrite: listed.price.input,
                    };
                return Object.assign(model, {
                    listedPrice: listed.price,
                    listedContext: listed.context,
                });
            });
        },
        api: openAICompletionsApi(),
    });
    return {
        ...provider,
        getModels: () =>
            provider
                .getModels()
                .map((model) => atEndpoint(model, endpoint.baseUrl)),
    };
}

/** A provider's stream with billed cost, request-size estimate, timing and safe errors kept on the final message. */
export function protectedProvider(
    home: HomeBase,
    provider: Provider,
    cards: () => Readonly<Record<string, Card>>,
): Provider {
    const prices = (models: readonly Model<string>[]): Model<string>[] =>
        models.map((model) => {
            const card = cards()[`${provider.id}::${model.id}`];
            const cost = card?.price;
            return cost == null
                ? model
                : {
                      ...model,
                      priceKnown: true,
                      cost: {
                          input: cost.input,
                          output: cost.output,
                          cacheRead: cost.input,
                          cacheWrite: cost.input,
                      },
                  };
        });
    const stream: ProviderStreams["stream"] = (model, input, options) =>
        capture(
            home,
            input.messages,
            (observe) =>
                provider.stream(model, input, {
                    ...options,
                    onProviderStreamEvent: observe,
                }),
            options?.onProviderStreamEvent,
        );
    return {
        ...provider,
        getModels: () => prices(provider.getModels()),
        stream,
        streamSimple: (model, input, options) =>
            capture(
                home,
                input.messages,
                (observe) =>
                    provider.streamSimple(model, input, {
                        ...options,
                        onProviderStreamEvent: observe,
                    }),
                options?.onProviderStreamEvent,
            ),
    };
}

function capture(
    home: HomeBase,
    messages: readonly unknown[],
    stream: (
        observe: NonNullable<StreamOptions["onProviderStreamEvent"]>,
    ) => AssistantMessageEventStream,
    previous: StreamOptions["onProviderStreamEvent"],
): AssistantMessageEventStream {
    const started = Date.now();
    let billed: number | undefined;
    let first: number | undefined;
    const source = stream(async (event, model) => {
        const cost = record(record(event).usage).cost;
        if (typeof cost === "number" && Number.isFinite(cost)) billed = cost;
        await previous?.(event, model);
    });
    return protect(
        source,
        started,
        () => billed,
        () => first,
        () => {
            first ??= Date.now() - started;
        },
        home,
        messages,
    );
}

function protect(
    source: AssistantMessageEventStream,
    started: number,
    billed: () => number | undefined,
    first: () => number | undefined,
    saw: () => void,
    home: HomeBase,
    messages: readonly unknown[],
): AssistantMessageEventStream {
    const out = createAssistantMessageEventStream();
    void (async () => {
        for await (const event of source) {
            if (
                event.type === "text_delta" ||
                event.type === "thinking_delta" ||
                event.type === "toolcall_delta"
            )
                saw();
            if (event.type === "done" || event.type === "error") {
                const message =
                    event.type === "done" ? event.message : event.error;
                const cost = billed();
                if (cost !== undefined) {
                    message.usage.cost.total = cost;
                    Object.assign(message, { billedCost: cost });
                }
                Object.assign(message, {
                    firstTokenMs: first() ?? null,
                    estimatedTokens: estimateOf(messages, message),
                });
                message.durationMs ??= Date.now() - started;
                if (message.errorMessage !== undefined)
                    message.errorMessage = safeWords(
                        home,
                        message.errorMessage,
                    );
            }
            out.push(event);
        }
        out.end(await source.result());
    })().catch((error: unknown) =>
        out.end({
            role: "assistant",
            content: [],
            api: "openai-completions",
            provider: "unknown",
            model: "unknown",
            timestamp: started,
            stopReason: "error",
            errorMessage: safeWords(home, String(error)),
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
        }),
    );
    return out;
}

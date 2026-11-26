/**
 * The providers a host of real models serves, as today's harness lists them
 * (`harnesses/pi/src/provider-set.ts`), and the realm's endpoints, each an
 * OpenAI-compatible server (`harnesses/pi/src/providers.ts`, `compatible`).
 */
import {
    type Api,
    createAssistantMessageEventStream,
    type Model,
    type MutableModels,
} from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { openaiProvider } from "@earendil-works/pi-ai/providers/openai";
import { openaiCodexProvider } from "@earendil-works/pi-ai/providers/openai-codex";
import { zaiProvider } from "@earendil-works/pi-ai/providers/zai";
import { ANSWER_SHARE, DEFAULT_WINDOW } from "./compaction.ts";
import type { Held } from "./copies.ts";
import type { Endpoint } from "./messages/agent.ts";

/**
 * The built-in providers this host's pi-ai lists and today's harness, on the
 * pi-ai of `harnesses/pi/package.json`, does not.
 */
const NOT_TODAYS: ReadonlySet<string> = new Set([
    "qwen-token-plan-individual-cn",
]);

/** The names today's harness gives the built-in providers that pi-ai renamed since. */
const TODAYS_NAMES: Readonly<Record<string, string>> = {
    "openai-codex": "OpenAI Codex (legacy)",
};

/** The built-in providers with today's names and sign-in methods. */
export function hostProviders(): Provider[] {
    const openai: Provider = openaiProvider();
    const served = new Set(
        openaiCodexProvider()
            .getModels()
            .map((m) => m.id),
    );
    const result = builtinProviders()
        .filter(
            (p) =>
                p.id !== "zai" &&
                p.id !== "zai-coding-cn" &&
                !NOT_TODAYS.has(p.id),
        )
        .map((p) => ({
            ...p,
            name: TODAYS_NAMES[p.id] ?? p.name,
            auth: {
                ...(p.auth.apiKey === undefined
                    ? {}
                    : { apiKey: p.auth.apiKey }),
                ...(p.id === "openrouter" && p.auth.oauth !== undefined
                    ? { oauth: p.auth.oauth }
                    : {}),
            },
        }));
    if (openai.auth.oauth !== undefined)
        result.push(
            createProvider({
                id: "chatgpt",
                name: "ChatGPT",
                auth: {
                    oauth: {
                        ...openai.auth.oauth,
                        loginLabel: "Continue with ChatGPT",
                    },
                },
                // A ChatGPT sign-in serves only the models of the Codex list,
                // not every model of the API.
                models: openai
                    .getModels()
                    .filter((m) => served.has(m.id))
                    .map((m) => ({ ...m, provider: "chatgpt" })),
                api: {
                    stream: (model, input, options) =>
                        alias(
                            openai.stream(
                                { ...model, provider: "openai" },
                                input,
                                options,
                            ),
                        ),
                    streamSimple: (model, input, options) =>
                        alias(
                            openai.streamSimple(
                                { ...model, provider: "openai" },
                                input,
                                options,
                            ),
                        ),
                },
            }),
        );
    result.push(zai());
    return result;
}

/** Z.ai's address for use paid by the token. */
const ZAI_PAID = "https://api.z.ai/api/paas/v4";

/**
 * Z.ai paid by the token. pi's own Z.ai calls its coding plan's address,
 * which Z.ai keeps for the coding tools it names, so its models are called at
 * the address for use paid by the token instead.
 */
function zai(): Provider {
    const coding: Provider = zaiProvider();
    return createProvider({
        id: "zai",
        name: "Z.ai",
        auth:
            coding.auth.apiKey === undefined
                ? {}
                : { apiKey: coding.auth.apiKey },
        // The highspeed models are served only on the coding plan.
        models: coding
            .getModels()
            .filter((m) => !m.id.endsWith("-highspeed"))
            .map((m) => ({ ...m, baseUrl: ZAI_PAID })),
        api: {
            stream: (model, input, options) =>
                coding.stream(model, input, options),
            streamSimple: (model, input, options) =>
                coding.streamSimple(model, input, options),
        },
    });
}

function alias(source: ReturnType<Provider["stream"]>) {
    const out = createAssistantMessageEventStream();
    void (async () => {
        for await (const event of source) {
            const message =
                event.type === "done"
                    ? event.message
                    : event.type === "error"
                      ? event.error
                      : event.partial;
            message.provider = "chatgpt";
            out.push(event);
        }
        const message = await source.result();
        message.provider = "chatgpt";
        out.end(message);
    })();
    return out;
}

/** The longest answer an endpoint's model gives, as today's (`harnesses/pi/src/models.ts`). */
const MOST_ANSWER = 8192;

/**
 * Serves each endpoint the copies' settings name, with the copy's model among
 * its models when it names one, and drops an endpoint they no longer name.
 * A newly served endpoint's list is asked for.
 */
export function servingEndpoints(models: MutableModels): (held: Held) => void {
    const served = new Map<string, string>();
    return ({ settings, model }) => {
        const endpoints = settings?.endpoints ?? [];
        for (const name of served.keys()) {
            if (!endpoints.some((endpoint) => endpoint.name === name)) {
                models.deleteProvider(name);
                served.delete(name);
            }
        }
        const chosen = model?.model.split("::");
        for (const endpoint of endpoints) {
            const seed =
                chosen?.length === 2 && chosen[0] === endpoint.name
                    ? chosen[1]
                    : undefined;
            const key = `${endpoint.base_url} ${seed ?? ""}`;
            if (served.get(endpoint.name) === key) continue;
            const known = models.getProvider(endpoint.name);
            const seeds = [
                ...(served.has(endpoint.name) && known !== undefined
                    ? known.getModels().map((m) => m.id)
                    : []),
                ...(seed === undefined ? [] : [seed]),
            ];
            models.setProvider(endpointProvider(endpoint, seeds));
            served.set(endpoint.name, key);
            void models.refresh({ providers: [endpoint.name], force: true });
        }
    };
}

const ENDPOINTS = new WeakSet<Provider>();

/** Whether `provider` is one of the realm's endpoints. */
export function isEndpoint(provider: Provider): boolean {
    return ENDPOINTS.has(provider);
}

/** The OpenAI-compatible endpoint, with its list and key sign-in. */
function endpointProvider(
    endpoint: Endpoint,
    seeds: readonly string[],
): Provider {
    const { name, base_url: baseUrl } = endpoint;
    const host = URL.canParse(baseUrl) ? new URL(baseUrl).hostname : "";
    const rate =
        host === "localhost" ||
        host === "[::1]" ||
        host === "0.0.0.0" ||
        host.startsWith("127.")
            ? 0.1
            : 1;
    const make = (id: string, window = DEFAULT_WINDOW) =>
        ({
            id,
            name: id,
            api: "openai-completions",
            provider: name,
            baseUrl,
            reasoning: false,
            input: ["text"],
            cost: {
                input: rate,
                output: rate,
                cacheRead: rate,
                cacheWrite: rate,
            },
            contextWindow: window,
            maxTokens: Math.min(MOST_ANSWER, Math.floor(window * ANSWER_SHARE)),
            compat: {
                supportsStore: false,
                supportsDeveloperRole: false,
                supportsUsageInStreaming: true,
            },
        }) satisfies Model<"openai-completions"> as Model<Api>;
    const provider = createProvider({
        id: name,
        baseUrl,
        models: [...new Set(seeds)].map((id) => make(id)),
        auth: {
            apiKey: {
                name: `${name} key`,
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
                    auth: { apiKey: credential?.key ?? "hamlet-keyless" },
                }),
            },
        },
        api: openAICompletionsApi(),
        fetchModels: async ({ credential, signal }) => {
            let response: Response;
            try {
                response = await fetch(
                    `${baseUrl.replace(/\/+$/, "")}/models`,
                    {
                        signal,
                        headers:
                            credential?.type === "api_key"
                                ? { Authorization: `Bearer ${credential.key}` }
                                : {},
                    },
                );
            } catch (error) {
                if (signal.aborted) throw error;
                throw new Error(`could not reach ${baseUrl}`);
            }
            if (!response.ok)
                throw new Error(
                    `HTTP ${response.status}: ${await response.text()}`,
                );
            const body: unknown = await response.json();
            const object = record(body);
            const array = object.data ?? object.models ?? body;
            if (!Array.isArray(array)) return [];
            return array
                .flatMap((item) => {
                    const value = record(item);
                    const id = value.id ?? value.name ?? value.model;
                    if (typeof id !== "string" || id === "") return [];
                    const window = value.context_length ?? value.context_window;
                    const model = make(
                        id,
                        typeof window === "number" && window > 0
                            ? window
                            : undefined,
                    );
                    const pricing = record(value.pricing);
                    const input = price(pricing.prompt);
                    const output = price(pricing.completion);
                    if (input !== null && output !== null)
                        model.cost = {
                            input,
                            output,
                            cacheRead: input,
                            cacheWrite: input,
                        };
                    return [model];
                })
                .sort((a, b) => a.id.localeCompare(b.id));
        },
    });
    ENDPOINTS.add(provider);
    return provider;
}

function record(value: unknown): Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value)
        ? Object.fromEntries(Object.entries(value))
        : {};
}

function price(value: unknown): number | null {
    if (typeof value !== "number" && typeof value !== "string") return null;
    const number = Number(value);
    return value === "" || !Number.isFinite(number) || number < 0
        ? null
        : number * 1_000_000;
}

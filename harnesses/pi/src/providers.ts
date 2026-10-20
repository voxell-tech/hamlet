// Provider work requested over the link; credentials and lists live only in memory (S7).
import type {
    AuthInteraction,
    CredentialStore,
    Model,
} from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import {
    createModels,
    createProvider,
    type Provider,
} from "@earendil-works/pi-ai/models";
import type { ProviderRequest, ProviderResult } from "./link.ts";
import { compatibleModel } from "./models.ts";
import { providers as supportedProviders } from "./provider-set.ts";

type Result = Omit<ProviderResult, "v" | "id" | "type" | "re">;

/** Runs one provider operation without a credentials or models store. */
export async function providerWork(
    request: ProviderRequest,
    context: {
        credentials: CredentialStore;
        interaction: AuthInteraction;
        providers?: Provider[];
    },
    faux?: Provider,
): Promise<Result> {
    const secrets: string[] = [];
    const signal = (timeout: number) =>
        AbortSignal.any([
            AbortSignal.timeout(timeout),
            ...(context.interaction.signal === undefined
                ? []
                : [context.interaction.signal]),
        ]);
    try {
        const builtins = context.providers ?? supportedProviders();
        if (request.action === "describe") {
            const providers = faux === undefined ? builtins : [faux];
            return {
                providers: providers.map((p) => ({
                    id: p.id,
                    name: p.name,
                    key: p.auth.apiKey?.login !== undefined,
                    subscription:
                        ["chatgpt", "openrouter"].includes(p.id) &&
                        p.auth.oauth !== undefined,
                })),
                models: providers.flatMap((p) =>
                    p.getModels().map((m) => ({ ...m })),
                ),
            };
        }
        const provider =
            request.provider === "faux"
                ? faux
                : request.baseUrl === undefined
                  ? builtins.find((p) => p.id === request.provider)
                  : compatible(request, request.baseUrl);
        if (provider === undefined)
            throw new Error(`Unknown provider: ${request.provider}`);
        const models = createModels({
            authContext: {
                env: async () => undefined,
                fileExists: async () => false,
            },
            credentials: context.credentials,
        });
        models.setProvider(provider);
        if (request.action === "login") {
            if (
                request.method === "subscription" &&
                !["chatgpt", "openrouter"].includes(provider.id)
            )
                throw new Error(
                    `Signing in with ${provider.id} is not offered. Use an API key.`,
                );
            await models.login(
                provider.id,
                request.method === "subscription" ? "oauth" : "api_key",
                {
                    ...context.interaction,
                    prompt: async (prompt) => {
                        const value = await context.interaction.prompt(prompt);
                        secrets.push(value);
                        return value;
                    },
                },
                {
                    agentName: "Hamlet",
                    getDeviceId: () => request.installationId ?? "",
                },
            );
            return {};
        }
        if (request.action === "refresh") {
            const refreshed = await models.refresh({
                providers: [provider.id],
                force: true,
                signal: signal(10_000),
            });
            const error = refreshed.errors.get(provider.id);
            if (error !== undefined) throw error;
            if (refreshed.aborted) throw new Error("in time");
            return {
                models: models.getModels(provider.id).map((m) => ({ ...m })),
            };
        }
        const model = models.getModel(provider.id, request.model ?? "");
        if (model === undefined)
            throw new Error(`no model named ${request.model}`);
        const answer = await models.completeSimple(
            model,
            {
                messages: [
                    {
                        role: "user",
                        content: "Say hello in one word.",
                        timestamp: Date.now(),
                    },
                ],
            },
            { signal: signal(90_000), maxRetries: 0 },
        );
        const text = answer.content
            .flatMap((p) => (p.type === "text" ? [p.text] : []))
            .join("")
            .trim();
        if (
            answer.stopReason === "error" ||
            answer.stopReason === "aborted" ||
            text === ""
        )
            throw new Error(
                answer.errorMessage ?? "the model answered with nothing",
            );
        return { text };
    } catch (error) {
        let text = String(error);
        for (const secret of secrets)
            if (secret) text = text.split(secret).join("[REDACTED]");
        return { error: text };
    }
}

/** The OpenAI-compatible endpoint adapter, including its list and key sign-in. */
function compatible(request: ProviderRequest, baseUrl: string): Provider {
    const host = new URL(baseUrl).hostname;
    const rate =
        host === "localhost" ||
        host === "[::1]" ||
        host === "0.0.0.0" ||
        host.startsWith("127.")
            ? 0.1
            : 1;
    const make = (id: string, window?: number) =>
        compatibleModel(request.provider, id, baseUrl, window, rate);
    const seeds = request.models.map((raw) => ({
        ...make(String(raw.id)),
        ...raw,
        baseUrl,
    })) as Model<"openai-completions">[];
    if (
        request.model !== undefined &&
        !seeds.some((m) => m.id === request.model)
    )
        seeds.push(make(request.model));
    return createProvider({
        id: request.provider,
        baseUrl,
        models: seeds,
        auth: {
            apiKey: {
                name: `${request.provider} key`,
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
                        baseUrl,
                    },
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
                    const context =
                        typeof window === "number" && window > 0
                            ? window
                            : null;
                    const pricing = record(value.pricing);
                    const input = price(pricing.prompt),
                        output = price(pricing.completion);
                    const listedPrice =
                        input === null || output === null
                            ? null
                            : { input, output };
                    const model = make(id, context ?? undefined);
                    if (listedPrice !== null)
                        model.cost = {
                            ...listedPrice,
                            cacheRead: listedPrice.input,
                            cacheWrite: listedPrice.input,
                        };
                    return [
                        Object.assign(model, {
                            listedPrice,
                            listedContext: context,
                        }),
                    ];
                })
                .sort((a, b) => a.id.localeCompare(b.id));
        },
    });
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

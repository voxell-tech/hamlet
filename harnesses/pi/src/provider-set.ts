// Supported sign-ins and the separate ChatGPT credential namespace.
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { openaiProvider } from "@earendil-works/pi-ai/providers/openai";
import { openaiCodexProvider } from "@earendil-works/pi-ai/providers/openai-codex";
import { claudeProvider } from "./claude.ts";

/** The providers a chief signs in to with a subscription (ADR 0006). */
export const SUBSCRIPTIONS = ["chatgpt", "openrouter", "claude"];

/** Built-in providers with Hamlet's supported sign-in methods; Claude too with its folders in `home`. */
export function providers(home?: string): Provider[] {
    const openai: Provider = openaiProvider();
    const served = new Set(
        openaiCodexProvider()
            .getModels()
            .map((m) => m.id),
    );
    const result = builtinProviders()
        .filter((p) => p.id !== "zai" && p.id !== "zai-coding-cn")
        .map((p) => ({
            ...p,
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
                // A ChatGPT sign-in serves only the models of the
                // Codex list, not every model of the API.
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
    const claude = home === undefined ? undefined : claudeProvider(home);
    if (claude !== undefined) result.push(claude);
    return result;
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

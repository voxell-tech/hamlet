// Supported sign-ins and the separate ChatGPT credential namespace.
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { openaiProvider } from "@earendil-works/pi-ai/providers/openai";

/** Built-in providers with Hamlet's supported sign-in methods. */
export function providers(): Provider[] {
    const openai: Provider = openaiProvider();
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
                models: openai
                    .getModels()
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

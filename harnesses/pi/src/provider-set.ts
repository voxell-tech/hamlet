// Supported sign-ins and the separate ChatGPT credential namespace.
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { openaiCodexProvider } from "@earendil-works/pi-ai/providers/openai-codex";
import { zaiProvider } from "@earendil-works/pi-ai/providers/zai";
import { claudeProvider } from "./claude.ts";
import {
    CHATGPT_REFUSAL,
    chatgptHeaders,
    chatgptRefused,
    seen,
} from "./limits.ts";

/** The providers a chief signs in to with a subscription (ADR 0006). */
export const SUBSCRIPTIONS = ["chatgpt", "openrouter", "claude"];

/** Built-in providers with Hamlet's supported sign-in methods; Claude too with its folders in `home`. */
export function providers(home?: string): Provider[] {
    const codex: Provider = openaiCodexProvider();
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
    // ChatGPT through Codex's sign-in and address (ADR 0006): its token
    // also reads the plan's usage, and each answer tells its limits.
    const oauth = codex.auth.oauth;
    if (oauth !== undefined)
        result.push(
            createProvider({
                id: "chatgpt",
                name: "ChatGPT",
                auth: {
                    oauth: {
                        ...oauth,
                        loginLabel: "Continue with ChatGPT",
                        // The browser sign-in, without asking which.
                        login: (interaction, options) =>
                            oauth.login(
                                {
                                    ...interaction,
                                    prompt: (prompt) =>
                                        prompt.type === "select"
                                            ? Promise.resolve("browser")
                                            : interaction.prompt(prompt),
                                },
                                options,
                            ),
                    },
                },
                models: codex
                    .getModels()
                    .map((m) => ({ ...m, provider: "chatgpt" })),
                api: {
                    stream: (model, input, options) =>
                        alias(
                            codex.stream(
                                { ...model, provider: "openai-codex" },
                                input,
                                watched(options),
                            ),
                        ),
                    streamSimple: (model, input, options) =>
                        alias(
                            codex.streamSimple(
                                { ...model, provider: "openai-codex" },
                                input,
                                watched(options),
                            ),
                        ),
                },
            }),
        );
    const claude = home === undefined ? undefined : claudeProvider(home);
    if (claude !== undefined) result.push(claude);
    result.push(zai());
    return result;
}

/** Z.ai's address for use paid by the token. */
const ZAI_PAID = "https://api.z.ai/api/paas/v4";

/**
 * Z.ai paid by the token. pi's own Z.ai calls its coding plan's
 * address, which Z.ai keeps for the coding tools it names, so its
 * models are called at the address for use paid by the token instead.
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

/** `options` that also tell the plan's limits from each answer's headers. */
function watched<O extends { onResponse?: unknown }>(
    options: O | undefined,
): O {
    const before = options?.onResponse as
        | ((
              response: { status: number; headers: Record<string, string> },
              model: unknown,
          ) => unknown)
        | undefined;
    return {
        ...(options as O),
        onResponse: async (
            response: { status: number; headers: Record<string, string> },
            model: unknown,
        ) => {
            const limits = chatgptHeaders(response.headers);
            if (limits !== undefined) seen("chatgpt", limits);
            await before?.(response, model);
        },
    };
}

/** When the ChatGPT plan last refused, so that the next call made after it tells the refusal lifted. */
let refused: number | undefined;

/** `source` as the provider `chatgpt`'s, telling a refusal for the plan's limits as it passes. */
function alias(source: ReturnType<Provider["stream"]>) {
    const started = Date.now();
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
            if (
                event.type === "error" &&
                CHATGPT_REFUSAL.test(message.errorMessage ?? "")
            ) {
                refused = Date.now();
                seen("chatgpt", chatgptRefused());
            } else if (
                event.type === "done" &&
                refused !== undefined &&
                started > refused
            ) {
                // The plan answers a call made since: the refusal
                // lifted. One made before tells nothing of it.
                refused = undefined;
                seen("chatgpt", {
                    checked: Date.now(),
                    state: "ok",
                    windows: [],
                });
            }
            out.push(event);
        }
        const message = await source.result();
        message.provider = "chatgpt";
        out.end(message);
    })();
    return out;
}

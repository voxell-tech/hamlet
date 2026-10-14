import type { AuthInteraction, Credential } from "@earendil-works/pi-ai";
import type { Home } from "./home.ts";
import type { ProviderView } from "./protocol.ts";

/** The providers and login methods Hamlet offers, without credentials (S7, ADR 0006). */
export async function providersOf(home: Home): Promise<ProviderView[]> {
    return Promise.all(
        [...home.models.getProviders()]
            .sort((a, b) => a.id.localeCompare(b.id))
            .map(async (provider) => ({
                id: provider.id,
                name: provider.name,
                configured:
                    (await home.models.checkAuth(provider.id)) !== undefined,
                methods: [
                    ...(provider.auth.apiKey?.login === undefined
                        ? []
                        : ["key" as const]),
                    ...(provider.id === "chatgpt" ||
                    provider.id === "openrouter"
                        ? ["subscription" as const]
                        : []),
                ],
                label:
                    provider.id === "chatgpt"
                        ? "Continue with ChatGPT"
                        : provider.id === "openrouter"
                          ? "Sign in with OpenRouter"
                          : provider.name,
            })),
    );
}

/** A key or permitted sign-in, stored only in the home's credentials file. */
export async function login(
    home: Home,
    provider: string,
    method: "key" | "subscription",
    interaction: AuthInteraction,
): Promise<Credential> {
    if (
        method === "subscription" &&
        provider !== "chatgpt" &&
        provider !== "openrouter"
    )
        throw new Error(
            `Signing in with ${provider} is not offered. Use an API key.`,
        );
    try {
        return await home.models.login(
            provider,
            method === "key" ? "api_key" : "oauth",
            interaction,
            {
                agentName: "Hamlet",
                getDeviceId: () => home.credentials.installationId,
            },
        );
    } catch (error) {
        if (/1455|EADDRINUSE/.test(String(error)))
            throw new Error(
                "Port 1455 is in use, probably by the Codex CLI or another sign-in. Cancel that sign-in and try again.",
            );
        throw error;
    }
}

import type { AuthInteraction, Credential } from "@earendil-works/pi-ai";
import type { Home } from "../home.ts";
import type { ProviderView } from "../protocol.ts";
import { err, ok, type Result } from "../result.ts";
import type { LoginFailure } from "./failure.ts";

const subscriptions: ReadonlySet<string> = new Set(["chatgpt", "openrouter"]);
/** Provider sign-ins are offered only for Hamlet's supported subscriptions. */
export function subscription(provider: string): boolean {
    return subscriptions.has(provider);
}

/** Provider views never expose credentials or ambient auth sources. */
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
                    ...(subscription(provider.id) &&
                    provider.auth.oauth !== undefined
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

/** A permitted sign-in, with transient prompt secrets scrubbed from refusals. */
export async function login(
    home: Home,
    provider: string,
    method: "key" | "subscription",
    interaction: AuthInteraction,
): Promise<Result<Credential, LoginFailure>> {
    if (method === "subscription" && !subscription(provider))
        return err({
            kind: "method-not-offered",
            operation: "sign-in",
            provider,
            retrySafe: false,
        });
    const secrets: string[] = [];
    try {
        const credential = await home.models.login(
            provider,
            method === "key" ? "api_key" : "oauth",
            {
                ...interaction,
                prompt: async (prompt) => {
                    const value = await interaction.prompt(prompt);
                    if (
                        prompt.type === "secret" ||
                        prompt.type === "manual_code"
                    )
                        secrets.push(value);
                    return value;
                },
            },
            {
                agentName: "Hamlet",
                getDeviceId: () => home.credentials.installationId,
            },
        );
        return ok(credential);
    } catch (error) {
        // SAFETY: pi-ai's sign-in callback does not retain Node's socket error type; its port/code text identifies the expected bind failure.
        // Verified by: models-connection.test.ts classifies a sign-in whose callback port is already in use.
        if (/1455|EADDRINUSE/.test(String(error)))
            return err({
                kind: "port-in-use",
                operation: "sign-in",
                provider,
                retrySafe: false,
            });
        return err({
            kind: "provider-refused",
            operation: "sign-in",
            provider,
            retrySafe: false,
            reason: home.secrets.value(
                error instanceof Error ? error.message : String(error),
                secrets,
            ),
        });
    }
}
export { type LoginFailure, loginFailureText } from "./failure.ts";

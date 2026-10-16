import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { err, ok, type Result } from "../result.ts";
import type { Endpoint, Settings } from "../settings.ts";
import { endpointName } from "./endpoints.ts";
import type { SettingsFailure } from "./failure.ts";

const builtins: ReadonlySet<string> = new Set([
    ...builtinProviders().map((provider) => provider.id),
    "chatgpt",
]);
/** Fields a chief may save, without overwriting the connection view. */
export type SettingsEdit = Partial<
    Pick<Settings, "model" | "reflectEvery" | "allowNet" | "endpoints">
>;

/** Establishes endpoint identity and address constraints before any side effects. */
export function validateSettings(
    set: SettingsEdit,
): Result<void, SettingsFailure> {
    const facts = { operation: "edit-settings", retrySafe: true } as const;
    if (
        set.reflectEvery !== undefined &&
        (!Number.isSafeInteger(set.reflectEvery) || set.reflectEvery < 0)
    )
        return err({ ...facts, kind: "reflect-every-not-whole" });
    const names = validateEndpointNames(set.endpoints ?? []);
    if (!names.ok) return names;
    for (const endpoint of set.endpoints ?? []) {
        let url: URL;
        try {
            url = new URL(endpoint.baseUrl);
        } catch {
            return err({
                ...facts,
                kind: "address-scheme",
                reason: "invalid-url",
            });
        }
        if (url.protocol !== "http:" && url.protocol !== "https:")
            return err({ ...facts, kind: "address-scheme", reason: "scheme" });
        if (url.username || url.password)
            return err({ ...facts, kind: "address-has-credentials" });
    }
    return ok(undefined);
}
/** Endpoint identities checked during sync, independently of process-only address overrides. */
export function validateEndpointNames(
    endpoints: readonly Endpoint[],
): Result<void, SettingsFailure> {
    const facts = { operation: "edit-settings", retrySafe: true } as const;
    const names = new Set<string>();
    for (const endpoint of endpoints) {
        if (!endpointName(endpoint.name))
            return err({
                ...facts,
                kind: "endpoint-name-invalid",
                name: endpoint.name,
            });
        if (builtins.has(endpoint.name))
            return err({
                ...facts,
                kind: "endpoint-builtin",
                name: endpoint.name,
            });
        if (names.has(endpoint.name))
            return err({
                ...facts,
                kind: "endpoint-duplicate",
                name: endpoint.name,
            });
        names.add(endpoint.name);
    }
    return ok(undefined);
}
export { type SettingsFailure, settingsFailureText } from "./failure.ts";

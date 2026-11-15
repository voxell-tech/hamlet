import type { AnyActorDefinition } from "../actor/definition.ts";
import { RegistryConfigSchema } from "./config/index.ts";
import { loadNapiRuntime } from "./napi-runtime.ts";
import { buildRegistryWithRuntime, callNative } from "./native.ts";

/**
 * Runs this process as the Node host of the Rust host that started it: the
 * actors of `kinds`, by kind, served over the link until the Rust host's
 * goodbye. The process takes the link first, with the native module's
 * `takeLink`, before it loads this module.
 *
 * Rejects with an `ActorError` when the Rust host refuses this host's kinds,
 * when another Node host holds the realm folder, and when the link was not
 * taken. A failure with no code of its own is `core.internal_error`, with its
 * words in `metadata.error`.
 */
export async function serveLink(
    kinds: Readonly<Record<string, AnyActorDefinition>>,
): Promise<void> {
    const { runtime } = await loadNapiRuntime();
    const { registry } = await buildRegistryWithRuntime(
        RegistryConfigSchema.parse({ use: kinds }),
        runtime,
    );
    await callNative(() => runtime.serveLink(registry));
}

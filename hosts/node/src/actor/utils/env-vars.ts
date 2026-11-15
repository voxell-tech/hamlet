// This file was changed for Hamlet. See NOTICE in this package.
// This file consolidates all environment variables that affect the runtime's behavior.
//
// IMPORTANT: When adding or modifying environment variables here, also update the
// documentation at: website/src/content/docs/general/environment-variables.mdx

import { getEnvUniversal } from "../utils.ts";

// Engine configuration
export const getActorEngine = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_ENGINE");
export const getActorEndpoint = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_ENDPOINT");
export const getActorToken = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_TOKEN");
export const getActorNamespace = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_NAMESPACE");
export const getActorPool = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_POOL");
export const getActorTotalSlots = (): number | undefined => {
    const value = getEnvUniversal("HAMLET_ACTOR_TOTAL_SLOTS");
    return value !== undefined ? parseInt(value, 10) : undefined;
};
export const getActorRunEngine = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_RUN_ENGINE") === "1";
export const getActorRunEngineHost = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_RUN_ENGINE_HOST");
export const getActorRunEnginePort = (): number | undefined => {
    const value = getEnvUniversal("HAMLET_ACTOR_RUN_ENGINE_PORT");
    return value !== undefined ? parseInt(value, 10) : undefined;
};
export const getActorRunEngineVersion = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_RUN_ENGINE_VERSION");
export const getActorRunServices = (): boolean | undefined => {
    const value = getEnvUniversal("HAMLET_ACTOR_RUN_SERVICES");
    return value === undefined ? undefined : value === "1";
};
export const getActorEnvoyKind = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_NODE_KIND") ??
    getEnvUniversal("HAMLET_ACTOR_ENVOY_KIND");
export const getActorEnvoyVersion = (): number | undefined => {
    // The legacy platform deployment still sets the old name.
    const value =
        getEnvUniversal("HAMLET_ACTOR_NODE_VERSION") ??
        getEnvUniversal("HAMLET_ACTOR_ENVOY_VERSION");
    return value !== undefined ? parseInt(value, 10) : undefined;
};
export const getActorPublicEndpoint = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_PUBLIC_ENDPOINT");
export const getActorPublicToken = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_PUBLIC_TOKEN");
// There is no HAMLET_ACTOR_PUBLIC_NAMESPACE because the frontend and backend cannot
// use different namespaces

// Runtime configuration
export const getActorInspectorDisable = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_INSPECTOR_DISABLE") === "1";
export const getActorStoragePath = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_STORAGE_PATH");
export const getActorRuntime = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_RUNTIME");
export type RuntimeMode = "node" | "serverless";

export const getActorRuntimeMode = (): RuntimeMode => {
    const value = getEnvUniversal("HAMLET_ACTOR_RUNTIME_MODE");
    if (value === undefined || value === "envoy") return "node";
    // Serverless is disabled for v3.
    if (value === "serverless") {
        throw new Error("serverless is disabled in this version");
    }
    if (value === "node") return value;
    throw new Error(
        `HAMLET_ACTOR_RUNTIME_MODE env var must be "node" or "serverless"; got "${value}"`,
    );
};

export const getActorPublicDir = (): string | undefined => {
    const value = getEnvUniversal("HAMLET_ACTOR_PUBLIC_DIR");
    return value === undefined || value === "" ? undefined : value;
};

export function parsePortEnv(raw: string | undefined): number | undefined {
    if (raw === undefined || raw === "") return undefined;
    const parsed = Number.parseInt(raw, 10);
    if (
        !Number.isFinite(parsed) ||
        parsed < 1 ||
        parsed > 65535 ||
        String(parsed) !== raw.trim()
    ) {
        throw new Error(
            `HAMLET_ACTOR_PORT env var must be an integer between 1 and 65535; got "${raw}"`,
        );
    }
    return parsed;
}

// Logging configuration
// DEPRECATED: LOG_LEVEL will be removed in a future version
export const getLogLevel = (): string | undefined =>
    getEnvUniversal("HAMLET_ACTOR_LOG_LEVEL") ?? getEnvUniversal("LOG_LEVEL");
export const getLogTarget = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_LOG_TARGET") === "1";
export const getLogTimestamp = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_LOG_TIMESTAMP") === "1";
export const getLogMessage = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_LOG_MESSAGE") === "1";
export const getLogErrorStack = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_LOG_ERROR_STACK") === "1";
export const getLogHeaders = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_LOG_HEADERS") === "1";

// Environment configuration
export const getNodeEnv = (): string | undefined => getEnvUniversal("NODE_ENV");
export const getNextPhase = (): string | undefined =>
    getEnvUniversal("NEXT_PHASE");
export const isDev = (): boolean => getNodeEnv() !== "production";

// Experimental
/**
 * Enables experimental OTel tracing for actors.
 *
 * When disabled, actors use an in-memory no-op traces implementation.
 */
export const getActorExperimentalOtel = (): boolean =>
    getEnvUniversal("HAMLET_ACTOR_EXPERIMENTAL_OTEL") === "1";

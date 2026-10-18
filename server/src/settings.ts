import { defineDoc, type ModelRef } from "@earendil-works/pi-durable";
import type { Limits } from "./limits/rules.ts";
import { MAYOR } from "./record/document.ts";
import { err, ok, type Result } from "./result.ts";

/** A place models are served, named for the models it serves. */
export type Endpoint = {
    name: string;
    /** OpenAI-compatible, such as `http://localhost:11434/v1`. */
    baseUrl: string;
};

/** The outcome of the last check of the connection. */
export type Link = {
    state: "untested" | "testing" | "connected" | "failed";
    /** The models the default model's endpoint listed. */
    models: number | null;
    /** The check's finding, in words for a chief. */
    words: string | null;
};

/** The settings of a home, which a chief edits. */
export type Settings = {
    /** The model that runs where nobody chose one, as `endpoint::id`. */
    model: string;
    /** Chief turns between reflections; 0 never reflects (C5). */
    reflectEvery: number;
    /** Network for every agent's tools (C19). */
    allowNet: boolean;
    endpoints: Endpoint[];
    link: Link;
    /** True once a chief has saved settings of the home's own. */
    saved: boolean;
};

/** The home's settings. */
export const SettingsDoc = defineDoc<Settings>({
    kind: "hamlet.settings",
    version: 1,
    scope: "session",
    initial: () => ({
        model: "local::qwen3.6:35b-a3b",
        reflectEvery: 10,
        allowNet: false,
        endpoints: [{ name: "local", baseUrl: "http://localhost:11434/v1" }],
        link: { state: "untested", models: null, words: null },
        saved: false,
    }),
});

/** The address and token of a remote workspace's manager. */
export type WorkspaceConnection = {
    /** HTTPS, or HTTP on loopback. */
    readonly url: string;
    /** The bearer token the manager accepts, kept with the credentials (S7). */
    readonly token: string;
};

/** Why a workspace connection was refused. */
export type WorkspaceRefusal =
    | { kind: "workspace-address" }
    | { kind: "workspace-token" };

/** The connection `url` and `token` name, once both meet the manager's rules. */
export function workspaceConnection(
    url: string,
    token: string,
): Result<WorkspaceConnection, WorkspaceRefusal> {
    let address: URL;
    try {
        address = new URL(url);
    } catch {
        return err({ kind: "workspace-address" });
    }
    const loopback = ["127.0.0.1", "[::1]", "localhost"].includes(
        address.hostname,
    );
    if (
        (address.protocol !== "https:" &&
            !(address.protocol === "http:" && loopback)) ||
        address.username ||
        address.password ||
        address.search ||
        address.hash ||
        address.port === "0"
    )
        return err({ kind: "workspace-address" });
    if (token.length < 16) return err({ kind: "workspace-token" });
    return ok({ url, token });
}

/** A chief's words for a refused workspace connection. */
export function workspaceRefusalText(refusal: WorkspaceRefusal): string {
    switch (refusal.kind) {
        case "workspace-address":
            return "The workspace address must start with https://, or http:// for localhost, and have no password or query.";
        case "workspace-token":
            return "The workspace token needs at least 16 characters.";
    }
    const unhandled: never = refusal;
    throw new Error(`Unclassified workspace refusal: ${unhandled}`);
}

/** Settings that `HAMLET_*` environment variables give this process alone, never written down. */
export type Overrides = {
    readonly model?: string;
    readonly reflectEvery?: number;
    readonly allowNet?: boolean;
    /** The address of the endpoint that serves the default model. */
    readonly baseUrl?: string;
    /** The key of the endpoint that serves the default model. */
    readonly apiKey?: string;
    /** The remote workspace for this process, in place of the one chosen in Settings. */
    readonly workspace?: WorkspaceConnection;
};

/** The overrides `env` sets; an empty variable sets nothing. */
export function readOverrides(
    env: Readonly<Record<string, string | undefined>>,
): Overrides {
    const set = (name: string) => env[name] || undefined;
    const every = set("HAMLET_REFLECT_EVERY");
    // Rust's usize parsing, which takes a leading plus.
    if (every !== undefined && !/^\+?[0-9]+$/.test(every)) {
        throw new Error(
            `HAMLET_REFLECT_EVERY must be a whole number, not \`${every}\``,
        );
    }
    const net = set("HAMLET_NET");
    const model = set("HAMLET_MODEL");
    const baseUrl = set("HAMLET_BASE_URL");
    const apiKey = set("HAMLET_API_KEY");
    const workspace = readWorkspace(
        set("HAMLET_WORKSPACE_URL"),
        set("HAMLET_WORKSPACE_TOKEN"),
    );
    return {
        ...(model === undefined ? {} : { model }),
        ...(every === undefined ? {} : { reflectEvery: Number(every) }),
        ...(net === undefined ? {} : { allowNet: net === "1" }),
        ...(baseUrl === undefined ? {} : { baseUrl }),
        ...(apiKey === undefined ? {} : { apiKey }),
        ...(workspace === undefined ? {} : { workspace }),
    };
}

function readWorkspace(
    url: string | undefined,
    token: string | undefined,
): WorkspaceConnection | undefined {
    if (url === undefined && token === undefined) return undefined;
    if (url === undefined || token === undefined)
        throw new Error(
            "HAMLET_WORKSPACE_URL and HAMLET_WORKSPACE_TOKEN are set together, or neither is.",
        );
    const connection = workspaceConnection(url, token);
    if (!connection.ok)
        throw new Error(
            `HAMLET_WORKSPACE_URL and HAMLET_WORKSPACE_TOKEN were refused. ${workspaceRefusalText(connection.error)}`,
        );
    return connection.value;
}

/** The settings in effect: those stored, the defaults where there are none, under the overrides. */
export function settingsOf(
    stored: Readonly<Settings> | undefined,
    overrides: Overrides,
): Settings {
    const settings = stored ?? SettingsDoc.definition.initial();
    const model = overrides.model ?? settings.model;
    const serving = modelRef(model)?.provider;
    const { baseUrl } = overrides;
    return {
        model,
        reflectEvery: overrides.reflectEvery ?? settings.reflectEvery,
        allowNet: overrides.allowNet ?? settings.allowNet,
        endpoints: settings.endpoints.map((endpoint) =>
            endpoint.name === serving && baseUrl !== undefined
                ? { name: endpoint.name, baseUrl }
                : { name: endpoint.name, baseUrl: endpoint.baseUrl },
        ),
        link: { ...settings.link },
        saved: settings.saved,
    };
}

/** The model a name `endpoint::id` gives, or `undefined` for a name of no endpoint. */
export function modelRef(name: string): ModelRef | undefined {
    const at = name.indexOf("::");
    if (at <= 0 || at + 2 === name.length) return undefined;
    return { provider: name.slice(0, at), modelId: name.slice(at + 2) };
}

/** The name `endpoint::id` of a model. */
export function modelName(model: ModelRef): string {
    return `${model.provider}::${model.modelId}`;
}

/** The model an agent runs: the chief's pin, an allowed mayor choice, the hamlet's model or the home's default (H29). */
export function choose(
    limits: Readonly<Limits>,
    settings: Readonly<Settings>,
    agent: { name: string; mayor: boolean; model?: string | null },
): string {
    const name = agent.mayor ? MAYOR : agent.name;
    const pinned = Object.hasOwn(limits.pinned, name)
        ? limits.pinned[name]
        : undefined;
    return (
        pinned ??
        (agent.model && limits.models.includes(agent.model)
            ? agent.model
            : undefined) ??
        limits.model ??
        settings.model
    );
}

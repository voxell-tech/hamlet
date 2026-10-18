import { defineDoc, type ModelRef } from "@earendil-works/pi-durable";
import { type Static, Type } from "typebox";
import { Value } from "typebox/value";
import type { Limits } from "./limits/rules.ts";
import { MAYOR } from "./record/document.ts";

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

/** Private connection to the workspace manager; no infrastructure credentials belong in Hamlet. */
export const WorkspaceSettings = Type.Object(
    {
        url: Type.String({ minLength: 1 }),
        token: Type.String({ minLength: 16 }),
    },
    { additionalProperties: false },
);
export type WorkspaceSettings = Static<typeof WorkspaceSettings>;

/** Settings that `HAMLET_*` environment variables give this process alone, never written down. */
export type Overrides = {
    readonly model?: string;
    readonly reflectEvery?: number;
    readonly allowNet?: boolean;
    /** The address of the endpoint that serves the default model. */
    readonly baseUrl?: string;
    /** The key of the endpoint that serves the default model. */
    readonly apiKey?: string;
    /** Explicitly routes ordinary tools to the workspace manager. */
    readonly workspace?: WorkspaceSettings;
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
    if (set("HAMLET_SUBSTRATE") !== undefined)
        throw new Error(
            "HAMLET_SUBSTRATE is unsupported. Configure HAMLET_WORKSPACE with the separate manager's URL and token.",
        );
    const rawWorkspace = set("HAMLET_WORKSPACE");
    let workspace: WorkspaceSettings | undefined;
    if (rawWorkspace !== undefined) {
        let value: unknown;
        try {
            value = JSON.parse(rawWorkspace);
        } catch {
            throw new Error(
                "HAMLET_WORKSPACE must be a JSON connection configuration.",
            );
        }
        if (!Value.Check(WorkspaceSettings, value))
            throw new Error(
                "HAMLET_WORKSPACE needs a manager URL and token of at least 16 characters.",
            );
        try {
            const url = new URL(value.url);
            const loopback = ["127.0.0.1", "[::1]", "localhost"].includes(
                url.hostname,
            );
            if (
                (url.protocol !== "https:" &&
                    !(url.protocol === "http:" && loopback)) ||
                url.username ||
                url.password ||
                url.search ||
                url.hash ||
                url.port === "0"
            )
                throw new Error("Invalid manager URL");
        } catch {
            throw new Error(
                "HAMLET_WORKSPACE needs HTTPS or loopback HTTP without URL credentials or query parameters.",
            );
        }
        workspace = value;
    }
    return {
        ...(model === undefined ? {} : { model }),
        ...(every === undefined ? {} : { reflectEvery: Number(every) }),
        ...(net === undefined ? {} : { allowNet: net === "1" }),
        ...(baseUrl === undefined ? {} : { baseUrl }),
        ...(apiKey === undefined ? {} : { apiKey }),
        ...(workspace === undefined ? {} : { workspace }),
    };
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

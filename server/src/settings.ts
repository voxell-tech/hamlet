import { defineDoc, type ModelRef } from "@earendil-works/pi-durable";

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

/** Settings that `HAMLET_*` environment variables give this process alone, never written down. */
export type Overrides = {
    readonly model?: string;
    readonly reflectEvery?: number;
    readonly allowNet?: boolean;
    /** The address of the endpoint that serves the default model. */
    readonly baseUrl?: string;
    /** The key of the endpoint that serves the default model. */
    readonly apiKey?: string;
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
    return {
        ...(model === undefined ? {} : { model }),
        ...(every === undefined ? {} : { reflectEvery: Number(every) }),
        ...(net === undefined ? {} : { allowNet: net === "1" }),
        ...(baseUrl === undefined ? {} : { baseUrl }),
        ...(apiKey === undefined ? {} : { apiKey }),
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

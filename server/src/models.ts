import type { Context } from "@earendil-works/chord";
import {
    type AuthContext,
    createAssistantMessageEventStream,
} from "@earendil-works/pi-ai";
import {
    createModels,
    createProvider,
    type MutableModels,
    type Provider,
} from "@earendil-works/pi-ai/models";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { openaiProvider } from "@earendil-works/pi-ai/providers/openai";
import { defineExtension } from "@earendil-works/pi-durable";
import type { Home, HomeBase, Part } from "./home.ts";
import { LimitsDoc, limitsOf } from "./limits/rules.ts";
import {
    type Card,
    CatalogueDoc,
    type Price,
    takeIn,
} from "./models/catalogue.ts";
import { choose, commitEdit } from "./models/choice.ts";
import { endpointName } from "./models/endpoints.ts";
import { plainly } from "./models/plainly.ts";
import { compatible, protectedProvider } from "./models/providers.ts";
import { FileModelsStore } from "./models/store.ts";
import { liveHamlets, RealmDoc } from "./realm/places.ts";
import { RecordDoc } from "./record.ts";
import { safeWords } from "./scrub.ts";
import {
    modelRef,
    type Settings,
    SettingsDoc,
    settingsOf,
} from "./settings.ts";

const NOTHING_AMBIENT: AuthContext = {
    env: async () => undefined,
    fileExists: async () => false,
};
const owned = new WeakSet<MutableModels>();
const overrides = new WeakMap<MutableModels, Record<string, Card>>();
const custom = new WeakMap<MutableModels, Map<string, string>>();
const stores = new WeakMap<MutableModels, FileModelsStore>();
const keySources = new WeakMap<
    MutableModels,
    { provider: string | undefined }
>();
const builtins = new Set(builtinProviders().map((provider) => provider.id));

/** The home's models, with no ambient credentials and persisted provider lists (S7). */
export function homeModels(home: HomeBase): MutableModels {
    const store = new FileModelsStore(home.layout.models);
    const source = {
        provider: modelRef(
            home.overrides.model ?? SettingsDoc.definition.initial().model,
        )?.provider,
    };
    const models = createModels({
        credentials: {
            read: async (provider) =>
                home.overrides.apiKey !== undefined &&
                provider === source.provider
                    ? { type: "api_key", key: home.overrides.apiKey }
                    : home.credentials.read(provider),
            list: () => home.credentials.list(),
            modify: (provider, change) =>
                home.credentials.modify(provider, change),
            delete: (provider) => home.credentials.delete(provider),
        },
        authContext: NOTHING_AMBIENT,
        modelsStore: store,
    });
    owned.add(models);
    stores.set(models, store);
    keySources.set(models, source);
    for (const provider of builtinProviders()) {
        if (provider.id === "zai" || provider.id === "zai-coding-cn") continue;
        const oauth =
            provider.id === "openrouter"
                ? (home.logins.openrouter ?? provider.auth.oauth)
                : undefined;
        models.setProvider(
            protectedProvider(
                home,
                {
                    ...provider,
                    auth: {
                        ...(provider.auth.apiKey === undefined
                            ? {}
                            : { apiKey: provider.auth.apiKey }),
                        ...(oauth === undefined ? {} : { oauth }),
                    },
                },
                () => overrides.get(models) ?? {},
            ),
        );
    }
    const openai: Provider = openaiProvider();
    const oauth = home.logins.chatgpt ?? openai.auth.oauth;
    if (oauth !== undefined) {
        const alias = createProvider({
            id: "chatgpt",
            name: "ChatGPT",
            auth: { oauth: { ...oauth, loginLabel: "Continue with ChatGPT" } },
            models: openai
                .getModels()
                .map((model) => ({ ...model, provider: "chatgpt" })),
            api: {
                stream: (model, input, options) =>
                    aliasStream(
                        openai.stream(
                            { ...model, provider: "openai" },
                            input,
                            options,
                        ),
                    ),
                streamSimple: (model, input, options) =>
                    aliasStream(
                        openai.streamSimple(
                            { ...model, provider: "openai" },
                            input,
                            options,
                        ),
                    ),
            },
        });
        models.setProvider(
            protectedProvider(home, alias, () => overrides.get(models) ?? {}),
        );
    }
    return models;
}
function aliasStream(source: ReturnType<Provider["streamSimple"]>) {
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

/** Installs the endpoints as they are now, without replacing injected test providers. */
export async function syncProviders(
    home: Home,
    settings: Settings,
    context: Context,
): Promise<void> {
    if (!owned.has(home.models)) return;
    overrides.set(home.models, {
        ...(await home.harness.snapshot(CatalogueDoc, context))?.models,
    });
    const previous = custom.get(home.models) ?? new Map<string, string>();
    const serving = modelRef(settings.model);
    const source = keySources.get(home.models);
    if (source !== undefined) source.provider = serving?.provider;
    const choices = [serving];
    for (const { id } of liveHamlets(
        await home.harness.snapshot(RealmDoc, context),
    )) {
        const limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, id, context),
        );
        const record = await home.harness.snapshot(RecordDoc, id, context);
        for (const agent of record?.agents ?? [])
            choices.push(
                modelRef(
                    choose(limits, settings, {
                        name: agent.name,
                        mayor: agent.number === record?.mayor,
                        model: agent.model,
                    }),
                ),
            );
    }
    const saved = await home.harness.snapshot(SettingsDoc, context);
    for (const endpoint of settings.endpoints) {
        const store = stores.get(home.models);
        store?.endpoint(
            endpoint.name,
            saved?.endpoints.find((each) => each.name === endpoint.name)
                ?.baseUrl ?? endpoint.baseUrl,
        );
        const seeds = [
            ...new Set(
                choices.flatMap((ref) =>
                    ref?.provider === endpoint.name ? [ref.modelId] : [],
                ),
            ),
        ];
        const stamp = `${endpoint.baseUrl}:${seeds.join(",")}`;
        if (previous.get(endpoint.name) === stamp) continue;
        if (
            !endpointName(endpoint.name) ||
            builtins.has(endpoint.name) ||
            endpoint.name === "chatgpt"
        )
            throw new Error(
                `\`${endpoint.name}\` is a built-in provider or not an endpoint name.`,
            );
        const provider = compatible(endpoint, seeds);
        const stored = await store?.read(endpoint.name);
        const missing = provider
            .getModels()
            .filter(
                (model) => !stored?.models.some((each) => each.id === model.id),
            );
        if (missing.length > 0)
            await store?.write(endpoint.name, {
                ...stored,
                models: [...(stored?.models ?? []), ...missing],
            });
        home.models.setProvider(
            protectedProvider(
                home,
                provider,
                () => overrides.get(home.models) ?? {},
            ),
        );
        previous.set(endpoint.name, stamp);
    }
    for (const name of previous.keys()) {
        if (settings.endpoints.some((endpoint) => endpoint.name === name))
            continue;
        home.models.deleteProvider(name);
        previous.delete(name);
    }
    custom.set(home.models, previous);
}

/** Refreshes Hamlet-made providers, bounded before resume, retaining their last-known lists on failure. */
export async function refreshModels(
    home: Home,
    context: Context,
    timeout = 10_000,
): Promise<void> {
    await syncProviders(
        home,
        settingsOf(
            await home.harness.snapshot(SettingsDoc, context),
            home.overrides,
        ),
        context,
    );
    const providers = [...(custom.get(home.models)?.keys() ?? [])];
    if (providers.length === 0) return;
    const result = await home.models.refresh({
        providers,
        force: true,
        signal: AbortSignal.timeout(timeout),
    });
    for (const provider of providers) {
        const models = home.models.getModels(provider);
        const url = home.models.getProvider(provider)?.baseUrl ?? "";
        await takeIn(
            home,
            provider,
            url,
            models.map((model) => {
                const listing = model as typeof model & {
                    listedPrice?: Price | null;
                    listedContext?: number | null;
                };
                return {
                    id: model.id,
                    context:
                        listing.listedContext === undefined
                            ? model.contextWindow
                            : listing.listedContext,
                    price: listing.listedPrice ?? null,
                };
            }),
            context,
        );
    }
    const settings = settingsOf(
        await home.harness.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const serving = modelRef(settings.model)?.provider;
    const error =
        serving === undefined ? undefined : result.errors.get(serving);
    await home.harness.commit(async (tx) => {
        (await tx.doc(SettingsDoc)).link =
            error !== undefined || result.aborted
                ? {
                      state: "failed",
                      models: null,
                      words:
                          error === undefined
                              ? "It took too long to answer."
                              : plainly(safeWords(home, error.message)),
                  }
                : {
                      state: "connected",
                      models:
                          serving === undefined
                              ? 0
                              : home.models.getModels(serving).length,
                      words: `Connected, ${serving === undefined ? 0 : home.models.getModels(serving).length} model(s)`,
                  };
    }, context);
    overrides.set(home.models, {
        ...(await home.harness.snapshot(CatalogueDoc, context))?.models,
    });
}

/** A chief's saved settings, with removed endpoints' credentials forgotten too. */
export async function editSettings(
    home: Home,
    set: Partial<
        Pick<Settings, "model" | "reflectEvery" | "allowNet" | "endpoints">
    >,
    context: Context,
): Promise<void> {
    if (
        set.reflectEvery !== undefined &&
        (!Number.isSafeInteger(set.reflectEvery) || set.reflectEvery < 0)
    )
        throw new Error("reflectEvery must be a whole number");
    if (set.endpoints !== undefined) {
        const names = new Set<string>();
        for (const endpoint of set.endpoints) {
            if (
                !endpointName(endpoint.name) ||
                builtins.has(endpoint.name) ||
                endpoint.name === "chatgpt" ||
                names.has(endpoint.name)
            )
                throw new Error(
                    `\`${endpoint.name}\` is not a unique custom endpoint name.`,
                );
            names.add(endpoint.name);
            const url = new URL(endpoint.baseUrl);
            if (url.protocol !== "http:" && url.protocol !== "https:")
                throw new Error(
                    "The address should start with https:// or http://.",
                );
            if (url.username || url.password)
                throw new Error(
                    "Keep credentials out of an endpoint's address.",
                );
        }
    }
    const before =
        (await home.harness.snapshot(SettingsDoc, context)) ??
        SettingsDoc.definition.initial();
    const next: Settings = { ...before, ...set, saved: true };
    await syncProviders(home, settingsOf(next, home.overrides), context);
    await home.harness.commit(
        (tx) =>
            commitEdit(home, tx, {
                by: home.chief,
                change: async (tx) => {
                    Object.assign(await tx.doc(SettingsDoc), next);
                },
            }),
        context,
    );
    for (const endpoint of before.endpoints)
        if (!next.endpoints.some((each) => each.name === endpoint.name))
            await home.credentials.delete(endpoint.name);
}

/** Keeps provider links synchronized with committed settings and closes refresh work. */
export const Providers: Part = (home) => {
    const stops: (() => Promise<unknown>)[] = [];
    return {
        extension: defineExtension({ name: "hamlet.models" }),
        async start(context) {
            await syncProviders(
                home,
                settingsOf(
                    await home.harness.snapshot(SettingsDoc, context),
                    home.overrides,
                ),
                context,
            );
            await home.models.refresh({ allowNetwork: false });
            await refreshModels(home, context, 1500);
            if (!owned.has(home.models)) return;
            const settings = await home.harness.watchDoc(SettingsDoc, context);
            settings?.start(async (value, _ops, context) =>
                syncProviders(
                    home,
                    settingsOf(value ?? undefined, home.overrides),
                    context,
                ),
            );
            if (settings !== undefined) stops.push(() => settings.stop());
            const unwatch = home.harness.subscribeCommits(({ changes }) => {
                for (const change of changes) {
                    if (
                        change.type === "document" &&
                        change.record.kind === CatalogueDoc.definition.kind &&
                        change.value !== null
                    ) {
                        overrides.set(home.models, {
                            ...(change.value.models as Record<string, Card>),
                        });
                    }
                }
            });
            stops.push(async () => unwatch());
        },
        async stop() {
            for (const stop of stops) await stop();
        },
    };
};

export {
    type Card,
    CatalogueDoc,
    catalogueOf,
    editCatalogue,
    type Listed,
    type Price,
    readList,
    record,
    takeIn,
} from "./models/catalogue.ts";
export { checkConnection, checkModel, namedModel } from "./models/check.ts";
export {
    Choice,
    choose,
    commitEdit,
    configureAgent,
    type Edit,
    reconfigure,
} from "./models/choice.ts";
export {
    addEndpoint,
    assumedRate,
    atEndpoint,
    endpointName,
    isLocal,
} from "./models/endpoints.ts";
export {
    FAUX_MODEL,
    readScript,
    Script,
    scriptedModels,
} from "./models/faux.ts";
export { login, providersOf } from "./models/logins.ts";
export { plainly } from "./models/plainly.ts";
export {
    compatible,
    DEFAULT_WINDOW,
    protectedProvider,
} from "./models/providers.ts";
export { FileModelsStore } from "./models/store.ts";

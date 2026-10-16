import type { Context } from "@earendil-works/chord";
import {
    type AuthContext,
    type AuthInteraction,
    type Credential,
    createAssistantMessageEventStream,
    type OAuthAuth,
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
import { Value } from "typebox/value";
import { commitEdit } from "./chats/configure.ts";
import type { Home, HomeBase, Part } from "./home.ts";
import { LimitsDoc, limitsOf } from "./limits/rules.ts";
import {
    type Card,
    CardRecord,
    CatalogueDoc,
    type CatalogueFailure,
    catalogueOf,
    editCatalogue,
    type Price,
    PriceSchema,
    takeIn,
} from "./models/catalogue.ts";
import {
    checkConnection,
    checkModel,
    type ModelFailure,
} from "./models/check.ts";
import {
    type LoginFailure,
    login,
    providersOf,
    subscription,
} from "./models/logins.ts";
import { plainly } from "./models/plainly.ts";
import { compatible, protectedProvider } from "./models/providers.ts";
import {
    type SettingsEdit,
    type SettingsFailure,
    settingsFailureText,
    validateEndpointNames,
    validateSettings,
} from "./models/settings.ts";
import { FileModelsStore } from "./models/store.ts";
import type { ProviderView } from "./protocol.ts";
import { liveHamlets, RealmDoc } from "./realm/places.ts";
import { RecordDoc } from "./record.ts";
import { ok, type Result } from "./result.ts";
import { safeWords } from "./scrub.ts";
import {
    choose,
    modelRef,
    type Settings,
    SettingsDoc,
    settingsOf,
} from "./settings.ts";

const NOTHING_AMBIENT: AuthContext = {
    env: async () => undefined,
    fileExists: async () => false,
};

/** Refreshes retain last-known lists when the provider cannot answer. */
export type RefreshOutcome = {
    providers: readonly string[];
    retained: readonly string[];
    aborted: boolean;
};

/** All model links, provider state and sign-ins owned by one open home. */
export class ModelHome {
    /** pi-ai's models, shared with the home's harness. */
    readonly models: MutableModels;
    readonly #base: HomeBase;
    readonly #logins: Readonly<Record<string, OAuthAuth>>;
    readonly #owned: boolean;
    readonly #store: FileModelsStore;
    readonly #keySource: { provider: string | undefined };
    readonly #custom = new Map<string, string>();
    #cards: Record<string, Card> = {};
    #attached: Home | undefined;

    /** Injected models keep their own providers and never refresh the network at startup. */
    constructor(
        base: HomeBase,
        models?: MutableModels,
        logins: Readonly<Record<string, OAuthAuth>> = {},
    ) {
        this.#logins = logins;
        this.#base = base;
        this.#store = new FileModelsStore(base.layout.models);
        this.#keySource = {
            provider: modelRef(
                base.overrides.model ?? SettingsDoc.definition.initial().model,
            )?.provider,
        };
        this.#owned = models === undefined;
        this.models = models ?? this.#createModels();
    }

    #home(): Home {
        if (this.#attached === undefined)
            throw new Error("Models are not attached to their home.");
        return this.#attached;
    }
    async #readCards(context: Context): Promise<void> {
        this.#cards = {
            ...(await this.#home().harness.snapshot(CatalogueDoc, context))
                ?.models,
        };
    }
    #createModels(): MutableModels {
        const home = this.#base;
        const store = this.#store;
        const source = this.#keySource;
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
        for (const provider of builtinProviders()) {
            if (provider.id === "zai" || provider.id === "zai-coding-cn")
                continue;
            const oauth =
                subscription(provider.id) && provider.id !== "chatgpt"
                    ? (this.#logins.openrouter ?? provider.auth.oauth)
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
                    () => this.#cards,
                ),
            );
        }
        const openai: Provider = openaiProvider();
        const oauth = this.#logins.chatgpt ?? openai.auth.oauth;
        if (oauth !== undefined) {
            const alias = createProvider({
                id: "chatgpt",
                name: "ChatGPT",
                auth: {
                    oauth: { ...oauth, loginLabel: "Continue with ChatGPT" },
                },
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
                protectedProvider(home, alias, () => this.#cards),
            );
        }
        return models;
    }

    /** Installs valid endpoints without replacing an injected provider. */
    async sync(
        settings: Settings,
        context: Context,
    ): Promise<Result<void, SettingsFailure>> {
        const home = this.#home();
        const valid = validateEndpointNames(settings.endpoints);
        if (!valid.ok) return valid;
        if (!this.#owned) return ok(undefined);
        await this.#readCards(context);
        const previous = this.#custom;
        const serving = modelRef(settings.model);
        this.#keySource.provider = serving?.provider;
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
            const store = this.#store;
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
            const provider = compatible(endpoint, seeds);
            const stored = await store?.read(endpoint.name);
            const missing = provider
                .getModels()
                .filter(
                    (model) =>
                        !stored?.models.some((each) => each.id === model.id),
                );
            if (missing.length > 0)
                await store?.write(endpoint.name, {
                    ...stored,
                    models: [...(stored?.models ?? []), ...missing],
                });
            home.models.setProvider(
                protectedProvider(home, provider, () => this.#cards),
            );
            previous.set(endpoint.name, stamp);
        }
        for (const name of previous.keys()) {
            if (settings.endpoints.some((endpoint) => endpoint.name === name))
                continue;
            home.models.deleteProvider(name);
            previous.delete(name);
        }
        return ok(undefined);
    }

    /** Refreshes only this home's custom providers with a bounded wait. */
    async refresh(
        context: Context,
        options: { timeout?: number } = {},
    ): Promise<Result<RefreshOutcome, SettingsFailure>> {
        const home = this.#home();
        const synchronized = await this.sync(
            settingsOf(
                await home.harness.snapshot(SettingsDoc, context),
                home.overrides,
            ),
            context,
        );
        if (!synchronized.ok) return synchronized;
        const providers = [...this.#custom.keys()];
        if (providers.length === 0)
            return ok({ providers, retained: [], aborted: false });
        const result = await home.models.refresh({
            providers,
            force: true,
            signal: AbortSignal.timeout(options.timeout ?? 10_000),
        });
        for (const provider of providers) {
            const models = home.models.getModels(provider);
            const url = home.models.getProvider(provider)?.baseUrl ?? "";
            await takeIn(
                home,
                provider,
                url,
                models.map((model) => {
                    const listedPrice =
                        "listedPrice" in model ? model.listedPrice : undefined;
                    const listedContext =
                        "listedContext" in model
                            ? model.listedContext
                            : undefined;
                    return {
                        id: model.id,
                        context:
                            listedContext === null ||
                            typeof listedContext === "number"
                                ? listedContext
                                : model.contextWindow,
                        price: Value.Check(PriceSchema, listedPrice)
                            ? listedPrice
                            : null,
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
        await this.#readCards(context);
        return ok({
            providers,
            retained: [...result.errors.keys()],
            aborted: result.aborted,
        });
    }

    /** Saves valid settings atomically with every changed model choice. */
    async editSettings(
        set: SettingsEdit,
        context: Context,
    ): Promise<Result<void, SettingsFailure>> {
        const valid = validateSettings(set);
        if (!valid.ok) return valid;
        const home = this.#home();
        const before =
            (await home.harness.snapshot(SettingsDoc, context)) ??
            SettingsDoc.definition.initial();
        const next: Settings = { ...before, ...set, saved: true };
        const synced = await this.sync(
            settingsOf(next, home.overrides),
            context,
        );
        if (!synced.ok) return synced;
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
        return ok(undefined);
    }

    /** Changes only known model cards, retaining the chief's overrides across refreshes. */
    editCatalogue(
        name: string,
        set: { allowed?: boolean; price?: Price | null; note?: string },
        context: Context,
    ): Promise<Result<void, CatalogueFailure>> {
        return editCatalogue(this.#home(), name, set, context);
    }

    /** Lists last-known model cards in stable name order. */
    catalogue(context: Context): Promise<readonly Card[]> {
        return catalogueOf(this.#home(), context);
    }

    /** Offered sign-in methods never include ambient credentials. */
    providers(): Promise<ProviderView[]> {
        return providersOf(this.#home());
    }

    /** Returns only classified provider refusals; credentials stay in the home. */
    login(
        provider: string,
        method: "key" | "subscription",
        interaction: AuthInteraction,
    ): Promise<Result<Credential, LoginFailure>> {
        return login(this.#home(), provider, method, interaction);
    }

    /** A bounded hello with the connection outcome saved for every client. */
    checkModel(
        name: string | undefined,
        context: Context,
    ): Promise<Result<string, ModelFailure>> {
        return checkModel(this.#home(), name, context);
    }

    /** Refreshes the serving provider and records its connection outcome. */
    checkConnection(context: Context): Promise<Result<string, ModelFailure>> {
        return checkConnection(this.#home(), context);
    }

    /** Wires provider watches to this home's lifecycle. */
    readonly part: Part = (home) => {
        this.#attached = home;
        const stops: (() => Promise<unknown>)[] = [];
        return {
            extension: defineExtension({ name: "hamlet.models" }),
            start: async (context) => {
                const initial = await this.sync(
                    settingsOf(
                        await home.harness.snapshot(SettingsDoc, context),
                        home.overrides,
                    ),
                    context,
                );
                if (!initial.ok) {
                    console.error(settingsFailureText(initial.error));
                    return;
                }
                await home.models.refresh({ allowNetwork: false });
                const refreshed = await this.refresh(context, {
                    timeout: 1500,
                });
                if (!refreshed.ok)
                    console.error(settingsFailureText(refreshed.error));
                if (!this.#owned) return;
                const settings = await home.harness.watchDoc(
                    SettingsDoc,
                    context,
                );
                settings?.start(async (value, _ops, context) => {
                    const result = await this.sync(
                        settingsOf(value ?? undefined, home.overrides),
                        context,
                    );
                    if (!result.ok)
                        console.error(settingsFailureText(result.error));
                });
                if (settings !== undefined) stops.push(() => settings.stop());
                const unwatch = home.harness.subscribeCommits(({ changes }) => {
                    for (const change of changes) {
                        if (
                            change.type === "document" &&
                            change.record.kind ===
                                CatalogueDoc.definition.kind &&
                            change.value !== null
                        ) {
                            if (!Value.Check(CardRecord, change.value.models))
                                throw new Error(
                                    "Committed catalogue cards are invalid.",
                                );
                            this.#cards = change.value.models;
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

export {
    type Card,
    CatalogueDoc,
    type CatalogueFailure,
    catalogueFailureText,
    type Listed,
    type Price,
    takeIn,
} from "./models/catalogue.ts";
export { type ModelFailure, modelFailureText } from "./models/check.ts";
export {
    addEndpoint,
    assumedRate,
    atEndpoint,
    endpointName,
    isLocal,
} from "./models/endpoints.ts";
export { type ProviderFailure, providerFailureText } from "./models/failure.ts";
export {
    FAUX_MODEL,
    readScript,
    Script,
    scriptedModels,
} from "./models/faux.ts";
export { type LoginFailure, loginFailureText } from "./models/logins.ts";
export { plainly } from "./models/plainly.ts";
export {
    compatible,
    DEFAULT_WINDOW,
    protectedProvider,
} from "./models/providers.ts";
export {
    type SettingsFailure,
    settingsFailureText,
} from "./models/settings.ts";
export { modelView } from "./models/state.ts";
export { FileModelsStore } from "./models/store.ts";

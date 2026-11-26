/**
 * The agent's model access: the model its copy names, each call made with
 * the realm's credential for it and scrubbed, and the price each call is
 * charged at.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    type Api,
    type ApiKeyAuth,
    type AssistantMessage,
    type AssistantMessageEvent,
    createModels,
    type Model,
    type Models,
    type ModelsSimpleStreamOptions,
} from "@earendil-works/pi-ai";
import { raceWithAbortSignal } from "@earendil-works/pi-ai/utils/abort";
import { AssistantMessageEventStream } from "@earendil-works/pi-ai/utils/event-stream";
import {
    type AgentChange,
    AgentDoc,
    type ConversationId,
    type Cursor,
    configure,
    type Harness,
    type ModelRef,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentContext, generationOf } from "./actor.ts";
import type { ReadCopies } from "./copies.ts";
import {
    FAUX,
    type Given,
    type Keys,
    keysOf,
    scrub,
    scrubbedError,
    secretsLoaded,
    withheld,
} from "./credentials.ts";
import {
    type Checked,
    EFFORTS,
    type Effort,
    type ListedImages,
    type ListedModel,
    type Price,
    type ProviderView,
} from "./messages/agent.ts";
import { U32_MAX } from "./messages/shared.ts";
import type { ImageLimits } from "./messages/workspace.ts";
import { isEndpoint } from "./providers.ts";
import type { BareRealm, Peers } from "./turn.ts";

/** Dollars per million tokens of a model nothing prices. */
const RATE = 1;
/** {@link RATE} for a model served on this machine. */
const LOCAL_RATE = 0.1;

/** The APIs whose requests carry a `service_tier`. */
const FAST_APIS: readonly Api[] = [
    "openai-responses",
    "openai-codex-responses",
];

/**
 * `listed` with each request made with the realm's key, priced in today's
 * order, and in the priority tier when the copy asks for it.
 */
export function agentModels(
    listed: Models,
    copies: ReadCopies,
    peers: Peers,
): Models {
    const keys = agentKeys(listed, peers);
    const served = async (
        model: Model<Api>,
        options: ModelsSimpleStreamOptions | undefined,
        bill: Bill,
    ): Promise<[Model<Api>, ModelsSimpleStreamOptions]> => {
        const signal = options?.signal;
        // A stop does not wait for a refresh or the first load, which go on.
        const stoppable = <T>(waited: Promise<T>): Promise<T> =>
            signal === undefined ? waited : raceWithAbortSignal(waited, signal);
        const [{ model: choice }, given] = await Promise.all([
            copies(),
            stoppable(keys.of(model.provider)),
            stoppable(secretsLoaded(peers.reach)),
        ]);
        const chosen = choice !== undefined && nameOf(model) === choice.model;
        const previous = options?.onProviderStreamEvent;
        // The caller's headers win over the login's, as pi-ai's `applyAuth`.
        const { headers, ...key } = given;
        const made: ModelsSimpleStreamOptions = {
            ...options,
            ...key,
            ...(headers === undefined
                ? {}
                : { headers: { ...headers, ...options?.headers } }),
            onProviderStreamEvent: async (event, called) => {
                const billed = billedCost(event);
                if (billed !== undefined) bill.billed = billed;
                await previous?.(event, called);
            },
        };
        // As today's harness gives the model the images its card says it
        // takes (`harnesses/pi/src/models.ts`, `selectedModel`).
        const carded =
            chosen &&
            choice.images !== undefined &&
            !model.input.includes("image");
        return [
            {
                ...model,
                ...(carded
                    ? { input: [...model.input, "image" as const] }
                    : {}),
                cost: costOf(model, chosen ? choice.price : undefined),
            },
            chosen && choice.fast === true ? fastened(made, model.api) : made,
        ];
    };
    const streamSimple: Models["streamSimple"] = (
        model,
        transcript,
        options,
    ) => {
        const bill: Bill = {};
        return new Scrubbed(
            served(model, options, bill).then(([made, asked]) =>
                takingKeys(listed, made.provider, asked).streamSimple(
                    made,
                    transcript,
                    asked,
                ),
            ),
            bill,
        );
    };
    const completeSimple: Models["completeSimple"] = async (
        model,
        transcript,
        options,
    ) => {
        try {
            const bill: Bill = {};
            const [made, asked] = await served(model, options, bill);
            return billedIn(
                scrub(
                    await takingKeys(
                        listed,
                        made.provider,
                        asked,
                    ).completeSimple(made, transcript, asked),
                ),
                bill,
            );
        } catch (error) {
            throw scrubbedError(error);
        }
    };
    return new Proxy(listed, {
        get(target, name, receiver) {
            if (name === "streamSimple") return streamSimple;
            if (name === "completeSimple") return completeSimple;
            const value: unknown = Reflect.get(target, name, receiver);
            return typeof value === "function" ? value.bind(target) : value;
        },
    });
}

/**
 * The listing a request to `provider` goes to. pi-ai takes a request's key
 * only for a provider with key auth (`auth/resolve.js`): one that signs in
 * only by OAuth is served by a listing of its own, where the login's access
 * token is that key. A key of settings alone is served by a listing of its
 * own too, which reads nothing of the host's environment or files, as
 * today's harness serves a request, so only the realm's settings sign it in.
 */
function takingKeys(
    listed: Models,
    provider: string,
    given: Given | undefined,
): Models {
    const served = listed.getProvider(provider);
    if (served === undefined) return listed;
    const signsInByOauth = served.auth.apiKey === undefined;
    const settingsAlone =
        given?.apiKey === undefined && given?.env !== undefined;
    if (!signsInByOauth && !settingsAlone) return listed;
    const models = createModels({ authContext: NOTHING_OF_THE_HOST });
    models.setProvider(
        signsInByOauth ? { ...served, auth: { apiKey: GIVEN_KEY } } : served,
    );
    return models;
}

const NOTHING_OF_THE_HOST = {
    env: async () => undefined,
    fileExists: async () => false,
};

const GIVEN_KEY: ApiKeyAuth = {
    name: "the realm's sign-in",
    resolve: async ({ credential }) =>
        credential?.key === undefined
            ? undefined
            : { auth: { apiKey: credential.key } },
};

/**
 * The source's events with the keys scrubbed. A source that throws throws
 * here too, as the spend gate expects of a broken stream. A text still
 * streaming is shown without a tail that begins a key, since pi-durable
 * keeps each partial and a kill keeps the last.
 */
class Scrubbed extends AssistantMessageEventStream {
    readonly #source: Promise<AssistantMessageEventStream>;
    readonly #bill: Bill;

    constructor(source: Promise<AssistantMessageEventStream>, bill: Bill) {
        super();
        // Its consumer may first wait on the realm, and a rejection seen
        // later is still a rejection there.
        source.catch(() => {});
        this.#source = source;
        this.#bill = bill;
    }

    override async *[Symbol.asyncIterator](): AsyncGenerator<AssistantMessageEvent> {
        const shown = new Map<number, string>();
        const received = new Map<number, string>();
        try {
            for await (const event of await this.#source)
                yield this.#withheld(event, shown, received);
        } catch (error) {
            throw scrubbedError(error);
        }
    }

    override async result(): Promise<AssistantMessage> {
        try {
            const message = billedIn(
                scrub(await (await this.#source).result()),
                this.#bill,
            );
            if (isCutOff(message)) withheldIn(message);
            return message;
        } catch (error) {
            throw scrubbedError(error);
        }
    }

    /**
     * `source` scrubbed, each text still streaming withheld. `shown` holds each
     * block's text as last shown, `received` each call's arguments so far.
     */
    #withheld(
        source: AssistantMessageEvent,
        shown: Map<number, string>,
        received: Map<number, string>,
    ): AssistantMessageEvent {
        const event = scrub(source);
        if (event.type === "done") billedIn(event.message, this.#bill);
        if (event.type === "error")
            withheldIn(billedIn(event.error, this.#bill));
        if (event.type === "done" && isCutOff(event.message)) {
            withheldIn(event.message);
        }
        if (!("partial" in event)) return event;
        withheldIn(
            event.partial,
            event.type === "text_end" || event.type === "thinking_end"
                ? event.contentIndex
                : undefined,
        );
        let text: string | undefined;
        if (event.type === "text_delta" || event.type === "thinking_delta") {
            const block = event.partial.content[event.contentIndex];
            text =
                block?.type === "text"
                    ? block.text
                    : block?.type === "thinking"
                      ? block.thinking
                      : undefined;
        } else if (source.type === "toolcall_delta") {
            // pi-durable keeps a call's arguments from its deltas alone.
            const sofar =
                (received.get(source.contentIndex) ?? "") + source.delta;
            received.set(source.contentIndex, sofar);
            text = withheld(scrub(sofar));
        }
        // pi-durable appends each delta to the text it kept.
        if (text !== undefined && "delta" in event) {
            const before = shown.get(event.contentIndex) ?? "";
            event.delta = text.slice(before.length);
            shown.set(event.contentIndex, text);
        }
        return event;
    }
}

/** What an endpoint says it billed a call, which stands over pi-ai's own price, as today's harness takes it. */
interface Bill {
    billed?: number;
}

/** `message` at the cost its endpoint billed, where it billed one. */
function billedIn(message: AssistantMessage, bill: Bill): AssistantMessage {
    if (bill.billed !== undefined) message.usage.cost.total = bill.billed;
    return message;
}

/** The cost a provider's raw event reports in its usage, as OpenRouter-style endpoints bill (`harnesses/pi/src/models.ts`). */
function billedCost(event: unknown): number | undefined {
    if (typeof event !== "object" || event === null || !("usage" in event))
        return undefined;
    const { usage } = event;
    if (typeof usage !== "object" || usage === null || !("cost" in usage))
        return undefined;
    const { cost } = usage;
    return typeof cost === "number" && Number.isFinite(cost) ? cost : undefined;
}

/** An answer whose text may end in part of a key. */
function isCutOff(message: AssistantMessage): boolean {
    return (
        message.stopReason === "error" ||
        message.stopReason === "aborted" ||
        message.stopReason === "length"
    );
}

function withheldIn(message: AssistantMessage, ended?: number): void {
    for (const [index, block] of message.content.entries()) {
        if (index === ended) continue;
        if (block.type === "text") block.text = withheld(block.text);
        else if (block.type === "thinking") {
            block.thinking = withheld(block.thinking);
        } else {
            block.arguments = JSON.parse(
                JSON.stringify(block.arguments, (_key, value: unknown) =>
                    typeof value === "string" ? withheld(value) : value,
                ),
            );
        }
    }
}

function nameOf(model: Pick<Model<Api>, "provider" | "id">): string {
    return `${model.provider}::${model.id}`;
}

/**
 * Today's order (`crates/hamlet_server/src/providers.rs`, `select`): the
 * catalogue's price, which the realm puts in the copy, then the listing's
 * cost, then a flat rate.
 */
function costOf(
    model: Model<Api>,
    price: Price | undefined,
): Model<Api>["cost"] {
    if (price !== undefined) {
        return {
            input: price.input,
            output: price.output,
            cacheRead: price.cache_read,
            cacheWrite: price.cache_write,
        };
    }
    const cost = listedCost(model);
    if (cost !== undefined) return cost;
    const rate = isLocal(model.baseUrl) ? LOCAL_RATE : RATE;
    return { input: rate, output: rate, cacheRead: rate, cacheWrite: rate };
}

/**
 * The listing's cost when it gives all four rates, as today's server reads
 * it (`providers.rs`, `select`). A listing may leave some out, whatever
 * pi-ai's type says.
 */
function listedCost(model: Model<Api>): Model<Api>["cost"] | undefined {
    const cost: Partial<Model<Api>["cost"]> | undefined = model.cost;
    const { input, output, cacheRead, cacheWrite } = cost ?? {};
    return typeof input === "number" &&
        typeof output === "number" &&
        typeof cacheRead === "number" &&
        typeof cacheWrite === "number"
        ? { input, output, cacheRead, cacheWrite }
        : undefined;
}

function isLocal(baseUrl: string | undefined): boolean {
    if (baseUrl === undefined || !URL.canParse(baseUrl)) return false;
    const host = new URL(baseUrl).hostname;
    return (
        host === "localhost" ||
        host === "[::1]" ||
        host === "0.0.0.0" ||
        host.startsWith("127.")
    );
}

/** As today's `withFast`: the provider's priority tier, where its API has one. */
function fastened(
    options: ModelsSimpleStreamOptions,
    api: Api,
): ModelsSimpleStreamOptions {
    if (!FAST_APIS.includes(api)) return options;
    const previous = options.onPayload;
    return {
        ...options,
        onPayload: async (payload, model) => {
            const own = await previous?.(payload, model);
            const body = own ?? payload;
            return typeof body === "object" && body !== null
                ? { ...body, service_tier: "priority" }
                : own;
        },
    };
}

/**
 * The models of `providers`, or of every provider when it is absent, each
 * with what today's catalogue learns of its listing (`providers.rs`,
 * `Card::learn`).
 */
export function modelList(
    listed: Models,
    providers?: ReadonlySet<string>,
): ListedModel[] {
    const models = listed
        .getModels()
        .filter((model) => providers?.has(model.provider) ?? true);
    return models.map((model) => {
        const cost = listedCost(model);
        const images = listedImages(model);
        return {
            name: nameOf(model),
            title: model.name,
            context: model.contextWindow,
            ...(images === undefined ? {} : { images }),
            efforts: effortsOf(model),
            ...(FAST_APIS.includes(model.api) ? { fast: true } : {}),
            ...(cost === undefined
                ? {}
                : {
                      price: {
                          input: cost.input,
                          output: cost.output,
                          cache_read: cost.cacheRead,
                          cache_write: cost.cacheWrite,
                      },
                  }),
        };
    });
}

/**
 * The efforts `model` can be asked for: every one its thinking map does not
 * leave out, the two highest only where it names them, or none but off for a
 * model that does not reason.
 */
function effortsOf(model: Model<Api>): Effort[] {
    if (!model.reasoning) return ["off"];
    const map: Partial<Record<string, unknown>> = model.thinkingLevelMap ?? {};
    return EFFORTS.filter((effort) =>
        effort in map
            ? map[effort] !== null
            : effort !== "xhigh" && effort !== "max",
    );
}

/**
 * The providers whose models a chief can choose, as today's catalogue
 * shows them: those the realm holds a sign-in for, and the endpoints.
 */
export async function reached(
    listed: Models,
    realm: Pick<BareRealm, "credential">,
): Promise<Set<string>> {
    const views = await providerViews(listed, realm);
    return new Set(
        views.flatMap(({ id, configured }) => {
            const provider = listed.getProvider(id);
            return configured ||
                (provider !== undefined && isEndpoint(provider))
                ? [id]
                : [];
        }),
    );
}

/**
 * Asks the model `name` for a hello, as today's check does. It goes to the
 * listing itself, so it is neither admitted to a conversation nor charged,
 * and a repeat asks again.
 */
export async function checkModel(
    listed: Models,
    keys: Keys,
    name: string,
): Promise<Checked> {
    const ref = named(listed, name);
    if ("type" in ref) return ref;
    const { provider, modelId } = ref;
    const model = listed.getModel(provider, modelId);
    if (model === undefined) {
        return { type: "refused", words: `no model is named \`${name}\`` };
    }
    try {
        const given = await keys.of(provider);
        const answer = await takingKeys(listed, provider, given).completeSimple(
            model,
            {
                messages: [
                    {
                        role: "user",
                        content: "Say hello in one word.",
                        timestamp: Date.now(),
                    },
                ],
            },
            {
                signal: AbortSignal.timeout(90_000),
                maxRetries: 0,
                ...given,
            },
        );
        const text = answer.content
            .flatMap((part) => (part.type === "text" ? [part.text] : []))
            .join("")
            .trim();
        if (
            answer.stopReason === "error" ||
            answer.stopReason === "aborted" ||
            text === ""
        ) {
            throw new Error(
                answer.errorMessage ?? "the model answered with nothing",
            );
        }
        return { type: "answered", words: scrub(text) };
    } catch (error) {
        // Scrubbed before `plainly` cuts it, so no part of a key is left.
        return { type: "failed", words: plainly(scrub(String(error)), true) };
    }
}

/**
 * Asks the provider of the model `name` for its list again, and answers with
 * the count of the models it lists, as today's connection check does. Its
 * login is rotated first when due, as today's refresh resolves it first.
 */
export async function checkConnection(
    listed: Models,
    keys: Keys,
    name: string,
): Promise<Checked> {
    const ref = named(listed, name);
    if ("type" in ref) return ref;
    const { provider } = ref;
    let error: unknown;
    try {
        await keys.fresh(provider);
        const { errors } = await listed.refresh({
            providers: [provider],
            force: true,
            signal: AbortSignal.timeout(90_000),
        });
        error = errors.get(provider);
    } catch (failed) {
        error = failed;
    }
    if (error !== undefined) {
        return {
            type: "failed",
            words: plainly(scrub(String(error)), false),
        };
    }
    const count = listed.getModels(provider).length;
    return {
        type: "answered",
        words: `Connected, ${count} model(s)`,
        models: count,
    };
}

/** The provider and model `name` names, or its refusal, as today's `named_model`. */
function named(listed: Models, name: string): ModelRef | Checked {
    const ref = refOf(name);
    if (ref === undefined) {
        return {
            type: "refused",
            words: `\`${name}\` names no endpoint. A model is named \`endpoint::model\`.`,
        };
    }
    if (listed.getProvider(ref.provider) === undefined) {
        return {
            type: "refused",
            words: `\`${name}\` is served by the endpoint \`${ref.provider}\`, which the settings do not have.`,
        };
    }
    return ref;
}

/** Whether the model `name` is served by a provider a chief signs in to. */
export function signedInTo(listed: Models, name: string): boolean {
    const auth = listed.getProvider(refOf(name)?.provider ?? "")?.auth;
    return auth?.apiKey?.login !== undefined || auth?.oauth !== undefined;
}

async function runningRef(copies: ReadCopies, host: string): Promise<ModelRef> {
    return (await copiedAgent(copies))?.model ?? hostRef(host);
}

function hostRef(host: string): ModelRef {
    const at = host.indexOf("/");
    return { provider: host.slice(0, at), modelId: host.slice(at + 1) };
}

/** The model the agent runs, as `provider::model`. */
export async function runningModel(
    copies: ReadCopies,
    host: string,
): Promise<string> {
    const { provider, modelId } = await runningRef(copies, host);
    return nameOf({ provider, id: modelId });
}

/** `host`, given as `provider/model`, as `provider::model`. */
export function hostModel(host: string): string {
    const { provider, modelId } = hostRef(host);
    return nameOf({ provider, id: modelId });
}

/** As `ImageInput`'s defaults in `crates/hamlet_protocol/src/client/images.rs`. */
const IMAGE_WIDTH = 2000;
const IMAGE_HEIGHT = 2000;
const IMAGE_BYTES = 4_718_592;
const IMAGES_PER_LINE = 20;

/** The images a model takes, and the most of them one line gives it. */
export interface ImagesTaken extends ImageLimits {
    readonly perLine: number;
}

/**
 * The images `model` takes, as `listed_images` in
 * `crates/hamlet_server/src/providers.rs` reads its listing; a faux model
 * takes them with no limit named, as `Providers::images` there gives.
 * `undefined` when it takes none.
 */
function listedImages(model: Model<Api>): ListedImages | undefined {
    if (model.provider !== FAUX && !model.input.includes("image")) {
        return undefined;
    }
    const images = model.inputLimits?.images;
    const named = (key: keyof ListedImages, value: number | undefined) =>
        value !== undefined && value > 0 && value <= U32_MAX
            ? { [key]: Math.floor(value) }
            : {};
    return {
        ...named("max_bytes", images?.resize?.maxBytes),
        ...named("max_height", images?.resize?.maxHeight),
        ...named("max_width", images?.resize?.maxWidth),
        ...named("per_line", images?.maxPerMessage),
    };
}

/**
 * The images a model whose card or listing says `listed` takes, as
 * `Limits::of` in `crates/hamlet_server/src/images.rs` fills in what it
 * leaves out and caps the most of them in one line. `undefined` when it
 * takes none.
 */
function imagesOf(listed: ListedImages | undefined): ImagesTaken | undefined {
    if (listed === undefined) return undefined;
    return {
        width: listed.max_width ?? IMAGE_WIDTH,
        height: listed.max_height ?? IMAGE_HEIGHT,
        bytes: listed.max_bytes ?? IMAGE_BYTES,
        perLine: Math.min(listed.per_line ?? IMAGES_PER_LINE, IMAGES_PER_LINE),
    };
}

/**
 * The images the model the agent runs takes: as its catalogue card says,
 * else as its listing does, and a faux model's as its listing does
 * (`crates/hamlet_server/src/providers.rs`, `Providers::images`).
 * `undefined` when it takes none.
 */
export async function runningImages(
    listed: Models,
    copies: ReadCopies,
    host: string,
): Promise<ImagesTaken | undefined> {
    const { model: choice } = await copies();
    const { provider, modelId } = await runningRef(copies, host);
    const model = listed.getModel(provider, modelId);
    if (model === undefined) return undefined;
    const card =
        model.provider !== FAUX && choice?.model === nameOf(model)
            ? choice.images
            : undefined;
    return imagesOf(card ?? listedImages(model));
}

/** The providers a subscription signs in to, as today's (ADR 0006). */
const SUBSCRIPTIONS = ["chatgpt", "openrouter", "claude"];

/** The words of a provider's sign-in button where they are not its name. */
const LABELS: Readonly<Record<string, string>> = {
    chatgpt: "Continue with ChatGPT",
    openrouter: "Sign in with OpenRouter",
    claude: "Sign in to Claude",
};

/**
 * The providers a chief may sign in to, and whether each is, in today's
 * order and words (`crates/hamlet_server/src/providers.rs`,
 * `provider_views`).
 */
export async function providerViews(
    listed: Models,
    realm: Pick<BareRealm, "credential">,
): Promise<ProviderView[]> {
    const providers = [...listed.getProviders()].sort((a, b) =>
        a.id < b.id ? -1 : a.id > b.id ? 1 : 0,
    );
    return Promise.all(
        providers.map(async (provider): Promise<ProviderView> => {
            const kept = await realm.credential({ provider: provider.id });
            return {
                configured: kept.type !== "refused",
                id: provider.id,
                label: LABELS[provider.id] ?? provider.name,
                methods: [
                    ...(provider.auth.apiKey?.login === undefined
                        ? []
                        : ["key" as const]),
                    ...(SUBSCRIPTIONS.includes(provider.id) &&
                    provider.auth.oauth !== undefined
                        ? ["subscription" as const]
                        : []),
                ],
                name: provider.name,
            };
        }),
    );
}

/** A failed hello or list in words a chief can act on, as today's `plainly` gives them. */
function plainly(error: string, hello: boolean): string {
    const lower = error.toLowerCase();
    const says = (...words: string[]) =>
        words.some((word) => lower.includes(word));
    if (says("http 401", "http 403", "error: 401", "error: 403", "api key")) {
        return "That key was not accepted. Check it was copied whole and is still active.";
    }
    if (says("http 402", "error: 402", "credit", "quota", "billing")) {
        return "Your provider account has no credit left. Add some, then try again.";
    }
    if (says("could not reach")) {
        return "Could not reach it. Check your connection and the address.";
    }
    if (says("in time")) {
        return "It took too long to answer. Try again, or pick a smaller model.";
    }
    const detail = [...error].slice(0, 160).join("");
    if (hello) {
        return `That model did not answer a hello. Pick another one. (${detail})`;
    }
    if (says("http 404")) {
        return "That address did not answer as a model provider. It usually ends in /v1.";
    }
    return `It answered with an error. Try again. (${detail})`;
}

export function agentKeys(listed: Models, peers: Peers): Keys {
    return keysOf(peers.reach, peers.report, {
        listed,
        keep: peers.reach.keep_credential,
    });
}

/**
 * Moves every conversation of the agent to the model its copy names, each
 * in its own commit, and leaves alone one already on it, so a repeat after a
 * kill does only what is left.
 */
export async function configured(
    c: AgentContext,
    copies: ReadCopies,
): Promise<void> {
    const wanted = await copiedAgent(copies);
    if (wanted === undefined) return;
    await withHarness(c, generationOf(c).options, async (harness, context) => {
        // The root is made on the copy's model when it is not there yet, so
        // the chief's first session starts on it.
        await harness.root(context, { agent: wanted });
        for (const id of await conversationsOf(harness, context)) {
            await onCopiedModel(harness, context, id, copies);
        }
    });
}

/**
 * Puts the conversation `id` on the model and effort the copy names. The
 * copy is read again after each write, so an older copy written late is
 * replaced by the newer one.
 */
export async function onCopiedModel(
    harness: Harness,
    context: Context,
    id: ConversationId,
    copies: ReadCopies,
): Promise<void> {
    let wanted = await copiedAgent(copies);
    while (wanted !== undefined) {
        const target = wanted;
        await harness.commit(async (tx) => {
            const agent = await tx.doc(AgentDoc, id);
            if (!isOn(agent, target)) await configure(tx, id, target);
        }, context);
        const now = await copiedAgent(copies);
        wanted = now === undefined || isOn(now, target) ? undefined : now;
    }
}

function isOn(
    agent: Pick<AgentChange, "model" | "thinkingLevel">,
    wanted: CopiedAgent,
): boolean {
    return (
        agent.model?.provider === wanted.model.provider &&
        agent.model.modelId === wanted.model.modelId &&
        (agent.thinkingLevel ?? null) === wanted.thinkingLevel
    );
}

/**
 * What puts a conversation on the copy's model and effort, as
 * `createConversation`'s `agent` takes it.
 */
interface CopiedAgent {
    readonly model: ModelRef;
    readonly thinkingLevel: Effort | null;
}

async function copiedAgent(
    copies: ReadCopies,
): Promise<CopiedAgent | undefined> {
    const { model: choice } = await copies();
    const model = choice === undefined ? undefined : refOf(choice.model);
    if (choice === undefined || model === undefined) return undefined;
    return {
        model,
        thinkingLevel: choice.effort ?? null,
    } satisfies AgentChange;
}

/** `provider::model` as pi-durable names it, or `undefined` when either half is empty. */
function refOf(name: string): ModelRef | undefined {
    const at = name.indexOf("::");
    const modelId = name.slice(at + 2);
    if (at <= 0 || modelId === "") return undefined;
    return { provider: name.slice(0, at), modelId };
}

async function conversationsOf(
    harness: Harness,
    context: Context,
): Promise<ConversationId[]> {
    const ids: ConversationId[] = [];
    let cursor: Cursor | undefined;
    do {
        const page = await harness.commit(
            (tx) => tx.scanConversations({}, 100, cursor),
            context,
        );
        ids.push(...page.items.map((conversation) => conversation.id));
        cursor = page.next;
    } while (cursor !== undefined);
    return ids;
}

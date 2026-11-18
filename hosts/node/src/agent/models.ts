/**
 * The agent's model access: the model its copy names, the credential the
 * realm gives for it, held in memory only, and the price each call is
 * charged at. Also the host's scrub list.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    type Api,
    type ApiKeyAuth,
    type AssistantMessage,
    type AssistantMessageEvent,
    type Credential,
    createModels,
    type Model,
    type Models,
    type ModelsSimpleStreamOptions,
    type OAuthAuth,
    type OAuthCredential,
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
import { PROVIDER as CLAUDE } from "./claude.ts";
import type { ReadCopies } from "./copies.ts";
import {
    type Checked,
    type CredentialAnswer,
    type Effort,
    type Kept,
    type ListedModel,
    type OauthLogin,
    type Price,
    u32,
} from "./messages.ts";
import { type BareRealm, type Peers, reasonOf } from "./turn.ts";

/** Dollars per million tokens of a model nothing prices. */
const RATE = 1;
/** {@link RATE} for a model served on this machine. */
const LOCAL_RATE = 0.1;

/** The provider of the faux models, which take no credential. */
const FAUX = "faux";

/** The APIs whose requests carry a `service_tier`. */
const FAST_APIS: readonly Api[] = [
    "openai-responses",
    "openai-codex-responses",
];

/**
 * Every key, token and typed sign-in answer this host was given, and every
 * value of the realm's secret list, retired ones included, as today's server
 * keeps them for its scrub
 * (`crates/hamlet_server/src/credentials.rs`, `remember`). They live only
 * here, for the host's life.
 */
const SECRETS = new Set<string>();

export function remember(...secrets: readonly string[]): void {
    for (const secret of secrets) if (secret !== "") SECRETS.add(secret);
}

/** The host's first load of the realm's secret list. */
let listLoaded: Promise<void> | undefined;

/**
 * The realm's whole secret list taken into the scrub, once in the host's
 * life; every read a client or a model makes waits for it. A load that
 * fails is tried again by the next read, so nothing leaves unscrubbed.
 */
export function secretsLoaded(
    realm: Pick<BareRealm, "secrets">,
): Promise<void> {
    listLoaded ??= realm.secrets().then(
        ({ values }) => remember(...values),
        (error: unknown) => {
            listLoaded = undefined;
            throw error;
        },
    );
    return listLoaded;
}

/** Takes the realm's secret list again after its first load. */
export async function secretsRefreshed(
    realm: Pick<BareRealm, "secrets">,
): Promise<void> {
    const first = listLoaded;
    await secretsLoaded(realm);
    if (first !== undefined) remember(...(await realm.secrets()).values);
}

/** Remembers the secrets of `kept`, as today's `remember_model` does. */
export function rememberKept(kept: Kept): void {
    if (kept.type === "key") remember(kept.key);
    else remember(kept.login.access, kept.login.refresh);
}

/**
 * `value` with every secret the host holds, and each of `also`, replaced,
 * longest first.
 */
export function scrub<T>(value: T, also: readonly string[] = []): T {
    const secrets = [...SECRETS, ...also.filter((word) => word !== "")].sort(
        (a, b) => b.length - a.length,
    );
    if (secrets.length === 0) return value;
    const clean = (text: string) =>
        secrets.reduce(
            (cleaned, secret) => cleaned.split(secret).join("[REDACTED]"),
            text,
        );
    return JSON.parse(
        JSON.stringify(value, (_key, item: unknown) => {
            if (typeof item === "string") return clean(item);
            if (
                typeof item === "object" &&
                item !== null &&
                !Array.isArray(item)
            ) {
                return Object.fromEntries(
                    Object.entries(item).map(([key, inner]) => [
                        clean(key),
                        inner,
                    ]),
                );
            }
            return item;
        }),
    ) as T;
}

/** `text` without a tail that begins a secret the host holds. */
export function withheld(text: string): string {
    let cut = 0;
    for (const secret of SECRETS) {
        for (let length = secret.length - 1; length > cut; length -= 1) {
            if (text.endsWith(secret.slice(0, length))) cut = length;
        }
    }
    return text.slice(0, text.length - cut);
}

/** The credential the realm gave this wake, per provider. */
export interface Keys {
    /**
     * The key a request to `provider` carries, `undefined` for the faux
     * models and for `claude`. Without one it throws, so no key from the
     * host's environment is used.
     */
    of(provider: string): Promise<string | undefined>;
}

/**
 * What an OAuth sign-in needs to serve requests: the listing's sign-in for
 * its provider, and the realm's keep for a rotated login. Without it, only
 * keys are used.
 */
export interface Subscriptions {
    readonly listed: Pick<Models, "getProvider">;
    readonly keep: BareRealm["keep_credential"];
}

/**
 * As pi-ai's `Models.getAuth` (`auth/resolve.js`): a login with less left
 * than this is refreshed before a request.
 */
const REFRESH_WITHIN_MS = 5 * 60_000;

export function keysOf(
    realm: Pick<BareRealm, "credential">,
    report: Peers["report"],
    subscriptions?: Subscriptions,
): Keys {
    const asked = new Map<string, Promise<Kept>>();
    const ask = async (provider: string): Promise<Kept> => {
        let answer: CredentialAnswer;
        try {
            answer = await realm.credential({ provider });
        } catch (error) {
            const words = `the realm's credential for ${provider} was not read`;
            report(words, error);
            throw new Error(`${words}: ${reasonOf(error)}`);
        }
        if (answer.type === "refused") throw new Error(answer.words);
        rememberKept(answer);
        return answer;
    };
    const keyOf = async (provider: string, kept: Kept): Promise<string> => {
        if (kept.type === "key" && kept.key !== "") return kept.key;
        const oauth = subscriptions?.listed.getProvider(provider)?.auth.oauth;
        if (
            kept.type === "oauth" &&
            oauth !== undefined &&
            subscriptions !== undefined
        ) {
            let login = kept.login;
            if (login.expires * 1000 <= Date.now() + REFRESH_WITHIN_MS) {
                login = await rotated(provider, login, oauth, {
                    keep: subscriptions.keep,
                    report,
                });
                asked.set(provider, Promise.resolve({ type: "oauth", login }));
            }
            const { apiKey } = await oauth.toAuth(credentialOf(login));
            if (apiKey !== undefined) return apiKey;
        }
        throw new Error(
            `The sign-in for ${provider} gives no key, and only a key is used yet.`,
        );
    };
    return {
        of: async (provider) => {
            if (provider === FAUX) return undefined;
            let asking = asked.get(provider);
            if (asking === undefined) {
                asking = ask(provider);
                asked.set(provider, asking);
                // A refusal or a failed call is asked again at the next request.
                asking.catch(() => asked.delete(provider));
            }
            const kept = await asking;
            // The Claude program signs in itself; the realm's credential only marks that it did.
            if (provider === CLAUDE) return undefined;
            return keyOf(provider, kept);
        },
    };
}

/**
 * Each rotation by the login it retires, so one refresh runs per login in
 * the host and every agent still holding the old one takes the new.
 */
const ROTATIONS = new Map<string, Promise<OauthLogin>>();

/**
 * `login` refreshed, once in the host, and handed to the realm before it
 * is used, as today's server saves a rotation before acknowledging it
 * (`docs/realm.md`).
 */
function rotated(
    provider: string,
    login: OauthLogin,
    oauth: OAuthAuth,
    to: { keep: BareRealm["keep_credential"]; report: Peers["report"] },
): Promise<OauthLogin> {
    const retired = `${provider}\n${login.refresh}`;
    let rotation = ROTATIONS.get(retired);
    if (rotation === undefined) {
        rotation = (async () => {
            // Not the request's signal: a rotation started is finished.
            const fresh = loginOf(
                await oauth.refresh(
                    credentialOf(login),
                    new AbortController().signal,
                ),
            );
            const value: Kept = { type: "oauth", login: fresh };
            rememberKept(value);
            try {
                await to.keep({ provider, value });
            } catch (error) {
                // This host keeps using it; the realm still holds the old one.
                to.report(
                    `the rotated login for ${provider} was not kept`,
                    scrubbedError(error),
                );
            }
            return fresh;
        })();
        ROTATIONS.set(retired, rotation);
        rotation.catch(() => ROTATIONS.delete(retired));
    }
    return rotation;
}

/** pi-ai's credential as the realm keeps it: an OAuth expiry in whole seconds. */
export function keptOf(credential: Credential): Kept {
    if (credential.type === "api_key") {
        return { type: "key", key: credential.key ?? "" };
    }
    return { type: "oauth", login: loginOf(credential) };
}

function loginOf(credential: OAuthCredential): OauthLogin {
    const { type: _, access, refresh, expires, ...extra } = credential;
    return {
        access,
        refresh,
        expires: u32(Math.floor(expires / 1000)),
        ...(Object.keys(extra).length === 0
            ? {}
            : { extra: JSON.stringify(extra) }),
    };
}

function credentialOf(login: OauthLogin): OAuthCredential {
    return {
        ...(login.extra === undefined
            ? {}
            : (JSON.parse(login.extra) as Record<string, unknown>)),
        type: "oauth",
        access: login.access,
        refresh: login.refresh,
        expires: login.expires * 1000,
    };
}

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
    ): Promise<[Model<Api>, ModelsSimpleStreamOptions]> => {
        const key = keys.of(model.provider);
        const [{ model: choice }, given] = await Promise.all([
            copies(),
            // A stop does not wait for a refresh, which goes on.
            options?.signal === undefined
                ? key
                : raceWithAbortSignal(key, options.signal),
            secretsLoaded(peers.reach),
        ]);
        const chosen = choice !== undefined && nameOf(model) === choice.model;
        const made: ModelsSimpleStreamOptions = {
            ...options,
            ...(given === undefined ? {} : { apiKey: given }),
        };
        return [
            {
                ...model,
                cost: costOf(model, chosen ? choice.price : undefined),
            },
            chosen && choice.fast === true ? fastened(made, model.api) : made,
        ];
    };
    const streamSimple: Models["streamSimple"] = (model, transcript, options) =>
        new Scrubbed(
            served(model, options).then(([made, asked]) =>
                takingKeys(listed, made.provider).streamSimple(
                    made,
                    transcript,
                    asked,
                ),
            ),
        );
    const completeSimple: Models["completeSimple"] = async (
        model,
        transcript,
        options,
    ) => {
        try {
            const [made, asked] = await served(model, options);
            return scrub(
                await takingKeys(listed, made.provider).completeSimple(
                    made,
                    transcript,
                    asked,
                ),
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
 * pi-ai takes a request's key only for a provider with key auth
 * (`auth/resolve.js`). One that signs in only by OAuth is served by a
 * listing of its own, where the login's access token is that key.
 */
function takingKeys(listed: Models, provider: string): Models {
    const served = listed.getProvider(provider);
    if (served === undefined || served.auth.apiKey !== undefined) return listed;
    const models = createModels();
    models.setProvider({ ...served, auth: { apiKey: GIVEN_KEY } });
    return models;
}

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

    constructor(source: Promise<AssistantMessageEventStream>) {
        super();
        // Its consumer may first wait on the realm, and a rejection seen
        // later is still a rejection there.
        source.catch(() => {});
        this.#source = source;
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
            const message = scrub(await (await this.#source).result());
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
        if (event.type === "error") withheldIn(event.error);
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

/** An answer whose text may end in part of a key. */
function isCutOff(message: AssistantMessage): boolean {
    return message.stopReason === "error" || message.stopReason === "aborted";
}

function withheldIn(message: AssistantMessage, ended?: number): void {
    for (const [index, block] of message.content.entries()) {
        if (index === ended) continue;
        if (block.type === "text") block.text = withheld(block.text);
        else if (block.type === "thinking") {
            block.thinking = withheld(block.thinking);
        }
    }
}

/** The spend gate keeps a thrown error's words as the answer. */
function scrubbedError(error: unknown): Error {
    return new Error(scrub(reasonOf(error)));
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

export function modelList(listed: Models): ListedModel[] {
    return listed.getModels().map((model) => {
        const cost = listedCost(model);
        return {
            name: nameOf(model),
            title: model.name,
            context: model.contextWindow,
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
 * Asks the model `name` for a hello, as today's check does. It goes to the
 * listing itself, so it is neither admitted to a conversation nor charged,
 * and a repeat asks again.
 */
export async function checkModel(
    listed: Models,
    keys: Keys,
    name: string,
): Promise<Checked> {
    const ref = refOf(name);
    if (ref === undefined) {
        return {
            type: "refused",
            words: `\`${name}\` names no endpoint. A model is named \`endpoint::model\`.`,
        };
    }
    const { provider, modelId } = ref;
    if (listed.getProvider(provider) === undefined) {
        return {
            type: "refused",
            words: `\`${name}\` is served by the endpoint \`${provider}\`, which the settings do not have.`,
        };
    }
    const model = listed.getModel(provider, modelId);
    if (model === undefined) {
        return { type: "refused", words: `no model is named \`${name}\`` };
    }
    try {
        const key = await keys.of(provider);
        const answer = await takingKeys(listed, provider).completeSimple(
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
                ...(key === undefined ? {} : { apiKey: key }),
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
        return { type: "failed", words: plainly(scrub(String(error))) };
    }
}

/** A failed hello in words a chief can act on, as today's `plainly` gives them. */
function plainly(error: string): string {
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
    return `That model did not answer a hello. Pick another one. (${detail})`;
}

function agentKeys(listed: Models, peers: Peers): Keys {
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

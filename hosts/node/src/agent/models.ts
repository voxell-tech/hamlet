/**
 * The agent's model access: the model its copy names, the key the realm
 * gives for it, held in memory only, and the price each call is charged at.
 */
import type { Context } from "@earendil-works/chord/context";
import type {
    Api,
    AssistantMessage,
    AssistantMessageEvent,
    Model,
    Models,
    ModelsSimpleStreamOptions,
} from "@earendil-works/pi-ai";
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
import type { CredentialAnswer, Effort, Price } from "./messages.ts";
import { type BareRealm, type Peers, reasonOf } from "./turn.ts";

/** Dollars per million tokens of a model nothing prices, as today. */
const RATE = 1;
/** {@link RATE} for a model served on this machine. */
const LOCAL_RATE = 0.1;

/** The provider of the faux models, which take no credential, as today. */
const FAUX = "faux";

/** The APIs whose requests carry a `service_tier`. */
const FAST_APIS: readonly Api[] = [
    "openai-responses",
    "openai-codex-responses",
];

/**
 * The keys the realm gave this wake. They live only here, so they go when
 * the agent sleeps.
 */
export interface Keys {
    /**
     * The realm's key for `provider`, `undefined` for the faux models. With
     * no key it throws words for the chief, so no other key, such as one
     * in the host's environment, is ever used.
     */
    of(provider: string): Promise<string | undefined>;
    /** `value` with every key held replaced, as today's scrub does. */
    scrub<T>(value: T): T;
    /** `text` without a tail that begins a held key. */
    withheld(text: string): string;
}

export function keysOf(
    realm: Pick<BareRealm, "credential">,
    report: Peers["report"],
): Keys {
    const held = new Map<string, string>();
    const asked = new Map<string, Promise<string>>();
    const ask = async (provider: string): Promise<string> => {
        let answer: CredentialAnswer;
        try {
            answer = await realm.credential({ provider });
        } catch (error) {
            const words = `the realm's credential for ${provider} was not read`;
            report(words, error);
            throw new Error(`${words}: ${reasonOf(error)}`);
        }
        if (answer.type === "refused") throw new Error(answer.words);
        if (answer.type === "oauth" || answer.key === "") {
            throw new Error(
                `The sign-in for ${provider} gives no key, and only a key is used yet.`,
            );
        }
        held.set(provider, answer.key);
        return answer.key;
    };
    return {
        of: (provider) => {
            if (provider === FAUX) return Promise.resolve(undefined);
            let asking = asked.get(provider);
            if (asking === undefined) {
                asking = ask(provider);
                asked.set(provider, asking);
                // A refusal or a failed call is asked again at the next request.
                asking.catch(() => asked.delete(provider));
            }
            return asking;
        },
        scrub: (value) => scrubbed(value, [...held.values()]),
        withheld: (text) => {
            let cut = 0;
            for (const key of held.values()) {
                for (let length = key.length - 1; length > cut; length -= 1) {
                    if (text.endsWith(key.slice(0, length))) cut = length;
                }
            }
            return text.slice(0, text.length - cut);
        },
    };
}

function scrubbed<T>(value: T, secrets: readonly string[]): T {
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

/**
 * `listed` with each request made with the realm's key, priced in today's
 * order, and in the priority tier when the copy asks for it.
 */
export function agentModels(
    listed: Models,
    copies: ReadCopies,
    peers: Peers,
): Models {
    const keys = keysOf(peers.reach, peers.report);
    const served = async (
        model: Model<Api>,
        options: ModelsSimpleStreamOptions | undefined,
    ): Promise<[Model<Api>, ModelsSimpleStreamOptions]> => {
        const [{ model: choice }, key] = await Promise.all([
            copies(),
            keys.of(model.provider),
        ]);
        const chosen = choice !== undefined && nameOf(model) === choice.model;
        const made: ModelsSimpleStreamOptions = {
            ...options,
            ...(key === undefined ? {} : { apiKey: key }),
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
                listed.streamSimple(made, transcript, asked),
            ),
            keys,
        );
    const completeSimple: Models["completeSimple"] = async (
        model,
        transcript,
        options,
    ) => {
        try {
            const [made, asked] = await served(model, options);
            return keys.scrub(
                await listed.completeSimple(made, transcript, asked),
            );
        } catch (error) {
            throw scrubbedError(keys, error);
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
 * The source's events with the keys scrubbed. A source that throws throws
 * here too, as the spend gate expects of a broken stream.
 *
 * pi-durable keeps each partial as it streams, and after a kill it keeps the
 * last one as an aborted answer, so a text still growing is shown without a
 * tail that begins a key: the rest of the key may be in the next delta.
 */
class Scrubbed extends AssistantMessageEventStream {
    readonly #source: Promise<AssistantMessageEventStream>;
    readonly #keys: Keys;

    constructor(source: Promise<AssistantMessageEventStream>, keys: Keys) {
        super();
        // Its consumer may first wait on the realm, and a rejection seen
        // later is still a rejection there.
        source.catch(() => {});
        this.#source = source;
        this.#keys = keys;
    }

    override async *[Symbol.asyncIterator](): AsyncGenerator<AssistantMessageEvent> {
        const shown = new Map<number, string>();
        try {
            for await (const event of await this.#source)
                yield this.#withheld(this.#keys.scrub(event), shown);
        } catch (error) {
            throw scrubbedError(this.#keys, error);
        }
    }

    override async result(): Promise<AssistantMessage> {
        try {
            const message = this.#keys.scrub(
                await (await this.#source).result(),
            );
            if (isCutOff(message)) withheldIn(this.#keys, message);
            return message;
        } catch (error) {
            throw scrubbedError(this.#keys, error);
        }
    }

    /**
     * `event` with each text that may still grow withheld, and its delta
     * what the shown text grew by, as pi-durable appends the deltas to what
     * it kept. `shown` holds each block's text as last shown.
     */
    #withheld(
        event: AssistantMessageEvent,
        shown: Map<number, string>,
    ): AssistantMessageEvent {
        if (event.type === "error") withheldIn(this.#keys, event.error);
        if (!("partial" in event)) return event;
        withheldIn(
            this.#keys,
            event.partial,
            event.type === "text_end" || event.type === "thinking_end"
                ? event.contentIndex
                : undefined,
        );
        if (event.type === "text_delta" || event.type === "thinking_delta") {
            const block = event.partial.content[event.contentIndex];
            const text =
                block?.type === "text"
                    ? block.text
                    : block?.type === "thinking"
                      ? block.thinking
                      : undefined;
            if (text !== undefined) {
                const before = shown.get(event.contentIndex) ?? "";
                event.delta = text.slice(before.length);
                shown.set(event.contentIndex, text);
            }
        }
        return event;
    }
}

/** An answer that ended before its text did, so its text may end in part of a key. */
function isCutOff(message: AssistantMessage): boolean {
    return message.stopReason === "error" || message.stopReason === "aborted";
}

/** Withholds the tail of each text in `message` but block `ended`'s. */
function withheldIn(
    keys: Keys,
    message: AssistantMessage,
    ended?: number,
): void {
    for (const [index, block] of message.content.entries()) {
        if (index === ended) continue;
        if (block.type === "text") block.text = keys.withheld(block.text);
        else if (block.type === "thinking") {
            block.thinking = keys.withheld(block.thinking);
        }
    }
}

/** The spend gate keeps a thrown error's words as the answer. */
function scrubbedError(keys: Keys, error: unknown): Error {
    return new Error(keys.scrub(reasonOf(error)));
}

/** `provider::model`, as the catalogue and the copy name a model. */
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
 * The listing's cost, when it gives all four rates. A listing may leave
 * some out, whatever pi-ai's type says, and then today's server reads no
 * cost (`providers.rs`, `select`).
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

/** A model the listing holds, named as the catalogue names it. */
export interface ListedModel {
    readonly name: string;
    readonly title: string;
    readonly context: number;
    readonly price?: Price;
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

/** As today's check answers: the model's hello, its failure, or a refusal of the name. */
export interface Checked {
    readonly type: "answered" | "failed" | "refused";
    readonly words: string;
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
        const answer = await listed.completeSimple(
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
        return { type: "answered", words: keys.scrub(text) };
    } catch (error) {
        // As today, the words come from the error's text, scrubbed before
        // `plainly` cuts it.
        return { type: "failed", words: plainly(keys.scrub(String(error))) };
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
 * Puts the conversation `id` on the model and effort the copy names, in a
 * commit that writes nothing when it is on them. The copy is read again
 * after each write, and written again while it changed, so the copy read
 * last always wins over an older one written later.
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

/** The copy's {@link CopiedAgent}; `undefined` with no model copy. */
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

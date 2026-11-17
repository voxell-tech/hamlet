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
import type { Effort, Price } from "./messages.ts";
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
    of(provider: string): Promise<string | undefined>;
    /** `value` with every key held replaced, as today's scrub does. */
    scrub<T>(value: T): T;
}

export function keysOf(
    realm: Pick<BareRealm, "credential">,
    report: Peers["report"],
): Keys {
    const held = new Map<string, string>();
    return {
        of: async (provider) => {
            if (provider === FAUX) return undefined;
            const kept = held.get(provider);
            if (kept !== undefined) return kept;
            try {
                const answer = await realm.credential({ provider });
                // An OAuth login is pi-ai's to resolve and refresh, from a
                // credential store; a refusal or a failed call is asked again
                // at the next request.
                if (answer.type !== "key" || answer.key === "") {
                    return undefined;
                }
                held.set(provider, answer.key);
                return answer.key;
            } catch (error) {
                report(
                    `the realm's credential for ${provider} was not read`,
                    error,
                );
                return undefined;
            }
        },
        scrub: (value) => scrubbed(value, [...held.values()]),
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
        const [made, asked] = await served(model, options);
        return keys.scrub(await listed.completeSimple(made, transcript, asked));
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
        try {
            for await (const event of await this.#source)
                yield this.#keys.scrub(event);
        } catch (error) {
            throw this.#scrubbed(error);
        }
    }

    override async result(): Promise<AssistantMessage> {
        try {
            return this.#keys.scrub(await (await this.#source).result());
        } catch (error) {
            throw this.#scrubbed(error);
        }
    }

    /** The spend gate keeps a thrown error's words as the answer. */
    #scrubbed(error: unknown): Error {
        return new Error(this.#keys.scrub(reasonOf(error)));
    }
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

/** A listing may leave a model's cost out, whatever pi-ai's type says. */
function listedCost(model: Model<Api>): Model<Api>["cost"] | undefined {
    return model.cost;
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
        // Today's words come from the thrown error's text, scrubbed before
        // `plainly` cuts it.
        const error = `Error: ${answer.errorMessage ?? "the model answered with nothing"}`;
        return { type: "failed", words: plainly(keys.scrub(error)) };
    }
    return { type: "answered", words: keys.scrub(text) };
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
            await harness.commit(async (tx) => {
                const agent = await tx.doc(AgentDoc, id);
                if (
                    agent.model?.provider === wanted.model.provider &&
                    agent.model.modelId === wanted.model.modelId &&
                    (agent.thinkingLevel ?? null) === wanted.thinkingLevel
                ) {
                    return;
                }
                await configure(tx, id, wanted);
            }, context);
        }
    });
}

/**
 * What puts a conversation on the copy's model and effort, as
 * `createConversation`'s `agent` takes it; `undefined` with no model copy.
 */
export async function copiedAgent(
    copies: ReadCopies,
): Promise<{ model: ModelRef; thinkingLevel: Effort | null } | undefined> {
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

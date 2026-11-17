import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type Api,
    type AssistantMessage,
    calculateCost,
    createAssistantMessageEventStream,
    createFauxCore,
    createModels,
    createProvider,
    type FauxResponseFactory,
    fauxAssistantMessage,
    type Model,
    type Models,
    type MutableModels,
    type SimpleStreamOptions,
    type StreamFunction,
} from "@earendil-works/pi-ai";
import { type Copies, type ModelChoice, u32 } from "../src/agent/messages.ts";
import { checkModel, keysOf, modelList } from "../src/agent/models.ts";
import type { BareRealm } from "../src/agent/turn.ts";
import {
    fakeCalls,
    from,
    named,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

/** Made up; no provider has ever issued it. */
const KEY = "sk-made-up-7c0ffee-4u7";

const LISTED = { input: 2, output: 4, cacheRead: 1, cacheWrite: 3 };

/** Prices each answer from the model it is handed, as a real provider does. */
function priced(
    stream: StreamFunction<string, SimpleStreamOptions>,
): StreamFunction<string, SimpleStreamOptions> {
    return (model, transcript, options) => {
        const source = stream(model, transcript, options);
        const out = createAssistantMessageEventStream();
        void (async () => {
            for await (const event of source) {
                if (event.type === "done") {
                    event.message.usage.cost = calculateCost(
                        model,
                        event.message.usage,
                    );
                }
                out.push(event);
            }
            out.end(await source.result());
        })();
        return out;
    };
}

/** A model its listing gives no cost, as a listing may leave it out. */
function unpriced(id: string, baseUrl: string): Model<Api> {
    const { cost: _, ...rest } = createFauxCore({ provider: "loopback" })
        .models[0];
    return { ...rest, id, name: id, baseUrl } as unknown as Model<Api>;
}

/**
 * A second provider beside the agent's faux one, signed in with a key,
 * whose models price their answers.
 */
function loopback(models: readonly Model<Api>[] = []) {
    const core = createFauxCore({
        provider: "loopback",
        models: [
            {
                id: "large",
                name: "Large",
                cost: LISTED,
                contextWindow: 64_000,
            },
        ],
    });
    const provider = createProvider({
        id: "loopback",
        models: [...core.models, ...models],
        api: {
            stream: priced(core.stream),
            streamSimple: priced(core.streamSimple),
        },
        auth: {
            apiKey: {
                name: "loopback key",
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
    return {
        core,
        add: (listed: Models): Models => {
            (listed as MutableModels).setProvider(provider);
            return listed;
        },
    };
}

function signedIn() {
    return fakeCalls({
        realm: { credential: () => ({ type: "key", key: KEY }) },
    });
}

/** Records which model each request ran, then answers with `answer`. */
function ran(
    names: string[],
    answer: FauxResponseFactory | AssistantMessage,
): FauxResponseFactory {
    return async (transcript, options, state, model) => {
        names.push(`${model.provider}::${model.id}`);
        return typeof answer === "function"
            ? answer(transcript, options, state, model)
            : answer;
    };
}

function choosing(version: number, value: ModelChoice): Copies {
    return { model: { version, value } };
}

async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `line-${text}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

async function rootModel(agent: Woken) {
    const root = await (await agent.harness()).root(context);
    return (await root.agent(context)).model;
}

/** The root conversation's answers, oldest first. */
async function answers(agent: Woken): Promise<AssistantMessage[]> {
    const root = await (await agent.harness()).root(context);
    const page = await root.entries(
        { order: "ascending" },
        500,
        undefined,
        context,
    );
    return page.items
        .flatMap((entry) => entry.model ?? [])
        .filter((message) => message.role === "assistant");
}

async function lastAnswer(agent: Woken): Promise<AssistantMessage> {
    const answer = (await answers(agent)).at(-1);
    if (answer === undefined) throw new Error("nothing was answered");
    return answer;
}

test("a model changed mid-session runs from the next call", async (t) => {
    const { calls } = signedIn();
    const second = loopback();
    const names: string[] = [];
    let agent: Woken | undefined;
    agent = await wakeAgent(t, {
        calls,
        models: second.add,
        responses: [
            ran(names, async () => {
                // Kept while the run's first request is out, so the run is
                // busy when the copy arrives.
                if (agent === undefined) throw new Error("not woken");
                await agent.actions.copies(
                    agent.by(from.realm),
                    choosing(1, { model: "loopback::large" }),
                );
                return toolCall("read", { path: "nothing.md" });
            }),
        ],
    });
    second.core.setResponses([ran(names, fauxAssistantMessage("Read it."))]);
    const faux = agent.config.model.replace("/", "::");

    await say(agent, "Read the note.");

    assert.deepEqual(names, [faux, "loopback::large"]);
    assert.deepEqual(await rootModel(agent), {
        provider: "loopback",
        modelId: "large",
    });
    await agent.stop();
});

test("a configure cut off by a kill is done at the next wake", async (t) => {
    const { calls } = signedIn();
    const second = loopback();
    const names: string[] = [];
    const agent = await wakeAgent(t, {
        calls,
        models: second.add,
        responses: [fauxAssistantMessage("Hello.")],
    });
    await say(agent, "Hello.");

    const failed = agent.failNextCommit();
    await assert.rejects(
        agent.actions.copies(
            agent.by(from.realm),
            choosing(1, { model: "loopback::large" }),
        ),
    );
    await failed;
    await agent.stop();
    const next = await agent.again();
    // The wake configures without being awaited.
    await until(async () => (await rootModel(next))?.provider === "loopback");
    second.core.setResponses([ran(names, fauxAssistantMessage("Hello."))]);
    await say(next, "Hello again.");

    assert.deepEqual(names, ["loopback::large"]);
    await next.stop();
});

test("each answer names its model and its price", async (t) => {
    const { calls, sent } = signedIn();
    const second = loopback([
        unpriced("remote", "https://models.example/v1"),
        unpriced("home", "http://127.0.0.1:11434/v1"),
    ]);
    const agent = await wakeAgent(t, { calls, models: second.add });
    const flat = (rate: number) => ({
        input: rate,
        output: rate,
        cacheRead: rate,
        cacheWrite: rate,
    });
    const tiers: [ModelChoice, string, typeof LISTED][] = [
        [
            {
                model: "loopback::large",
                price: { input: 7, output: 11, cache_read: 5, cache_write: 13 },
            },
            "large",
            { input: 7, output: 11, cacheRead: 5, cacheWrite: 13 },
        ],
        [{ model: "loopback::large" }, "large", LISTED],
        [{ model: "loopback::remote" }, "remote", flat(1)],
        [{ model: "loopback::home" }, "home", flat(0.1)],
    ];

    for (const [version, [choice, id, rates]] of tiers.entries()) {
        await agent.actions.copies(
            agent.by(from.realm),
            choosing(version + 1, choice),
        );
        second.core.setResponses([fauxAssistantMessage("Priced.")]);
        await say(agent, `Price ${id} at tier ${version}.`);

        const answer = await lastAnswer(agent);
        const expected = calculateCost(
            { cost: rates } as Model<Api>,
            structuredClone(answer.usage),
        ).total;
        assert.equal(`${answer.provider}::${answer.model}`, choice.model);
        assert.ok(expected > 0, "the answer used tokens");
        assert.equal(answer.usage.cost.total, expected);
        const charged = named(sent, "charge").at(-1)?.message as {
            micros: number;
        };
        assert.equal(charged.micros, u32(expected * 1_000_000));
    }
    await agent.stop();
});

test("the copy's effort and fast tier reach its requests", async (t) => {
    const { calls } = signedIn();
    const second = loopback([
        {
            ...unpriced("swift", "https://models.example/v1"),
            api: "openai-responses",
        },
    ]);
    const asked: { reasoning?: string; payload?: unknown }[] = [];
    second.core.setResponses([
        async (_transcript, options, _state, model) => {
            asked.push({
                ...(options?.reasoning === undefined
                    ? {}
                    : { reasoning: options.reasoning }),
                payload: await options?.onPayload?.({}, model),
            });
            return fauxAssistantMessage("Quick.");
        },
    ]);
    const agent = await wakeAgent(t, { calls, models: second.add });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "loopback::swift", effort: "high", fast: true }),
    );

    await say(agent, "Be quick.");

    assert.equal(asked.length, 1);
    assert.equal(asked[0]?.reasoning, "high");
    assert.partialDeepStrictEqual(asked[0]?.payload, {
        service_tier: "priority",
    });
    await agent.stop();
});

/** A realm that answers `credential` and lists every call it was sent. */
function recordingRealm(): {
    realm: Pick<BareRealm, "credential">;
    asked: string[];
} {
    const asked: string[] = [];
    return {
        asked,
        realm: {
            credential: async () => {
                asked.push("credential");
                return { type: "key", key: KEY };
            },
        },
    };
}

function listing(): { listed: Models; second: ReturnType<typeof loopback> } {
    const second = loopback();
    return { listed: second.add(createModels()), second };
}

test("a check is not charged", async () => {
    const { listed, second } = listing();
    const { realm, asked } = recordingRealm();
    const keys = keysOf(realm, () => {});
    const keysSeen: (string | undefined)[] = [];
    const hello: FauxResponseFactory = (_transcript, options) => {
        keysSeen.push(options?.apiKey);
        return fauxAssistantMessage(" Hello! ");
    };
    second.core.setResponses([hello, hello]);

    const first = await checkModel(listed, keys, "loopback::large");
    const again = await checkModel(listed, keys, "loopback::large");

    assert.deepEqual(first, { type: "answered", words: "Hello!" });
    assert.deepEqual(again, first);
    assert.equal(second.core.state.callCount, 2, "a repeat asks again");
    assert.deepEqual(keysSeen, [KEY, KEY]);
    assert.deepEqual(asked, ["credential"], "nothing was admitted or charged");
    assert.deepEqual(await checkModel(listed, keys, "large"), {
        type: "refused",
        words: "`large` names no endpoint. A model is named `endpoint::model`.",
    });
    second.core.setResponses([
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "401 status code (no body)",
        }),
        (_transcript, options) =>
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: `${"x".repeat(150)} ${options?.apiKey}`,
            }),
    ]);
    assert.deepEqual(await checkModel(listed, keys, "loopback::large"), {
        type: "failed",
        words: "That key was not accepted. Check it was copied whole and is still active.",
    });
    // Scrubbed before the cut to 160 characters, so no part of the key is left.
    assert.deepEqual(await checkModel(listed, keys, "loopback::large"), {
        type: "failed",
        words: `That model did not answer a hello. Pick another one. (Error: ${"x".repeat(150)} [R)`,
    });
    assert.deepEqual(await checkModel(listed, keys, "nowhere::large"), {
        type: "refused",
        words: "`nowhere::large` is served by the endpoint `nowhere`, which the settings do not have.",
    });
});

test("the model list names each listed model with its window and price", () => {
    const { listed } = listing();

    assert.deepEqual(
        modelList(listed).find((model) => model.name === "loopback::large"),
        {
            name: "loopback::large",
            title: "Large",
            context: 64_000,
            price: { input: 2, output: 4, cache_read: 1, cache_write: 3 },
        },
    );
});

/** `listed` whose requests, while `broken()`, throw with the key in the words. */
function breaking(listed: Models, broken: () => boolean): Models {
    return new Proxy(listed, {
        get(target, name, receiver) {
            const value: unknown = Reflect.get(target, name, receiver);
            if (typeof value !== "function") return value;
            if (name !== "streamSimple") return value.bind(target);
            return (...args: Parameters<Models["streamSimple"]>) => {
                if (!broken()) return target.streamSimple(...args);
                const thrown = async () => {
                    throw new Error(`the stream broke on ${args[2]?.apiKey}`);
                };
                return {
                    [Symbol.asyncIterator]: () => ({ next: thrown }),
                    result: thrown,
                } as unknown as ReturnType<Models["streamSimple"]>;
            };
        },
    });
}

test("a key is in no database, log, error or event", async (t) => {
    const { calls, sent } = signedIn();
    const second = loopback();
    const keysSeen: (string | undefined)[] = [];
    second.core.setResponses([
        (_transcript, options) => {
            keysSeen.push(options?.apiKey);
            return fauxAssistantMessage(`My key is ${options?.apiKey}.`);
        },
        (_transcript, options) =>
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: `the model will not take ${options?.apiKey}`,
            }),
    ]);
    let breakNext = false;
    const agent = await wakeAgent(t, {
        calls,
        models: (models) => breaking(second.add(models), () => breakNext),
    });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "loopback::large" }),
    );
    await say(agent, "What is your key?");
    await say(agent, "Take it.");
    breakNext = true;
    await say(agent, "Take it again.");

    const { listed, second: checked } = listing();
    checked.core.setResponses([
        (_transcript, options) =>
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: `the model will not take ${options?.apiKey}`,
            }),
    ]);
    const words = (
        await checkModel(
            listed,
            keysOf(recordingRealm().realm, () => {}),
            "loopback::large",
        )
    ).words;

    assert.deepEqual(keysSeen, [KEY], "the provider was given the key");
    assert.equal(
        words,
        "That model did not answer a hello. Pick another one. (Error: the model will not take [REDACTED])",
    );
    assert.deepEqual(
        (await answers(agent)).map((answer) => answer.errorMessage),
        [
            undefined,
            "the model will not take [REDACTED]",
            "the stream broke on [REDACTED]",
        ],
    );
    const tables = await agent.c.db.all<{ name: string }>(
        "SELECT name FROM sqlite_master WHERE type = 'table'",
    );
    const decoder = new TextDecoder();
    for (const { name } of tables) {
        const rows = await agent.c.db.all<Record<string, unknown>>(
            `SELECT * FROM "${name}"`,
        );
        const text = JSON.stringify(rows, (_key, value: unknown) =>
            value instanceof Uint8Array ? decoder.decode(value) : value,
        );
        assert.ok(!text.includes(KEY), `the table ${name} holds the key`);
    }
    for (const [what, seen] of Object.entries({
        log: agent.reported,
        events: agent.events,
        calls: sent.map((call) => call.message),
    })) {
        assert.ok(
            !JSON.stringify(seen).includes(KEY),
            `the ${what} hold the key`,
        );
    }
    assert.match(
        JSON.stringify((await answers(agent))[0]?.content),
        /My key is \[REDACTED\]\./,
    );
    await agent.stop();
});

test("a new conversation takes the copy's model", {
    todo: "sessions.ts makes a conversation on the host's model: each `createConversation` should pass `agent: await copiedAgent(copies)`",
}, async (t) => {
    const { calls } = signedIn();
    const second = loopback();
    const names: string[] = [];
    const agent = await wakeAgent(t, { calls, models: second.add });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "loopback::large" }),
    );
    second.core.setResponses([
        ran(names, fauxAssistantMessage("One.")),
        ran(names, fauxAssistantMessage("Two.")),
    ]);
    await say(agent, "One.");
    await agent.actions.new_session(agent.by(from.client), {
        id: "new-1",
        to: 7,
    });
    await say(agent, "Two.");

    assert.deepEqual(names, ["loopback::large", "loopback::large"]);
    await agent.stop();
});

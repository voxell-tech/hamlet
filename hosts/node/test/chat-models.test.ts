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
import { getCurrentSystemPrompt } from "@earendil-works/pi-ai/utils/transcript";
import type { Copies, ModelChoice } from "../src/agent/messages/agent.ts";
import { u32 } from "../src/agent/messages/shared.ts";
import {
    agentModels,
    checkModel,
    keysOf,
    modelList,
    scrub,
} from "../src/agent/models.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import type { BareRealm, Peers } from "../src/agent/turn.ts";
import {
    dumped,
    fakeCalls,
    from,
    itemOfLine,
    named,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context, fixture } from "./testing.ts";

/** Made up; no provider has ever issued it. */
const KEY = "sk-made-up-7c0ffee-4u7";

const LISTED = { input: 2, output: 4, cacheRead: 1, cacheWrite: 3 };

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

function unpriced(id: string, baseUrl: string): Model<Api> {
    const { cost: _, ...rest } = createFauxCore({ provider: "loopback" })
        .models[0];
    return { ...rest, id, name: id, baseUrl } as unknown as Model<Api>;
}

function partlyPriced(): Model<Api> {
    return {
        ...unpriced("partly", "https://models.example/v1"),
        cost: { input: 3, output: 5 },
    } as unknown as Model<Api>;
}

function failingWith(
    failing: (key: string | undefined) => Error | undefined,
    stream: StreamFunction<string, SimpleStreamOptions>,
): StreamFunction<string, SimpleStreamOptions> {
    return (model, transcript, options) => {
        const error = failing(options?.apiKey);
        if (error === undefined) return stream(model, transcript, options);
        const thrown = async () => {
            throw error;
        };
        return {
            [Symbol.asyncIterator]: () => ({ next: thrown }),
            result: thrown,
        } as unknown as ReturnType<typeof stream>;
    };
}

/**
 * A faux provider; `failing` gives the error its stream throws for a
 * request's key, or nothing to answer.
 */
function loopback(
    models: readonly Model<Api>[] = [],
    failing: (key: string | undefined) => Error | undefined = () => undefined,
) {
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
            stream: failingWith(failing, priced(core.stream)),
            streamSimple: failingWith(failing, priced(core.streamSimple)),
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
    assert.equal((await rootModel(next))?.provider, "loopback");
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
        partlyPriced(),
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
        // A listing's cost counts only with all four rates.
        [{ model: "loopback::partly" }, "partly", flat(1)],
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

test("an endpoint's billed cost stands over the price, as today's harness charges it", async (t) => {
    const { calls, sent } = signedIn();
    const second = loopback();
    const agent = await wakeAgent(t, { calls, models: second.add });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "loopback::large" }),
    );
    second.core.setResponses([
        async (_transcript, options, _state, model) => {
            await options?.onProviderStreamEvent?.(
                { usage: { cost: 0.0015 } },
                model,
            );
            return fauxAssistantMessage("Billed.");
        },
    ]);
    await say(agent, "Bill me.");

    assert.equal((await lastAnswer(agent)).usage.cost.total, 0.0015);
    const charged = named(sent, "charge").at(-1)?.message as {
        micros: number;
    };
    assert.equal(charged.micros, 1500);
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
    const second = loopback([partlyPriced()]);
    return { listed: second.add(createModels()), second };
}

test("a client reads the listing's models and checks one with the realm's key", async (t) => {
    const { calls, sent } = signedIn();
    const second = loopback();
    const keysSeen: (string | undefined)[] = [];
    second.core.setResponses([
        (_transcript, options) => {
            keysSeen.push(options?.apiKey);
            return fauxAssistantMessage(" Hello! ");
        },
    ]);
    const agent = await wakeAgent(t, { calls, models: second.add });
    const client = agent.by(from.client);

    const { models } = await agent.actions.read_models(client);
    const checked = await agent.actions.check(client, {
        model: "loopback::large",
    });

    assert.deepEqual(models, modelList(agent.config.models));
    assert.ok(models.some((model) => model.name === "loopback::large"));
    assert.deepEqual(checked, { type: "answered", words: "Hello!" });
    assert.deepEqual(keysSeen, [KEY]);
    assert.deepEqual(
        named(sent, "credential").map((call) => call.message),
        [
            { provider: "faux" },
            { provider: "loopback" },
            { provider: "loopback" },
        ],
    );
    await agent.stop();
});

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
            images: 20,
            price: { input: 2, output: 4, cache_read: 1, cache_write: 3 },
            efforts: ["off"],
        },
    );
    assert.deepEqual(
        modelList(listed).find((model) => model.name === "loopback::partly"),
        {
            name: "loopback::partly",
            title: "partly",
            context: createFauxCore({}).models[0]?.contextWindow,
            images: 20,
            efforts: ["off"],
        },
    );
});

test("the model list says how many images one line gives a model, as today's listing", () => {
    const listed = createModels();
    const taking = (id: string, input: Model<Api>["input"], most?: number) => ({
        id,
        input,
        ...(most === undefined
            ? {}
            : { inputLimits: { images: { maxPerMessage: most } } }),
    });
    listed.setProvider(
        served(
            "local",
            [
                taking("plain", ["text"]),
                taking("sees", ["text", "image"]),
                taking("few", ["text", "image"], 5),
                taking("many", ["text", "image"], 50),
            ],
            fauxAssistantMessage("Hello."),
        ),
    );

    // As `listed_images` in `crates/hamlet_server/src/providers.rs` reads a
    // listing and `Limits::of` in `crates/hamlet_server/src/images.rs` caps it.
    assert.deepEqual(
        modelList(listed).map(({ name, images }) => ({ name, images })),
        [
            { name: "local::plain", images: undefined },
            { name: "local::sees", images: 20 },
            { name: "local::few", images: 5 },
            { name: "local::many", images: 20 },
        ],
    );
});

test("a key is in no database, log, error or event", async (t) => {
    const { calls, sent } = signedIn();
    let breakNext = false;
    const second = loopback([], (key) =>
        breakNext ? new Error(`the stream broke on ${key}`) : undefined,
    );
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
    const agent = await wakeAgent(t, { calls, models: second.add });
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
    assertNoPartOfKey(await dumped(agent));
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

test("a new conversation takes the copy's model", async (t) => {
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

function splitting(
    chunks: readonly string[],
    end: "breaks" | "waits",
    into: "text" | "call" = "text",
): (listed: Models) => Models {
    const model = {
        ...createFauxCore({}).models[0],
        provider: "split",
        id: "split",
        name: "split",
    } as Model<Api>;
    const stream: StreamFunction<string, SimpleStreamOptions> = (
        made,
        _transcript,
        options,
    ) => {
        const out = createAssistantMessageEventStream();
        const message: AssistantMessage = {
            ...fauxAssistantMessage(""),
            api: made.api,
            provider: made.provider,
            model: made.id,
            content: [
                into === "text"
                    ? { type: "text", text: "" }
                    : {
                          type: "toolCall",
                          id: "call-with-key",
                          name: "read",
                          arguments: {},
                      },
            ],
        };
        const block = message.content[0];
        out.push({ type: "start", partial: structuredClone(message) });
        out.push({
            type: into === "text" ? "text_start" : "toolcall_start",
            contentIndex: 0,
            partial: structuredClone(message),
        });
        for (const chunk of chunks) {
            if (block?.type === "text") block.text += chunk;
            out.push({
                type: into === "text" ? "text_delta" : "toolcall_delta",
                contentIndex: 0,
                delta: chunk,
                partial: structuredClone(message),
            });
        }
        const failed = (reason: "aborted" | "error") => {
            const error = { ...structuredClone(message), stopReason: reason };
            out.push({ type: "error", reason, error });
            out.end(error);
        };
        if (end === "breaks") failed("error");
        else if (options?.signal?.aborted) failed("aborted");
        else
            options?.signal?.addEventListener("abort", () => failed("aborted"));
        return out;
    };
    const provider = createProvider({
        id: "split",
        models: [model],
        api: { stream, streamSimple: stream },
        auth: {
            apiKey: {
                name: "split key",
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
    return (listed) => {
        (listed as MutableModels).setProvider(provider);
        return listed;
    };
}

/** Stores each partial as it is committed, so a test waits only for the commit. */
const STORED_AT_ONCE = { progress: { persistWithinMs: 0 } };

for (const at of [6, 12, KEY.length - 1]) {
    test(`a kill mid-stream after ${at} characters of a key keeps none of them`, async (t) => {
        const { calls } = signedIn();
        const agent = await wakeAgent(t, {
            calls,
            settings: STORED_AT_ONCE,
            models: splitting([`My key is ${KEY.slice(0, at)}`], "waits"),
        });
        await agent.actions.copies(
            agent.by(from.realm),
            choosing(1, { model: "split::split" }),
        );
        await agent.actions.submit(agent.by(from.realm), {
            id: "line-key",
            input: { type: "line", text: "Say your key." },
        });
        // The partial holding the delta is kept before the kill.
        await until(async () => (await dumped(agent)).includes("My key is"));
        await agent.stop();
        const next = await agent.again();
        await until(async () =>
            (await next.entries()).some((entry) =>
                entry.text.includes("My key is"),
            ),
        );

        assertNoPartOfKey(await dumped(next));
        await next.stop();
    });
}

test("a kill while a call's arguments stream a key keeps none of it", async (t) => {
    const { calls } = signedIn();
    const agent = await wakeAgent(t, {
        calls,
        settings: STORED_AT_ONCE,
        models: splitting(
            [`{"path":"${KEY.slice(0, 12)}`, `${KEY.slice(12)}.md"}`],
            "waits",
            "call",
        ),
    });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "split::split" }),
    );
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-key",
        input: { type: "line", text: "Read it." },
    });
    // The partial holding both deltas is kept before the kill.
    await until(async () => (await dumped(agent)).includes("call-with-key"));
    await agent.stop();
    const next = await agent.again();
    await until(async () =>
        (await next.entries()).some((entry) => entry.kind === "pi.assistant"),
    );

    const kept = await dumped(next);
    assertNoPartOfKey(kept);
    assert.ok(kept.includes("[REDACTED].md"), "the call's arguments are kept");
    await next.stop();
});

function assertNoPartOfKey(text: string): void {
    for (let length = 5; length <= KEY.length; length += 1) {
        const prefix = KEY.slice(0, length);
        assert.ok(!text.includes(prefix), `the database holds ${prefix}`);
    }
}

test("an answer that breaks mid-key keeps none of it", async (t) => {
    const { calls } = signedIn();
    const agent = await wakeAgent(t, {
        calls,
        models: splitting([`My key is ${KEY.slice(0, 12)}`], "breaks"),
    });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "split::split" }),
    );

    await say(agent, "Say your key.");

    assert.equal((await lastAnswer(agent)).stopReason, "error");
    assertNoPartOfKey(await dumped(agent));
    await agent.stop();
});

/**
 * The provider `loopback` resolving a key as pi-ai's own do: the one it is
 * given, else the host's environment's.
 */
function fromEnvironment(variable: string, keysSeen: (string | undefined)[]) {
    const core = createFauxCore({
        provider: "loopback",
        models: [{ id: "large", name: "Large", cost: LISTED }],
    });
    core.setResponses([
        (_transcript, options) => {
            keysSeen.push(options?.apiKey);
            return fauxAssistantMessage(`Paid with ${options?.apiKey}.`);
        },
    ]);
    const provider = createProvider({
        id: "loopback",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            apiKey: {
                name: "loopback key",
                resolve: async ({ ctx, credential }) => {
                    const key = credential?.key ?? (await ctx.env(variable));
                    return key === undefined
                        ? undefined
                        : { auth: { apiKey: key } };
                },
            },
        },
    });
    return (listed: Models): Models => {
        (listed as MutableModels).setProvider(provider);
        return listed;
    };
}

const NOT_SIGNED_IN: Awaited<ReturnType<BareRealm["credential"]>> = {
    type: "refused",
    why: { type: "not_signed_in", provider: "loopback" },
    words: "Sign in to loopback first.",
};

test("a request with no key from the realm fails and uses no other key", async (t) => {
    const variable = "HAMLET_TEST_LOOPBACK_KEY";
    process.env[variable] = "sk-made-up-host-0dd";
    t.after(() => delete process.env[variable]);
    const answers: (() => Awaited<ReturnType<BareRealm["credential"]>>)[] = [
        () => NOT_SIGNED_IN,
        () => ({
            type: "oauth",
            login: { access: "made-up-access", refresh: "made-up", expires: 1 },
        }),
        () => {
            throw new Error("the realm is gone");
        },
    ];
    for (const [tried, answer] of answers.entries()) {
        const { calls, sent } = fakeCalls({
            realm: { credential: answer },
        });
        const keysSeen: (string | undefined)[] = [];
        const agent = await wakeAgent(t, {
            calls,
            models: fromEnvironment(variable, keysSeen),
        });
        await agent.actions.copies(
            agent.by(from.realm),
            choosing(1, { model: "loopback::large" }),
        );

        await say(agent, "Who pays?");

        assert.deepEqual(keysSeen, [], `the provider ran on answer ${tried}`);
        const failed = (await lastAnswer(agent)).errorMessage;
        assert.ok(failed !== undefined, "the request failed");
        if (tried === 0) assert.equal(failed, NOT_SIGNED_IN.words);
        assert.ok(!(await dumped(agent)).includes("sk-made-up-host-0dd"));
        // The request asks once, and the wake may ask for the copy's key
        // once more; nothing asks again.
        assert.ok(named(sent, "credential").length <= 2);
        await agent.stop();
    }
});

test("a check whose request throws answers scrubbed failed words", async () => {
    const { listed } = listing();
    const throwing = loopback(
        [],
        (key) => new Error(`401 Incorrect API key provided: ${key}`),
    ).add(createModels());
    const keys = keysOf(recordingRealm().realm, () => {});

    assert.deepEqual(await checkModel(throwing, keys, "loopback::large"), {
        type: "failed",
        words: "That key was not accepted. Check it was copied whole and is still active.",
    });
    const refused = await checkModel(
        listed,
        keysOf({ credential: async () => NOT_SIGNED_IN }, () => {}),
        "loopback::large",
    );
    assert.equal(refused.type, "failed");
    assert.match(refused.words, /Sign in to loopback first\./);
});

test("an agent's request that fails with the key fails with it scrubbed", async () => {
    const throwing = loopback([], (key) => new Error(`refused ${key}`)).add(
        createModels(),
    );
    const peers = {
        reach: recordingRealm().realm,
        report: () => {},
    } as unknown as Peers;
    const models = agentModels(throwing, async () => ({}), peers);
    const model = throwing.getModel("loopback", "large");
    assert.ok(model !== undefined);

    const answer = await models.completeSimple(model, { messages: [] });
    assert.equal(answer.stopReason, "error");
    assert.equal(answer.errorMessage, "refused [REDACTED]");
});

test("requests at once ask the realm for a key once", async () => {
    let asked = 0;
    let answer: Awaited<ReturnType<BareRealm["credential"]>> = NOT_SIGNED_IN;
    let gate = Promise.withResolvers<void>();
    const keys = keysOf(
        {
            credential: async () => {
                asked += 1;
                await gate.promise;
                return answer;
            },
        },
        () => {},
    );
    const both = () => [keys.of("loopback"), keys.of("loopback")];

    const refused = both();
    gate.resolve();
    await Promise.allSettled(refused);
    assert.equal(asked, 1, "two requests at once asked once");
    for (const each of refused) {
        await assert.rejects(each, /Sign in to loopback first\./);
    }
    answer = { type: "key", key: KEY };
    gate = Promise.withResolvers<void>();
    const granted = both();
    gate.resolve();
    assert.deepEqual(await Promise.all(granted), [
        { apiKey: KEY },
        { apiKey: KEY },
    ]);
    assert.equal(asked, 2, "a refusal is asked again");
});

test("a key of settings alone serves a request with its settings and no key", async () => {
    const core = createFauxCore({
        provider: "profiled",
        models: [{ id: "large", name: "Large" }],
    });
    const seen: unknown[] = [];
    core.setResponses([
        (_transcript, options) => {
            seen.push({ apiKey: options?.apiKey, env: options?.env });
            return fauxAssistantMessage("Hello");
        },
    ]);
    const listed = createModels();
    (listed as MutableModels).setProvider(
        createProvider({
            id: "profiled",
            models: core.models,
            api: { stream: core.stream, streamSimple: core.streamSimple },
            auth: {
                apiKey: {
                    name: "a profile",
                    // As pi-ai's Bedrock: a profile in the settings signs in.
                    resolve: async ({ ctx, credential }) =>
                        (credential?.env?.PROFILE ??
                            (await ctx.env("PROFILE"))) === undefined
                            ? undefined
                            : { auth: {} },
                },
            },
        }),
    );
    const env = { PROFILE: "work" };
    const keys = keysOf(
        { credential: async () => ({ type: "key", key: "", env }) },
        () => {},
    );

    assert.deepEqual(await checkModel(listed, keys, "profiled::large"), {
        type: "answered",
        words: "Hello",
    });
    assert.deepStrictEqual(seen, [{ apiKey: undefined, env }]);
});

test("a request reads nothing of the host's environment", async (t) => {
    const name = "HAMLET_TEST_HOST_TOKEN";
    process.env[name] = "host-made-up-token";
    t.after(() => {
        delete process.env[name];
    });
    const core = createFauxCore({
        provider: "hosted",
        models: [{ id: "large", name: "Large" }],
    });
    const seen: unknown[] = [];
    core.setResponses([
        (_transcript, options) => {
            seen.push(options?.apiKey);
            return fauxAssistantMessage("Hello");
        },
    ]);
    const listed = createModels();
    (listed as MutableModels).setProvider(
        createProvider({
            id: "hosted",
            models: core.models,
            api: { stream: core.stream, streamSimple: core.streamSimple },
            auth: {
                apiKey: {
                    name: "a token",
                    // As pi-ai's Bedrock: a token in the environment comes first.
                    resolve: async ({ ctx }) => ({
                        auth: { apiKey: (await ctx.env(name)) ?? "none" },
                    }),
                },
            },
        }),
    );
    const keys = keysOf(
        {
            credential: async () => ({
                type: "key",
                key: "",
                env: { PROFILE: "work" },
            }),
        },
        () => {},
    );

    await checkModel(listed, keys, "hosted::large");
    assert.deepStrictEqual(seen, ["none"]);
});

async function currentSession(agent: Woken) {
    const id = await conversationOfChat(agent.by(from.client), { with: 7 });
    assert.ok(id !== undefined, "the chief has a current session");
    const conversation = await (await agent.harness()).conversation(
        id,
        context,
    );
    assert.ok(conversation !== undefined, "the session has a conversation");
    return conversation;
}

async function sessionModel(agent: Woken) {
    return (await (await currentSession(agent)).agent(context)).model;
}

test("a /new whose copy read waits behind a newer copy ends on it", async (t) => {
    const { calls } = signedIn();
    const second = loopback([
        {
            ...createFauxCore({}).models[0],
            id: "other",
            name: "Other",
        } as Model<Api>,
    ]);
    const agent = await wakeAgent(t, { calls, models: second.add });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "loopback::large" }),
    );
    second.core.setResponses([fauxAssistantMessage("Root.")]);
    await say(agent, "Root first.");
    // Holds the /new's first read of the copy until the newer copy is kept.
    const db = agent.c.db as { all: (...args: unknown[]) => Promise<unknown> };
    const all = db.all.bind(db);
    const held = Promise.withResolvers<void>();
    const reached = Promise.withResolvers<void>();
    let armed = true;
    db.all = async (...args: unknown[]) => {
        const rows = await all(...args);
        if (armed && String(args[0]).includes("FROM copy")) {
            armed = false;
            reached.resolve();
            await held.promise;
        }
        return rows;
    };

    const begun = agent.actions.new_session(agent.by(from.client), {
        id: "new-held",
        to: 7,
    });
    await reached.promise;
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(2, { model: "loopback::other" }),
    );
    held.resolve();
    await begun;

    assert.deepEqual(await sessionModel(agent), {
        provider: "loopback",
        modelId: "other",
    });
    await agent.stop();
});

test("a rewind's conversation runs on the copy's model", async (t) => {
    const { calls } = signedIn();
    const second = loopback();
    const names: string[] = [];
    const agent = await wakeAgent(t, { calls, models: second.add });
    await agent.actions.copies(
        agent.by(from.realm),
        choosing(1, { model: "loopback::large" }),
    );
    second.core.setResponses(
        Array.from({ length: 4 }, () =>
            ran(names, fauxAssistantMessage("Ok.")),
        ),
    );
    await say(agent, "First line.");
    await say(agent, "Second line.");

    // The first rewind forks before the second line; the second leaves the
    // session empty.
    for (const [line, again] of [
        ["Second line.", "Second, again."],
        ["First line.", "From the start."],
    ] as const) {
        const rewound = await agent.actions.rewind(agent.by(from.client), {
            id: `rewind-${line}`,
            chat: { with: 7 },
            item: await itemOfLine(agent, line),
        });
        assert.equal(rewound.type, "rewound");
        await say(agent, again);
    }

    assert.deepEqual(names, Array(4).fill("loopback::large"));
    await agent.stop();
});

test("a hire's first line runs on the model and charter its create input holds, while the realm has not answered its read", async (t) => {
    const read = Promise.withResolvers<Copies>();
    const { calls, sent } = fakeCalls({
        realm: {
            credential: () => ({ type: "key", key: KEY }),
            read_copies: () => read.promise,
        },
    });
    const second = loopback();
    const names: string[] = [];
    const prompts: string[] = [];
    const agent = await wakeAgent(t, {
        calls,
        models: second.add,
        input: {
            kind: "hire",
            task: "standing-7",
            place: {
                version: 1,
                hires: [],
                roads: [],
                tools_off: [],
                manual: false,
                agrees_roads: false,
            },
            copies: {
                charter: { version: 1, value: "Keep the docs whole." },
                ...choosing(1, { model: "loopback::large" }),
            },
        },
    });
    second.core.setResponses([
        ran(names, (transcript) => {
            prompts.push(getCurrentSystemPrompt(transcript.messages) ?? "");
            return fauxAssistantMessage("Hello.");
        }),
    ]);
    await until(() => named(sent, "read_copies").length === 1);

    await say(agent, "Hello.");

    assert.deepEqual(names, ["loopback::large"]);
    assert.match(prompts[0] ?? "", /^Keep the docs whole\./);
    read.resolve({});
    await agent.stop();
});

/**
 * A provider of faux `models` whose requests answer `answer`, each model
 * without a cost unless it is given one.
 */
function served(
    id: string,
    models: NonNullable<Parameters<typeof createFauxCore>[0]["models"]>,
    answer: AssistantMessage,
) {
    const core = createFauxCore({ provider: id, models });
    core.setResponses([answer]);
    return createProvider({
        id,
        models: core.models.map((model, index) =>
            models[index]?.cost === undefined
                ? ({ ...model, cost: undefined } as unknown as Model<Api>)
                : model,
        ),
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            apiKey: {
                name: `${id} key`,
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
}

test("a client's read_models and check answer as their fixtures hold", async (t) => {
    const listed = createModels();
    listed.setProvider(
        served(
            "anthropic",
            [
                {
                    id: "claude-sonnet-4-5",
                    name: "Claude Sonnet 4.5",
                    input: ["text"],
                    contextWindow: 200_000,
                    cost: {
                        input: 3,
                        output: 15,
                        cacheRead: 0.3,
                        cacheWrite: 3.75,
                    },
                },
            ],
            fauxAssistantMessage("Hello."),
        ),
    );
    listed.setProvider(
        served(
            "local",
            [
                {
                    id: "small",
                    name: "Small",
                    input: ["text"],
                    contextWindow: 8192,
                },
            ],
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: "the provider answered 401",
            }),
        ),
    );
    const agent = await wakeAgent(t, {
        calls: signedIn().calls,
        models: () => listed,
    });
    const client = agent.by(from.client);
    const [check] = fixture("agent/check.args") as [{ model: string }];

    assert.deepStrictEqual(
        await agent.actions.read_models(client),
        fixture("agent/read_models.answer"),
    );
    assert.deepStrictEqual(
        await agent.actions.check(client, check),
        fixture("agent/check.answered.answer"),
    );
    // Their fixtures' words are not words the agent writes, so only their
    // fields and the type of each are compared.
    const formOf = (answer: object) =>
        Object.entries(answer).map(([key, value]) => [
            key,
            key === "type" ? value : typeof value,
        ]);
    for (const [model, name] of [
        ["local::small", "agent/check.failed.answer"],
        ["anthropic::nope", "agent/check.refused.answer"],
    ] as const) {
        assert.deepStrictEqual(
            formOf(await agent.actions.check(client, { model })),
            formOf(fixture(name) as object),
            name,
        );
    }
    await agent.stop();
});

test("a scrub replaces the secret that begins first, and the longest of those that begin at one place, as today", () => {
    // As `crates/hamlet_sandbox/src/scrub.rs` `Scrubber::replace`.
    assert.equal(scrub("abcde", ["bcde", "ab"]), "[REDACTED]cde");
    assert.equal(scrub("abcde", ["ab", "abc"]), "[REDACTED]de");
});

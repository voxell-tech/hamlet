import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type AssistantMessage,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
} from "@earendil-works/pi-ai";
import type { Done, TurnFailed } from "../src/agent/messages.ts";
import {
    fakeCalls,
    from,
    named,
    until,
    type Woken,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

/** As `crates/hamlet_messages/src/realm/budget.rs` words `OverCost`. */
const SPENT =
    "the budget of the realm has run out: 5000 of 5000 millionths of a dollar spent; a chief starts it again with /reset-budget";

const refused: Done = {
    type: "refused",
    why: { type: "over_cost" },
    words: SPENT,
};

/** As `crates/hamlet_messages/src/realm/oversight.rs` words `Hold::Objection`. */
const held = {
    type: "refused",
    why: { type: "objection", task: "task-1" },
    words: "task task-1 is held on an objection until a chief says go or withdraw",
} as const;

/** Today's, in `crates/hamlet_server/src/talk/verdicts.rs`. */
const EMPTY = "the model answered with nothing. Send the message again";
const NO_ROOM =
    "the model has no room left to answer, even with the conversation summarised. Start a new session, or choose a model that holds more";

async function say(agent: Woken, n: number, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `input-${n}`,
        task: `task-${n}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

async function notices(agent: Woken): Promise<{ text: string; at: number }[]> {
    return (await agent.entries())
        .filter((entry) => entry.kind === "hamlet.notice")
        .map((entry) => entry.data as { text: string; at: number });
}

async function rootRef(agent: Woken): Promise<string> {
    return String((await (await agent.harness()).root(context)).id);
}

test("a refused call leaves a notice naming /reset-budget", async (t) => {
    const { calls } = fakeCalls({ realm: { may_spend: () => refused } });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("never sent")],
    });

    await say(agent, 1, "Go.");

    assert.equal(agent.faux.state.callCount, 0);
    const [notice, ...more] = await notices(agent);
    assert.equal(notice?.text, SPENT);
    assert.match(notice?.text ?? "", /\/reset-budget/);
    assert.ok(Number.isInteger(notice?.at) && (notice?.at ?? 0) < 2 ** 32);
    assert.deepEqual(more, []);
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a refused call in a chief's chat tells the realm the turn failed", async (t) => {
    const { calls, sent } = fakeCalls({ realm: { may_spend: () => refused } });
    const agent = await wakeAgent(t, { calls });

    await say(agent, 1, "Go.");
    await agent.actions.submit(agent.by(from.realm), {
        id: "letter-1",
        task: "task-2",
        input: { type: "letter", from: { agent: 3 }, text: "Hello." },
    });
    await agent.idle();

    const failed = {
        task: "task-1",
        conversation: await rootRef(agent),
        words: SPENT,
        cause: "budget",
    };
    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [failed],
        "a letter's chat is no chief's: its refusal tells nothing",
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a held call ends quietly", async (t) => {
    const answers = [held, refused];
    const { calls, sent } = fakeCalls({
        realm: { may_spend: () => answers.shift() ?? refused },
    });
    const agent = await wakeAgent(t, { calls });

    await say(agent, 1, "Go.");
    await say(agent, 2, "Go on.");

    assert.deepEqual(
        (await notices(agent)).map((notice) => notice.text),
        [SPENT],
    );
    assert.deepEqual(
        named(sent, "turn_failed").map(
            (call) => (call.message as TurnFailed).task,
        ),
        ["task-2"],
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("an empty answer fails its turn with today's words", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("  ")],
    });

    await say(agent, 1, "Go.");
    await until(() => named(sent, "turn_failed").length > 0);

    assert.deepEqual(
        (await notices(agent)).map((notice) => notice.text),
        [EMPTY],
    );
    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [
            {
                task: "task-1",
                conversation: await rootRef(agent),
                words: EMPTY,
                cause: "model",
            },
        ],
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a provider error that ends the turn tells the realm its words", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage([], {
                stopReason: "error",
                errorMessage: "invalid x-api-key",
            }),
        ],
    });

    await say(agent, 1, "Go.");
    await until(() => named(sent, "turn_failed").length > 0);

    assert.deepEqual(await notices(agent), []);
    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [
            {
                task: "task-1",
                conversation: await rootRef(agent),
                words: "invalid x-api-key",
                cause: "model",
            },
        ],
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

const overloaded = () =>
    fauxAssistantMessage([], {
        stopReason: "error",
        errorMessage: "overloaded",
    });

test("a provider error tried again tells the realm only when the turn fails", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        settings: { retry: { enabled: true, maxRetries: 2, baseDelayMs: 1 } },
        responses: [
            overloaded(),
            fauxAssistantMessage("Fine."),
            fauxAssistantMessage([], {
                stopReason: "error",
                errorMessage: "invalid x-api-key",
            }),
        ],
    });

    await say(agent, 1, "Go.");
    await say(agent, 2, "Again.");
    await until(() => named(sent, "turn_failed").length > 0);

    assert.deepEqual(
        named(sent, "turn_failed").map(
            (call) => (call.message as TurnFailed).task,
        ),
        ["task-2"],
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a stop while a provider error waits to be tried again ends the agent", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        // Long enough that the retry is still asleep at the stop.
        settings: {
            retry: { enabled: true, maxRetries: 2, baseDelayMs: 600_000 },
        },
        responses: [overloaded()],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await until(async () =>
        (await agent.entries()).some((entry) => entry.kind === "pi.assistant"),
    );

    // A stop takes milliseconds; a hang waits for the retry's sleep.
    assert.equal(await within(agent.stop(), 30_000, "hung"), undefined);
    // What the close cut off has settled by the end of the next wake.
    await (await agent.again()).stop();
    assert.deepEqual(agent.reported, []);
    assert.deepEqual(named(sent, "turn_failed"), []);
});

/** About 10,000 tokens by pi-ai's estimate, so three leave room to compact. */
function long(n: number): string {
    return `Line ${n}. ${"word ".repeat(7_000)}`;
}

function textOf(message: Message): string {
    if (typeof message.content === "string") return message.content;
    return message.content
        .map((part) => (part.type === "text" ? part.text : ""))
        .join("");
}

function script(answers: AssistantMessage[]): FauxResponseStep[] {
    const step: FauxResponseStep = (request) =>
        request.messages
            .map(textOf)
            .some((text) => text.startsWith("<conversation>"))
            ? fauxAssistantMessage("Lines 1 to 3 asked for words.")
            : (answers.shift() ?? fauxAssistantMessage("No answer was left."));
    return Array.from({ length: 20 }, () => step);
}

test("a second short cut-off tells the realm the turn failed", async (t) => {
    const cutOff = () => fauxAssistantMessage([], { stopReason: "length" });
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: script([
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Two."),
            fauxAssistantMessage("Three."),
            cutOff(),
            cutOff(),
        ]),
    });

    for (const n of [1, 2, 3]) await say(agent, n, long(n));
    await say(agent, 4, "Line 4. Answer now.");
    await until(() => named(sent, "turn_failed").length > 0);

    assert.deepEqual(
        (await notices(agent)).map((notice) => notice.text),
        [NO_ROOM],
    );
    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [
            {
                task: "task-4",
                conversation: await rootRef(agent),
                words: NO_ROOM,
                cause: "model",
            },
        ],
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a kill while the realm is told of a refusal sends nothing again", async (t) => {
    const told = Promise.withResolvers<void>();
    const { calls, sent } = fakeCalls({
        realm: {
            may_spend: () => refused,
            // The first is cut off by the kill and never answered.
            turn_failed: () => {
                told.resolve();
                return new Promise<never>(() => {});
            },
        },
    });
    const agent = await wakeAgent(t, { calls });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await told.promise;
    await agent.idle();
    await agent.stop();

    const again = await agent.again();
    await again.idle();

    assert.equal(named(sent, "may_spend").length, 1);
    assert.deepEqual(
        (await notices(again)).map((notice) => notice.text),
        [SPENT],
    );
    assert.equal(
        named(sent, "turn_failed").length,
        1,
        "a turn_failed cut off is not sent again",
    );
    const harness = await again.harness();
    const root = await harness.root(context);
    const input = await harness.commit(
        (tx) => tx.submissionByRequest(root.id, "input-1"),
        context,
    );
    assert.equal(input?.status, "unanswered");
    await again.stop();
});

test("a refused automatic compaction fails the turn with the realm's words", async (t) => {
    let asked = 0;
    const { calls, sent } = fakeCalls({
        // The fourth is the summary the fourth line's turn compacts with.
        realm: { may_spend: () => (++asked >= 4 ? refused : { type: "done" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: script([
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Two."),
            fauxAssistantMessage("Three."),
        ]),
        // Blocks at about 35,000 tokens of the faux model's 128,000: past
        // three lines, short of four.
        settings: {
            compaction: {
                reserveTokens: 93_000,
                backgroundTokens: 0,
                keepRecentTokens: 20_000,
            },
        },
    });

    for (const n of [1, 2, 3]) await say(agent, n, long(n));
    await say(agent, 4, long(4));
    await until(() => named(sent, "turn_failed").length > 0);

    assert.equal(asked, 4, "the fourth turn asked only for its summary");
    assert.deepEqual(
        (await notices(agent)).map((notice) => notice.text),
        [SPENT],
    );
    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [
            {
                task: "task-4",
                conversation: await rootRef(agent),
                words: SPENT,
                cause: "budget",
            },
        ],
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a turn with no model fails with the model it lacks", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        models: (models) =>
            new Proxy(models, {
                get(target, key, receiver) {
                    if (key === "getModel") return () => undefined;
                    const value: unknown = Reflect.get(target, key, receiver);
                    return typeof value === "function"
                        ? value.bind(target)
                        : value;
                },
            }),
    });

    await say(agent, 1, "Go.");
    await until(() => named(sent, "turn_failed").length > 0);

    // pi-durable's words for the run, never its bare reason.
    const words = /^Model faux\/.+ is not available$/;
    const [notice] = await notices(agent);
    assert.match(notice?.text ?? "", words);
    const [told] = named(sent, "turn_failed").map((call) => call.message);
    assert.match((told as { words: string }).words, words);
    assert.partialDeepStrictEqual(told, {
        task: "task-1",
        conversation: await rootRef(agent),
        cause: "model",
    });
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a stopped turn tells nothing", async (t) => {
    const asked = Promise.withResolvers<void>();
    const { calls, sent } = fakeCalls({
        workspace: { cancel: () => ({ commands: [] }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            (_request, options) => {
                asked.resolve();
                return new Promise((_resolve, reject) =>
                    options?.signal?.addEventListener("abort", () =>
                        reject(new Error("aborted")),
                    ),
                );
            },
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await asked.promise;

    await agent.actions.stop(agent.by(from.client), { id: "stop-1" });
    await agent.idle();

    assert.deepEqual(await notices(agent), []);
    assert.deepEqual(named(sent, "turn_failed"), []);
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("/retry carries on after an empty answer", async (t) => {
    const { calls } = fakeCalls({
        realm: { open_task: () => ({ type: "opened", task: "task-retry" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage(" "),
            fauxAssistantMessage("Carried."),
        ],
    });
    await say(agent, 1, "Go.");

    const answer = await agent.actions.retry(agent.by(from.client), {
        id: "retry-1",
        to: 7,
    });
    await agent.idle();

    assert.deepEqual(answer, { type: "done" });
    assert.ok(
        (await agent.entries()).some((entry) => entry.text === "Carried."),
    );
    await agent.stop();
});

test("an empty answer to a question sends no reply", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { reply: () => ({ type: "done" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("  ")],
    });

    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: {
            type: "question",
            letter: 1,
            from: { agent: 12, name: "Ada" },
            text: "How?",
        },
    });
    await agent.idle();

    assert.deepEqual(named(sent, "reply"), []);
    await agent.stop();
});

test("a refused summary in the cut-off rule fails with the realm's words", async (t) => {
    let asked = 0;
    const { calls, sent } = fakeCalls({
        // The fifth is the cut-off rule's summary.
        realm: { may_spend: () => (++asked >= 5 ? refused : { type: "done" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: script([
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Two."),
            fauxAssistantMessage("Three."),
            fauxAssistantMessage([], { stopReason: "length" }),
        ]),
    });

    for (const n of [1, 2, 3]) await say(agent, n, long(n));
    await say(agent, 4, "Line 4. Answer now.");
    await until(() => named(sent, "turn_failed").length > 0);

    assert.deepEqual(
        (await notices(agent)).map((notice) => notice.text),
        [SPENT],
    );
    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [
            {
                task: "task-4",
                conversation: await rootRef(agent),
                words: SPENT,
                cause: "budget",
            },
        ],
    );
    await agent.stop();
});

test("a close while the realm is told of a failed turn reports nothing", async (t) => {
    const told = Promise.withResolvers<void>();
    const { calls } = fakeCalls({
        realm: {
            turn_failed: () => {
                told.resolve();
                return new Promise<never>(() => {});
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage(" ")],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await told.promise;

    await agent.stop();
    // The tell the close cut off has settled by the next wake's end.
    const again = await agent.again();
    await again.idle();

    assert.deepEqual(agent.reported, []);
    await again.stop();
});

for (const [what, given] of [
    ["a refusal", { may_spend: () => refused }],
    ["an empty answer", {}],
] as const) {
    test(`${what} does not wait for the realm to take turn_failed`, async (t) => {
        const { calls, sent } = fakeCalls({
            realm: {
                ...given,
                turn_failed: () => new Promise<never>(() => {}),
            },
        });
        const agent = await wakeAgent(t, {
            calls,
            responses: [fauxAssistantMessage(" ")],
        });
        await agent.actions.submit(agent.by(from.realm), {
            id: "input-1",
            task: "task-1",
            input: { type: "line", text: "Go." },
        });
        await until(() => named(sent, "turn_failed").length > 0);

        // An idle conversation is idle in milliseconds; 30 s is far past a busy machine.
        assert.equal(await within(agent.idle(), 30_000, "hung"), undefined);
        await agent.stop();
    });
}

test("a held call's item is the realm's words, never tried again", async (t) => {
    // `503` is a status code pi-ai tries again.
    const hold = {
        type: "refused",
        why: { type: "objection", task: "503" },
        words: "task 503 is held on an objection until a chief says go or withdraw",
    } as const;
    const { calls, sent } = fakeCalls({ realm: { may_spend: () => hold } });
    const agent = await wakeAgent(t, {
        calls,
        settings: { retry: { enabled: true, maxRetries: 2, baseDelayMs: 1 } },
    });

    await say(agent, 1, "Go.");

    assert.equal(named(sent, "may_spend").length, 1);
    const harness = await agent.harness();
    const root = await harness.root(context);
    const page = await root.entries(
        { order: "ascending" },
        50,
        undefined,
        context,
    );
    const items = page.items
        .filter((entry) => entry.kind === "pi.assistant")
        .map(
            (entry) =>
                (entry.model?.[0] as AssistantMessage | undefined)
                    ?.errorMessage,
        );
    assert.deepEqual(items, [hold.words]);
    assert.deepEqual(named(sent, "turn_failed"), []);
    await agent.stop();
});

test("a provider error on a harness opened again tells the realm", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage([], {
                stopReason: "error",
                errorMessage: "invalid x-api-key",
            }),
        ],
    });
    const first = await agent.harness();
    const failed = agent.failNextCommit();
    await assert.rejects(say(agent, 0, "Lost."));
    await failed;
    await until(async () => (await agent.harness()) !== first);

    await say(agent, 1, "Go.");
    await until(() => named(sent, "turn_failed").length > 0);

    assert.deepEqual(
        named(sent, "turn_failed").map((call) => call.message),
        [
            {
                task: "task-1",
                conversation: await rootRef(agent),
                words: "invalid x-api-key",
                cause: "model",
            },
        ],
    );
    await agent.stop();
});

test("a realm that cannot be asked fails the turn as the model's", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            may_spend: () => {
                throw new Error("gone");
            },
        },
    });
    const agent = await wakeAgent(t, { calls });

    await say(agent, 1, "Go.");
    await until(() => named(sent, "turn_failed").length > 0);

    const [failed] = named(sent, "turn_failed").map(
        (call) => call.message as TurnFailed,
    );
    assert.equal(failed?.cause, "model");
    assert.match(failed?.words ?? "", /: it could not be asked \(gone\)$/);
    await agent.stop();
});

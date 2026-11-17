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
    // Before the close was told apart, its noise was reported by here.
    await new Promise((resolve) => setImmediate(resolve));
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

/** Answers a summarising request with a summary, every other with the next of `answers`. */
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

test("a repeat after a kill writes one notice", async (t) => {
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
    await agent.stop();

    const again = await agent.again();
    await again.idle();

    assert.equal(
        named(sent, "may_spend").length,
        2,
        "the wake ran the refused request again",
    );
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
    assert.equal(input?.status, "unanswered", "the repeat ended the turn");
    await again.stop();
});

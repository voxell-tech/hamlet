import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
} from "@earendil-works/pi-ai";
import type { RealmActions } from "../src/agent/messages.ts";
import {
    fakeCalls,
    from,
    named,
    type Sent,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

// `crates/hamlet_server/src/talk/reflections.rs:18-24`.
const REFLECT =
    "Reflection time (automatic, not from a chief). Look back over this session. Keep what is worth remembering about the chiefs and the lessons learned in your memory/ folder, and save or improve a skill in skills/ for anything you did that is likely to come up again. If nothing is worth keeping, say so in one line.";

type OpenTask = RealmActions["open_task"];

/** Each reflection's task is the realm's own (H12), numbered as it opens. */
function opening(): OpenTask {
    let n = 0;
    return () => {
        n += 1;
        return { type: "opened", task: `reflection-task-${n}` };
    };
}

/** `HAMLET_REFLECT_EVERY` is read at each wake, so it holds to the test's end. */
async function wake(
    t: TestContext,
    every: string,
    responses: FauxResponseStep[],
    openTask: (
        message: Parameters<OpenTask>[0],
    ) => ReturnType<OpenTask> | Promise<ReturnType<OpenTask>> = opening(),
): Promise<{ agent: Woken; sent: Sent[] }> {
    const before = process.env.HAMLET_REFLECT_EVERY;
    process.env.HAMLET_REFLECT_EVERY = every;
    t.after(() => {
        if (before === undefined) delete process.env.HAMLET_REFLECT_EVERY;
        else process.env.HAMLET_REFLECT_EVERY = before;
    });
    const { calls, sent } = fakeCalls({ realm: { open_task: openTask } });
    const agent = await wakeAgent(t, { calls, responses });
    t.after(() => agent.stop());
    return { agent, sent };
}

function say(agent: Woken) {
    let n = 0;
    return async (text: string) => {
        n += 1;
        await agent.actions.submit(agent.by(from.realm), {
            id: `line-${n}`,
            input: { type: "line", text },
        });
        await agent.idle();
    };
}

function send(agent: Woken, id: string, text: string): void {
    void agent.actions
        .submit(agent.by(from.realm), { id, input: { type: "line", text } })
        .catch(() => {});
}

const words = (text: string) => fauxAssistantMessage(text);

/** The call a kill cut off and its rerun ask under one request. */
function assertOneRequest(sent: readonly Sent[]): void {
    const requests = named(sent, "open_task").map(
        (call) => (call.message as { request: string }).request,
    );
    assert.equal(requests.length, 2);
    assert.equal(requests[0], requests[1]);
}

function held(text: string): {
    step: FauxResponseStep;
    asked: Promise<void>;
    release(): void;
} {
    const asked = Promise.withResolvers<void>();
    const released = Promise.withResolvers<void>();
    return {
        step: async () => {
            asked.resolve();
            await released.promise;
            return words(text);
        },
        asked: asked.promise,
        release: released.resolve,
    };
}

async function said(agent: Woken): Promise<string[]> {
    return (await agent.entries())
        .filter((entry) => entry.text !== "")
        .map((entry) => entry.text);
}

async function reflections(
    agent: Woken,
): Promise<{ text: string; data: unknown }[]> {
    return (await agent.entries())
        .filter((entry) => entry.kind === "hamlet.reflection")
        .map(({ text, data }) => ({ text, data }));
}

// `crates/hamlet_acceptance/tests/chats.rs:176`,
// `the_hand_reflects_every_few_answered_turns`. The item a client shows
// for the reflection's entry, "reflection", is `transcript.ts`'s.
test("A5: the Hand reflects every few answered turns", async (t) => {
    const { agent, sent } = await wake(t, "2", [
        words("A."),
        words("B."),
        words("Remembered."),
        words("C."),
    ]);
    const line = say(agent);
    await line("one");
    assert.deepEqual(await said(agent), ["one", "A."]);
    await line("two");
    assert.deepEqual(await said(agent), [
        "one",
        "A.",
        "two",
        "B.",
        REFLECT,
        "Remembered.",
    ]);
    assert.deepEqual(await reflections(agent), [
        { text: REFLECT, data: { task: "reflection-task-1" } },
    ]);
    await line("three");
    assert.deepEqual((await said(agent)).slice(-2), ["three", "C."]);
    assert.equal((await reflections(agent)).length, 1);
    assert.deepEqual(
        named(sent, "open_task").map(
            (call) => (call.message as Parameters<OpenTask>[0]).turn,
        ),
        ["reflection"],
    );
});

test("reflection at zero only when asked", async (t) => {
    const { agent, sent } = await wake(t, "0", [
        words("A."),
        words("B."),
        words("C."),
        words("Kept it."),
    ]);
    const line = say(agent);
    await line("one");
    await line("two");
    await line("three");
    assert.deepEqual(await reflections(agent), []);
    assert.deepEqual(named(sent, "open_task"), []);

    assert.equal(await agent.actions.reflect(agent.by(from.client)), "asked");
    await agent.idle();
    assert.deepEqual((await said(agent)).slice(-2), [REFLECT, "Kept it."]);
    assert.deepEqual(await reflections(agent), [
        { text: REFLECT, data: { task: "reflection-task-1" } },
    ]);
});

test("a /new repeated after its reflection was asked reflects once", async (t) => {
    const reflecting = held("Kept it.");
    const { agent, sent } = await wake(t, "0", [
        words("One."),
        reflecting.step,
    ]);
    await say(agent)("a");
    const client = agent.by(from.client);
    const begin = () =>
        agent.actions.new_session(client, { id: "new-1", to: 7 });

    assert.deepEqual(await begin(), { number: 2 });
    await reflecting.asked;
    assert.deepEqual(await begin(), { number: 2 });
    reflecting.release();
    await agent.idle();
    assert.deepEqual(await begin(), { number: 2 });
    await agent.idle();

    assert.deepEqual(await said(agent), ["a", "One.", REFLECT, "Kept it."]);
    assert.equal((await reflections(agent)).length, 1);
    assert.equal(named(sent, "open_task").length, 1);
});

test("a reflection's repeat after a kill is admitted once, on the task its request opened", async (t) => {
    let hangs = true;
    const { agent, sent } = await wake(
        t,
        "1",
        [words("One."), words("Kept it.")],
        () =>
            hangs
                ? new Promise<never>(() => {})
                : { type: "opened", task: "reflection-task-2" },
    );
    void say(agent)("a").catch(() => {});
    await until(() => named(sent, "open_task").length === 1);
    await agent.stop();
    hangs = false;

    const back = await agent.again();
    t.after(() => back.stop());
    await back.idle();
    assert.deepEqual(await said(back), ["a", "One.", REFLECT, "Kept it."]);
    assert.deepEqual(await reflections(back), [
        { text: REFLECT, data: { task: "reflection-task-2" } },
    ]);
    assertOneRequest(sent);
});

test("/reflect answers already while one is owed", async (t) => {
    const busy = held("One.");
    const { agent, sent } = await wake(t, "0", [
        busy.step,
        words("Kept it."),
        words("Kept more."),
    ]);
    const client = agent.by(from.client);
    send(agent, "line-1", "a");
    await busy.asked;

    assert.equal(await agent.actions.reflect(client), "asked");
    assert.equal(await agent.actions.reflect(client), "already");
    busy.release();
    await agent.idle();
    assert.deepEqual(await said(agent), ["a", "One.", REFLECT, "Kept it."]);

    assert.equal(await agent.actions.reflect(client), "asked");
    await agent.idle();
    assert.deepEqual(await reflections(agent), [
        { text: REFLECT, data: { task: "reflection-task-1" } },
        { text: REFLECT, data: { task: "reflection-task-2" } },
    ]);
    assert.equal(named(sent, "open_task").length, 2);
});

// `crates/hamlet_server/src/talk/chats.rs`.
const CARRY_ON = "Carry on from where you stopped.";

function aborted(): { step: FauxResponseStep; asked: Promise<void> } {
    const asked = Promise.withResolvers<void>();
    return {
        step: (_request, options) => {
            asked.resolve();
            return new Promise<never>((_, reject) =>
                options?.signal?.addEventListener("abort", () =>
                    reject(new Error("aborted")),
                ),
            );
        },
        asked: asked.promise,
    };
}

test("a short cut-off's second try answers its turn, and the reflection follows it", async (t) => {
    // Lines long enough that the compaction the cut-off asks has to summarise.
    const long = (n: number) => `Line ${n}. ${"word ".repeat(7_000)}`;
    const { agent } = await wake(t, "4", [
        words("One."),
        words("Two."),
        words("Three."),
        () => fauxAssistantMessage([], { stopReason: "length" }),
        words("Summary."),
        words("Four."),
        words("Kept."),
    ]);
    const line = say(agent);
    for (const n of [1, 2, 3]) await line(long(n));
    await line("Line 4. Answer now.");
    assert.deepEqual((await said(agent)).slice(-4), [
        CARRY_ON,
        "Four.",
        REFLECT,
        "Kept.",
    ]);
    assert.equal((await reflections(agent)).length, 1);
});

test("a /reflect while a cut-off waits for its summary is asked after the turn carries on", async (t) => {
    const long = (n: number) => `Line ${n}. ${"word ".repeat(7_000)}`;
    const summary = held("Summary.");
    const { agent } = await wake(t, "0", [
        words("One."),
        words("Two."),
        words("Three."),
        () => fauxAssistantMessage([], { stopReason: "length" }),
        summary.step,
        words("Four."),
        words("Kept."),
    ]);
    const line = say(agent);
    for (const n of [1, 2, 3]) await line(long(n));
    send(agent, "line-4", "Line 4. Answer now.");
    await summary.asked;

    assert.equal(await agent.actions.reflect(agent.by(from.client)), "asked");
    summary.release();
    await until(async () => (await said(agent)).at(-1) === "Kept.");

    assert.deepEqual((await said(agent)).slice(-4), [
        CARRY_ON,
        "Four.",
        REFLECT,
        "Kept.",
    ]);
});

test("a /reflect as the turn that owes a reflection ends asks one reflection on one task", async (t) => {
    const busy = held("One.");
    const { agent, sent } = await wake(t, "1", [busy.step, words("Kept.")]);
    send(agent, "line-1", "a");
    await busy.asked;

    busy.release();
    const asked = await agent.actions.reflect(agent.by(from.client));
    await agent.idle();

    assert.ok(asked === "asked" || asked === "already");
    assert.deepEqual(await said(agent), ["a", "One.", REFLECT, "Kept."]);
    assert.equal(named(sent, "open_task").length, 1);
});

test("an empty answer fails its turn, which is not counted", async (t) => {
    const { agent } = await wake(t, "1", [
        () => fauxAssistantMessage([]),
        words("B."),
        words("Kept."),
    ]);
    const line = say(agent);
    await line("a");
    assert.deepEqual(await reflections(agent), []);
    await line("b");
    assert.deepEqual((await said(agent)).slice(-3), ["B.", REFLECT, "Kept."]);
});

test("leaving a session whose only turn was aborted asks no reflection", async (t) => {
    const stopped = aborted();
    const { agent, sent: calls } = await wake(t, "0", [
        stopped.step,
        words("Kept."),
    ]);
    send(agent, "line-1", "a");
    await stopped.asked;
    const harness = await agent.harness();
    await (await harness.root(context)).abort(context);
    await agent.idle();

    await agent.actions.new_session(agent.by(from.client), {
        id: "new-1",
        to: 7,
    });
    await agent.idle();
    assert.deepEqual(await reflections(agent), []);
    assert.deepEqual(named(calls, "open_task"), []);
});

// As `crates/hamlet_server/src/talk/reflections.rs:42-58` asks a session
// put away while its turn was ending.
test("a /new mid-turn at zero reflects once, after the answer", async (t) => {
    const busy = held("One.");
    const { agent } = await wake(t, "0", [busy.step, words("Kept.")]);
    send(agent, "line-1", "a");
    await busy.asked;
    await agent.actions.new_session(agent.by(from.client), {
        id: "new-1",
        to: 7,
    });
    busy.release();
    await agent.idle();
    assert.deepEqual(await said(agent), ["a", "One.", REFLECT, "Kept."]);
    assert.equal((await reflections(agent)).length, 1);
});

test("a /new during a turn that then fails asks no reflection", async (t) => {
    const busy = held("");
    const { agent, sent } = await wake(t, "0", [busy.step, words("Kept.")]);
    send(agent, "line-1", "a");
    await busy.asked;
    await agent.actions.new_session(agent.by(from.client), {
        id: "new-1",
        to: 7,
    });
    busy.release();
    await agent.idle();

    assert.deepEqual(await reflections(agent), []);
    assert.deepEqual(named(sent, "open_task"), []);
});

test("a /new repeated after a kill cut off its reflection's task asks the realm again under the same request", async (t) => {
    let hangs = true;
    const { agent, sent: calls } = await wake(
        t,
        "0",
        [words("One."), words("Kept.")],
        () =>
            hangs
                ? new Promise<never>(() => {})
                : { type: "opened", task: "reflection-task-2" },
    );
    await say(agent)("a");
    const begin = (woken: Woken) =>
        woken.actions.new_session(woken.by(from.client), {
            id: "new-1",
            to: 7,
        });
    void begin(agent).catch(() => {});
    await until(() => named(calls, "open_task").length === 1);
    await agent.stop();
    hangs = false;

    const back = await agent.again();
    t.after(() => back.stop());
    assert.deepEqual(await begin(back), { number: 2 });
    await back.idle();
    assert.deepEqual(await said(back), ["a", "One.", REFLECT, "Kept."]);
    assert.deepEqual(await reflections(back), [
        { text: REFLECT, data: { task: "reflection-task-2" } },
    ]);
    assertOneRequest(calls);
});

test("HAMLET_REFLECT_EVERY reads a leading plus, as Rust's parse does", async (t) => {
    const { agent } = await wake(t, "+2", [
        words("A."),
        words("B."),
        words("Kept."),
    ]);
    const line = say(agent);
    await line("one");
    await line("two");
    assert.deepEqual((await said(agent)).slice(-2), [REFLECT, "Kept."]);
});

function lastUserText(messages: readonly Message[]): string | undefined {
    const last = messages.findLast((message) => message.role === "user");
    if (last === undefined || last.role !== "user") return undefined;
    return typeof last.content === "string"
        ? last.content
        : last.content
              .flatMap((part) => (part.type === "text" ? [part.text] : []))
              .join("");
}

for (const [owed, every] of [
    ["every turn", "1"],
    ["by /reflect", "0"],
] as const) {
    test(`a line sent as a turn ends is answered in a run of its own, before the reflection owed ${owed}`, async (t) => {
        const first = held("First.");
        const asked: (readonly Message[])[] = [];
        const recorded =
            (text: string): FauxResponseStep =>
            (request) => {
                asked.push(request.messages);
                return words(text);
            };
        const { agent } = await wake(t, every, [
            first.step,
            recorded("Second."),
            recorded("Kept."),
        ]);
        send(agent, "line-1", "a");
        await first.asked;
        if (every === "0") {
            assert.equal(
                await agent.actions.reflect(agent.by(from.client)),
                "asked",
            );
        }
        await agent.actions.submit(agent.by(from.realm), {
            id: "line-2",
            input: { type: "line", text: "b" },
        });
        first.release();
        await until(async () => {
            const shown = await said(agent);
            return shown.includes(REFLECT) && shown.at(-1) !== REFLECT;
        });

        assert.deepEqual(asked.map(lastUserText), ["b", REFLECT]);
        assert.deepEqual(await said(agent), [
            "a",
            "First.",
            "b",
            "Second.",
            REFLECT,
            "Kept.",
        ]);
    });
}

test("a stop drops the reflection that waits for the run", async (t) => {
    const busy = aborted();
    const { agent } = await wake(t, "0", [busy.step, words("Kept.")]);
    send(agent, "line-1", "a");
    await busy.asked;
    const client = agent.by(from.client);
    assert.equal(await agent.actions.reflect(client), "asked");

    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 1,
    });
    await agent.idle();
    assert.deepEqual(await reflections(agent), []);
    assert.equal(agent.faux.state.callCount, 1);
    assert.equal(await agent.actions.reflect(client), "asked");
    await agent.idle();
    assert.deepEqual((await said(agent)).slice(-2), [REFLECT, "Kept."]);
});

import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    createRegistry,
    defineExtension,
    type Extension,
    LiveDoc,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";
import type { RawAccess } from "../src/actor/db/mod.ts";
import { timers, Wake } from "../src/agent/timers.ts";
import {
    closeHarness,
    createPiRuntime,
    openOnWake,
    PI_RUNTIME,
    type PiContext,
    resumeDueSleeps,
} from "../src/pi/runtime.ts";
import { migratePiTables } from "../src/pi/storage.ts";
import {
    fakeCalls,
    from,
    toolCall,
    until,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import { context, fauxModels, plainDatabase, scratchFile } from "./testing.ts";

test("a timer arms one alarm, and its input is taken once when pi.wake fires", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            // Late enough that the turn ends first, so only the alarm runs it.
            toolCall("wake", { in_seconds: 1.5, note: "look at the build" }),
            fauxAssistantMessage("I set a timer."),
            fauxAssistantMessage("The build is green."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: { type: "line", text: "Check the build later." },
    });
    await agent.idle();
    // Idle with a sleep pending, the wrapper asks the host for one alarm.
    await until(() => agent.alarms.length > 0);
    assert.equal(agent.alarms.length, 1);
    const [at] = agent.alarms;
    assert.ok(at);

    await new Promise((resolve) =>
        setTimeout(resolve, Math.max(0, at - Date.now()) + 20),
    );
    assert.equal(await agent.actions.pi.wake(agent.by(from.alarm)), null);
    await until(
        async () =>
            (await agent.entries()).filter(
                (entry) => entry.kind === "hamlet.wake",
            ).length > 0,
    );
    await agent.idle();
    assert.equal(await agent.actions.pi.wake(agent.by(from.alarm)), null);
    await agent.idle();

    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "hamlet.wake")
            .map((entry) => entry.text),
        ["Your timer fired: look at the build"],
    );
    assert.equal(agent.faux.state.callCount, 3);
    await agent.stop();
});

test("a timer with no date sets nothing and arms no alarm, and the model is told so", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("wake", { in_seconds: 1e13, note: "later" }, "c1"),
            toolCall("wake", { in_seconds: -1e13, note: "earlier" }, "c2"),
            fauxAssistantMessage("No timer."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: { type: "line", text: "Wake me much later." },
    });
    await agent.idle();

    const harness = await agent.harness();
    const root = await harness.root(context);
    const set = await harness.commit(
        async (tx) =>
            (
                await tx.scanTasks(
                    { conversationId: root.id, kind: "hamlet.wake" },
                    100,
                )
            ).items.length,
        context,
    );
    const results = (await agent.entries())
        .filter((entry) => entry.kind === "pi.tool-result")
        .map((entry) => entry.text);
    await agent.stop();

    assert.equal(set, 0);
    assert.deepEqual(agent.alarms, []);
    assert.deepEqual(
        results,
        Array(2).fill(
            "`in_seconds` is out of range: no time that far off has a date. No timer is set.",
        ),
    );
});

/**
 * Stands in for the host of one `pi()` actor on `database`, and returns its
 * next generation. As the host's, an alarm outlives the generation that set
 * it, and calls `pi.wake` on the open one only once the wall clock has
 * reached its time (`hamlet_actor_core`'s schedule takes only what is due).
 */
function host(database: RawAccess): () => PiContext {
    let open: PiContext | undefined;
    const alarm = (at: number) => {
        const left = at - Date.now();
        // The host keeps an alarm in its database, so it holds no process open.
        if (left > 0) setTimeout(alarm, left, at).unref();
        else if (open !== undefined) void resumeDueSleeps(open);
    };
    return () => {
        open = {
            actorId: "agent-1",
            db: database,
            vars: { [PI_RUNTIME]: createPiRuntime() },
            conns: new Map(),
            log: { warn: () => {}, error: () => {} },
            keepAwake: (promise: Promise<unknown>) => promise,
            schedule: { at: async (at: number) => alarm(at) },
            abortSignal: new AbortController().signal,
        } as unknown as PiContext;
        return open;
    };
}

/**
 * Two generations of a `pi()` actor on one file. The first runs `Wake me.`
 * until `cut` resolves and is closed there; the second resumes and runs until
 * no timer noted `check` is pending and no run is live. Returns what the
 * second finds: the states of those timers and the inputs they admitted.
 */
async function cutOffAndResumed(
    t: TestContext,
    given: {
        extension: Extension;
        cut: Promise<void>;
        seed?: (tx: Tx, conversation: ConversationId) => Promise<void>;
    },
): Promise<{ set: string[]; inputs: number }> {
    const database = plainDatabase(scratchFile(t));
    await migratePiTables(database);
    const registry = createRegistry();
    registry.install(given.extension);
    const { faux, models, model } = fauxModels();
    faux.setResponses([
        toolCall("wake", { in_seconds: 1.5, note: "check" }),
        fauxAssistantMessage("set"),
        fauxAssistantMessage("woke"),
        fauxAssistantMessage("woke again"),
    ]);
    const options = { registry, models, model };
    const generation = host(database);

    const first = generation();
    const firstHarness = await openOnWake(first, options);
    firstHarness.resume();
    const root = await firstHarness.root(context);
    const seed = given.seed;
    if (seed !== undefined)
        await firstHarness.commit((tx) => seed(tx, root.id), context);
    await root.submit(
        { type: "input", content: "Wake me.", requestId: "r1" },
        context,
    );
    assert.equal(
        await within(
            given.cut.then(() => "cut off"),
            10_000,
            "never cut off",
        ),
        "cut off",
    );
    await closeHarness(first, { reason: "destroy" });

    const second = generation();
    const secondHarness = await openOnWake(second, options);
    secondHarness.resume();
    const root2 = await secondHarness.root(context);
    const timersSet = () =>
        secondHarness.commit(
            async (tx) =>
                (
                    await tx.scanTasks(
                        { conversationId: root2.id, kind: "hamlet.wake" },
                        2000,
                    )
                ).items
                    .filter(
                        (task) =>
                            (task.input as { note: string }).note === "check",
                    )
                    .map((task) => task.state.status),
            context,
        );
    const admitted = async () =>
        (await root2.entries({}, 200, undefined, context)).items.filter(
            (entry) => entry.kind === "hamlet.wake",
        ).length;
    await until(
        async () =>
            (await timersSet()).every((status) => status === "terminal") &&
            (await admitted()) > 0 &&
            (await secondHarness.snapshot(LiveDoc, root2.id, context))?.run ===
                undefined,
        // A thousand seeded timers are scanned on each look.
        20_000,
    );
    const set = await timersSet();
    const inputs = await admitted();
    await closeHarness(second, { reason: "destroy" });
    return { set, inputs };
}

/** A `wake` that runs whole, then hangs on its first run until the close cuts it off. */
function hangingOnce(): {
    extension: Extension;
    cut: Promise<void>;
    runs: () => number;
} {
    let runs = 0;
    const cut = Promise.withResolvers<void>();
    const real = timers();
    const wake = real.tools?.find((tool) => tool.name === "wake");
    assert.ok(wake, "timers() offers wake");
    const hanging: ToolRegistration = {
        ...wake,
        execute: async (args, api, toolContext) => {
            runs += 1;
            const result = await wake.execute(args, api, toolContext);
            if (runs > 1) return result;
            cut.resolve();
            return untilAborted(toolContext.abortSignal);
        },
    };
    return {
        extension: defineExtension({ ...real, tools: [hanging] }),
        cut: cut.promise,
        runs: () => runs,
    };
}

function untilAborted(signal: AbortSignal | undefined): Promise<never> {
    return new Promise((_, reject) =>
        signal?.addEventListener("abort", () => reject(new Error("cut off"))),
    );
}

test("a timer whose in-process wake comes early is admitted by the host's alarm", {
    timeout: 30_000,
}, async (t) => {
    // Node can fire a timer before `Date.now()` reaches its time; here the
    // runtime's in-process wake, `resumeDue`, fires 50 ms early.
    const onTime = globalThis.setTimeout;
    const early: number[] = [];
    t.mock.method(
        globalThis,
        "setTimeout",
        (callback: (...args: unknown[]) => void, ms = 0, ...args: unknown[]) =>
            callback.name === "resumeDue"
                ? onTime(
                      (...given: unknown[]) => {
                          early.push(Date.now());
                          callback(...given);
                      },
                      ms - 50,
                      ...args,
                  )
                : onTime(callback, ms, ...args),
    );
    const database = plainDatabase(scratchFile(t));
    await migratePiTables(database);
    const registry = createRegistry();
    registry.install(timers());
    const { faux, models, model } = fauxModels();
    faux.setResponses([
        toolCall("wake", { in_seconds: 1.5, note: "check" }),
        fauxAssistantMessage("set"),
        fauxAssistantMessage("woke"),
    ]);
    const c = host(database)();
    const harness = await openOnWake(c, { registry, models, model });
    harness.resume();
    const root = await harness.root(context);
    await root.submit(
        { type: "input", content: "Wake me.", requestId: "r1" },
        context,
    );
    const admitted = async () =>
        (await root.entries({}, 200, undefined, context)).items.filter(
            (entry) => entry.kind === "hamlet.wake",
        );
    const timersSet = () =>
        harness.commit(
            async (tx) =>
                (
                    await tx.scanTasks(
                        { conversationId: root.id, kind: "hamlet.wake" },
                        10,
                    )
                ).items,
            context,
        );
    await until(
        async () =>
            (await admitted()).length > 0 &&
            (await timersSet()).every(
                (task) => task.state.status === "terminal",
            ) &&
            (await harness.snapshot(LiveDoc, root.id, context))?.run ===
                undefined,
        10_000,
    );
    const [timer] = await timersSet();
    const inputs = await admitted();
    await closeHarness(c, { reason: "destroy" });

    assert.ok(timer, "the wake set one timer");
    const { at } = timer.input as { at: number };
    assert.ok(
        early.some((fired) => fired < at),
        "an in-process wake came before the timer's time",
    );
    assert.equal(inputs.length, 1, "the timer admitted one input");
    const timestamp = inputs[0]?.model?.[0]?.timestamp ?? 0;
    assert.ok(timestamp >= at, "the input came at or after the timer's time");
});

test("a wake cut off after it set its timer sets no second one on its rerun, and the timer admits once", {
    timeout: 30_000,
}, async (t) => {
    const hanging = hangingOnce();
    const { set, inputs } = await cutOffAndResumed(t, hanging);
    assert.equal(hanging.runs(), 2);
    assert.deepEqual(set, ["terminal"]);
    assert.equal(inputs, 1);
});

test("a wake's rerun finds its timer behind a thousand older ones", {
    timeout: 60_000,
}, async (t) => {
    const hanging = hangingOnce();
    const later = Date.now() + 3_600_000;
    const { set, inputs } = await cutOffAndResumed(t, {
        ...hanging,
        seed: async (tx, conversationId) => {
            for (let n = 0; n < 1000; n += 1) {
                await tx.createTask(
                    Wake,
                    { call: `older-${n}`, at: later, note: "older" },
                    {
                        ownership: { kind: "conversation" },
                        conversationId,
                        background: true,
                    },
                );
            }
        },
    });
    assert.equal(hanging.runs(), 2);
    assert.deepEqual(set, ["terminal"]);
    assert.equal(inputs, 1);
});

test("a timer admits its input and ends in one commit, so a kill between the two cannot admit it twice", {
    timeout: 30_000,
}, async (t) => {
    const real = timers();
    const cut = Promise.withResolvers<void>();
    let commits = 0;
    // The admit phase's first commit lands, then the phase hangs as a kill
    // right after it would leave it.
    const killedAfterOneCommit: typeof Wake.definition.phases.admit = (
        task,
        runtime,
        phaseContext,
    ) =>
        Wake.definition.phases.admit(
            task,
            new Proxy(runtime, {
                get(target, key, receiver) {
                    if (key !== "commit")
                        return Reflect.get(target, key, receiver);
                    return async (
                        ...args: Parameters<typeof target.commit>
                    ) => {
                        const done = await target.commit(...args);
                        commits += 1;
                        if (commits > 1) return done;
                        cut.resolve();
                        return untilAborted(phaseContext.abortSignal);
                    };
                },
            }),
            phaseContext,
        );
    const extension = defineExtension({
        ...real,
        tasks: [
            {
                definition: {
                    ...Wake.definition,
                    phases: {
                        ...Wake.definition.phases,
                        admit: killedAfterOneCommit,
                    },
                },
            },
        ],
    });
    const { set, inputs } = await cutOffAndResumed(t, {
        extension,
        cut: cut.promise,
    });
    assert.deepEqual(set, ["terminal"]);
    assert.equal(inputs, 1);
});

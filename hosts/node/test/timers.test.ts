/**
 * U11: `wake`, the agent's timer.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import {
    createRegistry,
    defineExtension,
    LiveDoc,
    type ToolRegistration,
} from "@earendil-works/pi-durable";
import type { RawAccess } from "../src/actor/db/mod.ts";
import { timers } from "../src/agent/timers.ts";
import {
    closeHarness,
    createPiRuntime,
    openOnWake,
    PI_RUNTIME,
    type PiContext,
} from "../src/pi/runtime.ts";
import { migratePiTables } from "../src/pi/storage.ts";
import {
    fakeCalls,
    from,
    toolCall,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import { context, fauxModels, plainDatabase, scratchFile } from "./testing.ts";

test("a timer arms one alarm, and its input is taken once when pi.wake fires", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("wake", { in_seconds: 0.3, note: "look at the build" }),
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
    // A second fire of the alarm finds nothing due.
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

/** One generation of a `pi()` actor over a plain SQLite file, with no host. */
function generation(database: RawAccess): PiContext {
    return {
        actorId: "agent-1",
        db: database,
        vars: { [PI_RUNTIME]: createPiRuntime() },
        conns: new Map(),
        log: { warn: () => {}, error: () => {} },
        keepAwake: (promise: Promise<unknown>) => promise,
        schedule: { at: async () => {} },
        abortSignal: new AbortController().signal,
    } as unknown as PiContext;
}

test("a wake cut off after it set its timer sets no second one on its rerun, and the timer admits once", {
    timeout: 30_000,
}, async (t) => {
    const database = plainDatabase(scratchFile(t));
    await migratePiTables(database);
    const real = timers();
    const wake = real.tools?.find((tool) => tool.name === "wake");
    assert.ok(wake, "timers() offers wake");
    let runs = 0;
    const cutOff = Promise.withResolvers<void>();
    // The real `wake` runs whole, then its first run hangs until the close cuts it off.
    const hanging: ToolRegistration = {
        ...wake,
        execute: async (args, api, toolContext) => {
            runs += 1;
            const result = await wake.execute(args, api, toolContext);
            if (runs > 1) return result;
            cutOff.resolve();
            return new Promise((_, reject) =>
                toolContext.abortSignal?.addEventListener("abort", () =>
                    reject(new Error("cut off")),
                ),
            );
        },
    };
    const registry = createRegistry();
    registry.install(defineExtension({ ...real, tools: [hanging] }));
    const { faux, models, model } = fauxModels();
    faux.setResponses([
        toolCall("wake", { in_seconds: 1.5, note: "check" }),
        fauxAssistantMessage("set"),
        fauxAssistantMessage("woke"),
    ]);
    const options = { registry, models, model };

    const first = generation(database);
    const firstHarness = await openOnWake(first, options);
    firstHarness.resume();
    const root = await firstHarness.root(context);
    const submission = await root.submit(
        { type: "input", content: "Wake me.", requestId: "r1" },
        context,
    );
    await Promise.race([
        cutOff.promise,
        submission.wait(context).then((settled) => {
            throw new Error(
                `the run ended before wake was cut off: ${settled.status}`,
            );
        }),
    ]);
    await closeHarness(first, { reason: "destroy" });

    const second = generation(database);
    const secondHarness = await openOnWake(second, options);
    secondHarness.resume();
    const root2 = await secondHarness.root(context);
    const timersSet = () =>
        secondHarness.commit(
            async (tx) =>
                (
                    await tx.scanTasks(
                        { conversationId: root2.id, kind: "hamlet.wake" },
                        100,
                    )
                ).items.map((task) => task.state.status),
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
    );
    const set = await timersSet();
    const inputs = await admitted();
    await closeHarness(second, { reason: "destroy" });

    assert.equal(runs, 2);
    assert.deepEqual(set, ["terminal"]);
    assert.equal(inputs, 1);
});

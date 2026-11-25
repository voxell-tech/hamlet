import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { OnTask } from "../src/agent/messages/agent.ts";
import {
    fakeCalls,
    from,
    held,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { fixture } from "./testing.ts";

function onTask(
    agent: Woken,
    tasks: string[],
    within_ms: number,
): Promise<boolean> {
    return agent.actions.on_task(agent.by(from.realm), { tasks, within_ms });
}

test("an agent is on a task while an input under it runs or waits, and not on another", async (t) => {
    const slow = held();
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        responses: [
            slow.step,
            fauxAssistantMessage("Done."),
            fauxAssistantMessage("Seen."),
        ],
    });
    const realm = agent.by(from.realm);
    await agent.actions.submit(realm, {
        id: "line-1",
        task: "7",
        input: { type: "line", text: "Take your time" },
    });
    await slow.asked;
    await agent.actions.submit(realm, {
        id: "notice-1",
        task: "8",
        input: { type: "notice", text: "Later." },
    });

    const running = await onTask(agent, ["7"], 0);
    const queued = await onTask(agent, ["8"], 0);
    const other = await onTask(agent, ["9"], 60_000);
    slow.answer(fauxAssistantMessage("Slow."));
    await agent.idle();
    const after = await onTask(agent, ["7", "8"], 0);
    await agent.stop();

    assert.deepEqual(
        { running, queued, other, after },
        { running: true, queued: true, other: false, after: false },
    );
    assert.deepEqual(agent.reported, []);
});

test("an agent is on a task whose input ended within the time asked, also after a restart, and not once it has passed", async (t) => {
    t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
    const first = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        responses: [fauxAssistantMessage("Done.")],
    });
    await first.actions.submit(first.by(from.realm), {
        id: "line-1",
        task: "7",
        input: { type: "line", text: "Quick one" },
    });
    await first.idle();
    t.mock.timers.tick(600_000);
    const within = await onTask(first, ["7"], 900_000);
    const passed = await onTask(first, ["7"], 300_000);
    const noMinutes = await onTask(first, ["7"], 0);
    const agent = await first.killed();
    const restarted = [
        await onTask(agent, ["7"], 900_000),
        await onTask(agent, ["7"], 300_000),
    ];
    await agent.stop();

    assert.deepEqual(
        { within, passed, noMinutes, restarted },
        {
            within: true,
            passed: false,
            noMinutes: false,
            restarted: [true, false],
        },
    );
});

test("agent/on_task is read and answered as its answer fixture holds", async (t) => {
    const slow = held();
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        responses: [slow.step, fauxAssistantMessage("Done.")],
    });
    const [message] = fixture("agent/on_task.args") as [OnTask];
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-1",
        task: message.tasks[0] ?? "",
        input: { type: "line", text: "Take your time" },
    });
    await slow.asked;

    const answer = await agent.actions.on_task(agent.by(from.realm), message);
    slow.answer(fauxAssistantMessage("Slow."));
    await agent.idle();
    await agent.stop();

    assert.deepStrictEqual(answer, fixture("agent/on_task.answer"));
});

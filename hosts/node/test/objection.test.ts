import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    fauxAssistantMessage,
    fauxToolCall,
    type ToolCall,
} from "@earendil-works/pi-ai";
import type { AgentInput } from "../src/agent/messages/agent.ts";
import type { Done } from "../src/agent/messages/realm.ts";
import {
    fakeCalls,
    from,
    named,
    toolResults,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import { fixture } from "./testing.ts";

const OBJECTED =
    "Tool call blocked: The chief is considering an objection, so this call did not run. Make it again if the chief says to go on.";
const OBJECTING = "The chief is considering an objection.";

const object = (reason = "The file is better left alone.") =>
    fauxToolCall("object", { reason }, { id: "objects" });
const command = fauxToolCall(
    "bash",
    { command: "echo done > ruled" },
    { id: "runs" },
);
const write = fauxToolCall(
    "write",
    { path: "/work/ruled", content: "done" },
    { id: "writes" },
);

/** As `crates/hamlet_messages/src/realm/oversight.rs` words `ObjectRefusal`. */
const NO_REASON: Done = {
    type: "refused",
    why: { type: "no_reason" },
    words: "Say why you object.",
};
const NOT_A_CHIEFS_LINE: Done = {
    type: "refused",
    why: { type: "not_a_chiefs_line" },
    words: "Only a chief's instruction can be objected to, in the turn that works on it.",
};

async function answered(
    t: TestContext,
    calls: readonly ToolCall[],
    objected: Done,
    input?: AgentInput,
) {
    const { calls: fakes, sent } = fakeCalls({
        realm: { object: () => objected, worker_done: () => null },
    });
    const agent = await wakeAgent(t, {
        calls: fakes,
        responses: [
            fauxAssistantMessage([...calls], { stopReason: "toolUse" }),
            fauxAssistantMessage("Done."),
        ],
        ...(input === undefined ? {} : { input }),
    });
    t.after(() => agent.stop());
    if (input === undefined) {
        await agent.actions.submit(agent.by(from.realm), {
            id: "input-1",
            task: "task-1",
            input: { type: "line", text: "Make the file." },
        });
    }
    await until(async () => (await toolResults(agent)).length === calls.length);
    await agent.idle();
    const results = (await toolResults(agent)).map(
        ({ output, isError, diagnostics }) => ({
            text: output || diagnostics.map((d) => d.message).join(""),
            isError,
        }),
    );
    return { sent, results };
}

const held = { text: OBJECTED, isError: true };
const opened = { text: OBJECTING, isError: false };

test("an answer's command and file write after its object call do not run while the objection is open", async (t) => {
    const { sent, results } = await answered(t, [object(), command, write], {
        type: "done",
    });
    assert.equal(named(sent, "object").length, 1);
    assert.deepEqual(named(sent, "run"), []);
    assert.deepEqual(named(sent, "write_file"), []);
    assert.deepEqual(results, [opened, held, held]);
});

test("an answer's command before its object call runs", async (t) => {
    const { sent, results } = await answered(t, [command, object()], {
        type: "done",
    });
    assert.equal(named(sent, "run").length, 1);
    assert.notDeepEqual(results[0], held);
    assert.deepEqual(results[1], opened);
});

test("an answer's command after an object call the realm refuses runs", async (t) => {
    const { sent, results } = await answered(
        t,
        [object("  "), command],
        NO_REASON,
    );
    assert.equal(named(sent, "run").length, 1);
    assert.equal(results[0]?.text, NO_REASON.words);
    assert.notDeepEqual(results[1], held);
});

/** A worker briefed with `bash`, who is offered no `object` call. */
const WORKER: AgentInput = {
    ...(fixture("agent/worker.input") as AgentInput),
    brief: { text: "Make the file.", tools: [{ name: "bash" }] },
};

test("a worker's command beside an object call runs", async (t) => {
    const { sent, results } = await answered(
        t,
        [object(), command],
        NOT_A_CHIEFS_LINE,
        WORKER,
    );
    assert.equal(results[0]?.text, "Tool object is not available");
    assert.equal(named(sent, "run").length, 1);
    assert.notDeepEqual(results[1], held);
});

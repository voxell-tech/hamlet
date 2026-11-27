import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import {
    fakeCalls,
    from,
    named,
    toolResults,
    wakeAgent,
} from "./agent-testing.ts";

const OBJECTED =
    "Tool call blocked: The chief is considering an objection, so this call did not run. Make it again if the chief says to go on.";

const object = fauxToolCall(
    "object",
    { reason: "The file is better left alone." },
    { id: "objects" },
);
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

for (const [order, calls] of [
    ["after", [object, command, write]],
    ["before", [command, write, object]],
] as const) {
    test(`an answer's command and file write ${order} its object call do not run`, async (t) => {
        const { calls: fakes, sent } = fakeCalls({
            realm: { object: () => ({ type: "done" }) },
        });
        const agent = await wakeAgent(t, {
            calls: fakes,
            responses: [
                fauxAssistantMessage([...calls], { stopReason: "toolUse" }),
                fauxAssistantMessage("Objected."),
            ],
        });
        t.after(() => agent.stop());
        await agent.actions.submit(agent.by(from.realm), {
            id: "input-1",
            task: "task-1",
            input: { type: "line", text: "Make the file." },
        });
        await agent.idle();

        assert.equal(named(sent, "object").length, 1);
        assert.deepEqual(named(sent, "run"), []);
        assert.deepEqual(named(sent, "write_file"), []);
        const results = (await toolResults(agent)).map(
            ({ output, isError, diagnostics }) => ({
                text: output || diagnostics.map((d) => d.message).join(""),
                isError,
            }),
        );
        const held = { text: OBJECTED, isError: true };
        const objected = {
            text: "The chief is considering an objection.",
            isError: false,
        };
        assert.deepEqual(
            results,
            order === "after" ? [objected, held, held] : [held, held, objected],
        );
    });
}

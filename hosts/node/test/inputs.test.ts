import assert from "node:assert/strict";
import { test } from "node:test";
import { type Context, fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { Submit } from "../src/agent/messages/agent.ts";
import {
    asked,
    fakeCalls,
    from,
    named,
    toolCall,
    userWords,
    wakeAgent,
} from "./agent-testing.ts";
import { fixture } from "./testing.ts";

function submitted(name: string, id: string): Submit {
    const [message] = fixture(name) as [Submit];
    return { ...message, id };
}

test("a letter and a question reach the model as their bare words, and an answer as its author's, as today", async (t) => {
    const seen: Context[] = [];
    const { calls } = fakeCalls({
        realm: { reply: () => fixture("realm/reply.done.answer") as never },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            asked(seen, "Good."),
            asked(seen, "None."),
            asked(seen, "Thanks."),
        ],
    });
    for (const [name, id] of [
        ["agent/submit.letter.args", "input-1"],
        ["agent/submit.question.args", "input-2"],
        ["agent/submit.answer.args", "input-3"],
    ] as const) {
        await agent.actions.submit(agent.by(from.realm), submitted(name, id));
        await agent.idle();
    }
    await agent.stop();

    // As `crates/hamlet_server/src/talk/letters.rs` keeps a letter, a
    // question and an answer.
    assert.deepEqual(
        seen.map((request) => userWords(request).at(-1)),
        ["The tests pass", "Which tests fail?", "Ada answers: Two of them"],
    );
});

test("a worker's answer is told in the conversation that started the worker, by its name", async (t) => {
    const seen: Context[] = [];
    const { calls, sent } = fakeCalls({
        realm: {
            start_worker: () =>
                fixture("realm/start_worker.started.answer") as never,
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("start_worker", { task: "Check the links" }),
            fauxAssistantMessage("Started."),
            asked(seen, "Fixing them."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        submitted("agent/submit.letter.args", "input-1"),
    );
    await agent.idle();
    const [started] = named(sent, "start_worker");
    const conversation = (started?.message as { conversation?: string })
        ?.conversation;
    assert.ok(conversation);
    await agent.actions.submit(agent.by(from.realm), {
        id: "worker/15",
        input: {
            type: "worker_answer",
            from: { agent: 15, name: "worker#15" },
            text: "Three links are broken",
            conversation,
        },
    });
    await agent.idle();
    await agent.stop();

    const [request] = seen;
    assert.ok(request);
    const words = userWords(request);
    assert.deepEqual(
        [words[0], words.at(-1)],
        ["The tests pass", "worker#15 answers: Three links are broken"],
    );
});

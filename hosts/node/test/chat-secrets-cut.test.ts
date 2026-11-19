/**
 * The host loads the realm's secret list once in its process, so this
 * file's tests share one list.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import {
    dumped,
    fakeCalls,
    from,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

/** Made up; no provider has ever issued it. */
const KEY = "sk-made-up-cut-4b1d-93e7";

function assertNoStartOfKey(what: string, text: string): void {
    for (let length = 5; length <= KEY.length; length += 1) {
        const start = KEY.slice(0, length);
        assert.ok(!text.includes(start), `${what} holds ${start}`);
    }
}

async function read(agent: Woken): Promise<string> {
    return JSON.stringify(
        await agent.actions.read_chat(agent.by(from.client), {
            id: "00000000-0000-4000-8000-0000000000dd",
            chat: { with: 7 },
        }),
    );
}

test("a stopped answer never holds the start of a key", async (t) => {
    const { calls } = fakeCalls({
        realm: { secrets: () => ({ version: 1, values: [KEY] }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage(`My key is ${KEY.slice(0, 12)}`, {
                stopReason: "length",
            }),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-key",
        input: { type: "line", text: "Say your key." },
    });
    await agent.idle();

    const page = await read(agent);
    assert.match(page, /My key is/);
    assertNoStartOfKey("a client's read", page);
    assertNoStartOfKey("the database", await dumped(agent));
    assertNoStartOfKey("the events", JSON.stringify(agent.events));
    await agent.stop();
});

test("a call stopped mid-argument never holds the start of a key", async (t) => {
    const { calls } = fakeCalls({
        realm: { secrets: () => ({ version: 1, values: [KEY] }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage(
                [fauxToolCall("read", { path: `notes/${KEY.slice(0, 12)}` })],
                { stopReason: "length" },
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-key",
        input: { type: "line", text: "Read it." },
    });
    await agent.idle();

    const page = await read(agent);
    assert.match(page, /notes\//);
    assertNoStartOfKey("a client's read", page);
    assertNoStartOfKey("the database", await dumped(agent));
    assertNoStartOfKey("the events", JSON.stringify(agent.events));
    await agent.stop();
});

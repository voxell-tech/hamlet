/**
 * The host's scrub lives as long as its process, so the tests of this file
 * run in order on one host: the first is its first load.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { ChatPage } from "../src/agent/messages/agent.ts";
import type { SecretList } from "../src/agent/messages/realm.ts";
import {
    dumped,
    fakeCalls,
    from,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

/** Made up; no provider has ever issued it. */
const STORED = "sk-made-up-stored-5e1f-9b";

/** Made up, as `STORED`. */
const LATER = "sk-made-up-later-0d4c-2a";

async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `line-${text}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

function page(agent: Woken): Promise<ChatPage> {
    return agent.actions.read_chat(agent.by(from.client), {
        id: "00000000-0000-4000-8000-0000000000aa",
        chat: { with: 7 },
    });
}

test("a key stored before the host started is taken out of what a client reads", async (t) => {
    const { calls } = fakeCalls({
        realm: { secrets: () => ({ version: 1, values: [STORED] }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage(`The stored key is ${STORED}.`)],
    });

    await say(agent, "What key is stored?");

    const read = JSON.stringify(await page(agent));
    assert.match(read, /The stored key is \[REDACTED\]\./);
    for (const [what, seen] of Object.entries({
        "client's read": read,
        database: await dumped(agent),
        log: JSON.stringify(agent.reported),
        events: JSON.stringify(agent.events),
    })) {
        assert.ok(!seen.includes(STORED), `the ${what} holds the key`);
    }
    await agent.stop();
});

test("a key stored while the host runs is taken out of what a client reads after the next wake", async (t) => {
    let list: SecretList = { version: 1, values: [STORED] };
    const { calls } = fakeCalls({ realm: { secrets: () => list } });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage(`The new key is ${LATER}.`)],
    });
    await say(agent, "What key is new?");
    assert.ok(JSON.stringify(await page(agent)).includes(LATER));

    list = { version: 2, values: [STORED, LATER] };
    await agent.stop();
    const woken = await agent.again();

    await until(async () => !JSON.stringify(await page(woken)).includes(LATER));
    await woken.stop();
});

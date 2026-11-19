/**
 * The host loads the realm's secret list once in its process, so this file
 * holds the one test that sees the host before that load.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { ChatPage } from "../src/agent/messages/agent.ts";
import type { SecretList } from "../src/agent/messages/realm.ts";
import {
    fakeCalls,
    from,
    named,
    timedOut,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

/** Made up; no provider has ever issued it. */
const STORED = "sk-made-up-stored-3a7e-6c";

function page(agent: Woken): Promise<ChatPage> {
    return agent.actions.read_chat(agent.by(from.client), {
        id: "00000000-0000-4000-8000-0000000000bb",
        chat: { with: 7 },
    });
}

test("nothing reaches a client before the realm answers its secret list", async (t) => {
    const late = Promise.withResolvers<SecretList>();
    const answers: (() => SecretList | Promise<SecretList>)[] = [
        () => {
            throw timedOut();
        },
        () => {
            throw timedOut();
        },
    ];
    const { calls, sent } = fakeCalls({
        realm: { secrets: () => answers.shift()?.() ?? late.promise },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("Kept.")],
    });
    await until(() => named(sent, "secrets").length === 1);

    await assert.rejects(page(agent), /timed out/);

    await agent.actions.submit(agent.by(from.realm), {
        id: "line-keep",
        input: { type: "line", text: `Keep ${STORED} safe.` },
    });
    await until(() => named(sent, "secrets").length === 3);
    late.resolve({ version: 1, values: [STORED] });
    await agent.idle();
    await until(() =>
        JSON.stringify(agent.events).includes("Keep [REDACTED] safe."),
    );

    assert.ok(!JSON.stringify(agent.events).includes(STORED));
    assert.match(JSON.stringify(await page(agent)), /Keep \[REDACTED\] safe/);
    await agent.stop();
});

/**
 * The host loads the realm's secret list once in its process, so this file
 * holds the one test that stops a turn before that load.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import type { SecretList } from "../src/agent/messages/realm.ts";
import { fakeCalls, from, named, wakeAgent } from "./agent-testing.ts";

test("a stop ends a turn whose model call waits for a secret list the realm has not answered", async (t) => {
    const late = Promise.withResolvers<SecretList>();
    const spending = Promise.withResolvers<void>();
    const { calls, sent } = fakeCalls({
        realm: {
            secrets: () => late.promise,
            may_spend: () => {
                spending.resolve();
                return { type: "done" };
            },
        },
    });
    const agent = await wakeAgent(t, { calls, responses: [] });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await spending.promise;

    assert.deepEqual(
        await agent.actions.stop(agent.by(from.client), { id: "stop-1" }),
        { turns: 1 },
    );
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 0);
    assert.deepEqual(named(sent, "turn_failed"), []);
    late.resolve({ version: 1, values: [] });
    await agent.stop();
});

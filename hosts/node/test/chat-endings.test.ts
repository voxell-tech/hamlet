import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    type FauxResponseStep,
    fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import type { HarnessSettings } from "@earendil-works/pi-durable";
import type { TurnFailed } from "../src/agent/messages/realm.ts";
import {
    fakeCalls,
    from,
    named,
    notices,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

/**
 * A model call that answers only when the host stops it, and resolves
 * `asked` when it is made.
 */
function unanswered(asked: PromiseWithResolvers<void>): FauxResponseStep {
    return (_request, options) => {
        asked.resolve();
        return new Promise((resolve) =>
            options?.signal?.addEventListener(
                "abort",
                () =>
                    resolve(
                        fauxAssistantMessage("", {
                            stopReason: "aborted",
                            errorMessage: "aborted",
                        }),
                    ),
                { once: true },
            ),
        );
    };
}

/**
 * Wakes the agent with `settings`, gives it a line, waits for `cut` and
 * kills the host there, then wakes it again.
 */
async function cutOff(
    t: TestContext,
    settings: HarnessSettings,
    cut: Promise<void>,
    calls: ReturnType<typeof fakeCalls>,
    responses: FauxResponseStep[],
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls: calls.calls,
        settings,
        responses,
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await cut;
    const again = await agent.killed();
    t.after(() => again.stop());
    return again;
}

/**
 * Waits for the realm to hear the turn failed, and checks it heard once,
 * with `words`, which the notice shows too. The words are the run's own.
 */
async function failedOnce(
    agent: Woken,
    calls: ReturnType<typeof fakeCalls>,
    words: string,
): Promise<void> {
    await until(() => named(calls.sent, "turn_failed").length > 0);
    await agent.idle();
    const told = named(calls.sent, "turn_failed").map(
        (call) => call.message as TurnFailed,
    );
    assert.equal(told.length, 1);
    const [failed] = told;
    assert.ok(failed !== undefined);
    assert.deepEqual(
        { task: failed.task, words: failed.words, cause: failed.cause },
        { task: "task-1", words, cause: "model" },
    );
    assert.deepEqual(
        (await notices(agent)).map((notice) => notice.text),
        [words],
    );
}

test("a turn killed mid-answer more often than it retries ends interrupted, with a notice and the realm told once", async (t) => {
    const asked = Promise.withResolvers<void>();
    const calls = fakeCalls({});
    const agent = await cutOff(
        t,
        { retry: { maxRetries: 0 } },
        asked.promise,
        calls,
        [unanswered(asked)],
    );
    await failedOnce(
        agent,
        calls,
        "The host stopped while the model was answering, on attempt 1",
    );
});

test("a turn killed before its model is asked as often as the restart limit ends there, with a notice and the realm told once", async (t) => {
    const spending = Promise.withResolvers<void>();
    const calls = fakeCalls({
        realm: {
            may_spend: () => {
                spending.resolve();
                return new Promise(() => {});
            },
        },
    });
    const agent = await cutOff(
        t,
        { restartLimit: 1 },
        spending.promise,
        calls,
        [],
    );
    await failedOnce(
        agent,
        calls,
        "The generation was interrupted once in its request phase",
    );
});

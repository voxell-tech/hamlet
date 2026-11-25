import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    type AssistantMessage,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
} from "@earendil-works/pi-ai";
import type { HarnessSettings } from "@earendil-works/pi-durable";
import type { Calls } from "../src/actor/actor/mod.ts";
import type { Input } from "../src/agent/messages/agent.ts";
import {
    fakeCalls,
    from,
    modelFailed,
    named,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

async function wake(
    t: TestContext,
    calls: Calls,
    responses: FauxResponseStep[],
    settings?: HarnessSettings,
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        responses,
        ...(settings === undefined ? {} : { settings }),
    });
    t.after(() => agent.stop());
    return agent;
}

let inputs = 0;

function submitted(agent: Woken, input: Input, id?: string): Promise<null> {
    inputs += 1;
    return agent.actions.submit(agent.by(from.realm), {
        id: id ?? `stop-${inputs}`,
        input,
    });
}

const line = (text: string): Input => ({ type: "line", text });

/** A model request that waits until it is answered or aborted. */
function held(): {
    readonly step: FauxResponseStep;
    readonly asked: Promise<void>;
    answer(message: AssistantMessage): void;
} {
    const asked = Promise.withResolvers<void>();
    const answer = Promise.withResolvers<AssistantMessage>();
    return {
        step: (_request, options) => {
            asked.resolve();
            const aborted = Promise.withResolvers<AssistantMessage>();
            options?.signal?.addEventListener(
                "abort",
                () =>
                    aborted.resolve(
                        fauxAssistantMessage("", {
                            stopReason: "aborted",
                            errorMessage: "aborted",
                        }),
                    ),
                { once: true },
            );
            return Promise.race([answer.promise, aborted.promise]);
        },
        asked: asked.promise,
        answer: answer.resolve,
    };
}

function seen(asked: (readonly Message[])[], text: string): FauxResponseStep {
    return (request) => {
        asked.push(request.messages);
        return fauxAssistantMessage(text);
    };
}

function words(messages: readonly Message[]): string[] {
    return messages.flatMap((message) =>
        message.role !== "user"
            ? []
            : typeof message.content === "string"
              ? [message.content]
              : message.content.flatMap((part) =>
                    part.type === "text" ? [part.text] : [],
                ),
    );
}

async function shown(agent: Woken): Promise<string[]> {
    const page = await agent.actions.read_chat(agent.by(from.client), {
        id: "11111111-1111-4111-8111-111111111111",
        chat: { with: 7 },
    });
    return page.items
        .filter((item) => item.kind === "user")
        .map((item) => item.text);
}

test("a stop withdraws a line queued behind a failed run", async (t) => {
    const asked: (readonly Message[])[] = [];
    const first = held();
    const agent = await wake(
        t,
        fakeCalls({}).calls,
        [first.step, seen(asked, "Heard.")],
        { retry: { maxRetries: 0 } },
    );
    await submitted(agent, line("First."));
    await first.asked;
    await submitted(agent, line("Queued."));
    first.answer(modelFailed("the provider is down"));
    await agent.idle();

    assert.deepEqual(
        await agent.actions.stop(agent.by(from.realm), { number: 1 }),
        { turns: 1 },
    );
    await submitted(agent, line("After."));
    await agent.idle();
    const request = words(asked[0] ?? []);
    assert.ok(request.includes("After."));
    assert.ok(!request.includes("Queued."), "the queued line was withdrawn");
});

test("a stop delivered twice aborts once", async (t) => {
    const { calls, sent } = fakeCalls({});
    const before = held();
    const after = held();
    const agent = await wake(t, calls, [before.step, after.step]);
    await submitted(agent, line("Before."));
    await before.asked;

    const stop = { number: 3, withdrawn: [] };
    assert.deepEqual(await agent.actions.stop(agent.by(from.realm), stop), {
        turns: 1,
    });
    await agent.idle();
    await submitted(agent, line("After."));
    await after.asked;
    assert.deepEqual(
        await agent.actions.stop(agent.by(from.realm), stop),
        { turns: 1 },
        "a repeat answers the first count",
    );
    after.answer(fauxAssistantMessage("Answered after."));
    await agent.idle();

    assert.equal(named(sent, "cancel").length, 1);
    assert.deepEqual(named(sent, "lapse"), []);
    const page = await agent.actions.read_chat(agent.by(from.client), {
        id: "11111111-1111-4111-8111-111111111111",
        chat: { with: 7 },
    });
    assert.ok(
        page.items.some((item) => item.text === "Answered after."),
        "the line given after the stop ran to its answer",
    );
});

test("a withdrawn line stays in the chat", async (t) => {
    const agent = await wake(t, fakeCalls({}).calls, []);
    assert.deepEqual(
        await agent.actions.stop(agent.by(from.realm), {
            number: 1,
            withdrawn: [{ id: "line-9", line: line("Never ran.") }],
        }),
        { turns: 1 },
    );
    await agent.idle();

    assert.deepEqual(await shown(agent), ["Never ran."]);
    assert.equal(agent.faux.state.callCount, 0);
});

// A host frozen with a line in flight reads the line's `Submit` after the
// `Stop` that withdrew it.
test("a line submitted after the stop that withdrew it shows once and never runs", async (t) => {
    const agent = await wake(t, fakeCalls({}).calls, [
        fauxAssistantMessage("Never asked."),
    ]);
    await agent.actions.stop(agent.by(from.realm), {
        number: 1,
        withdrawn: [{ id: "line-9", line: line("Too late.") }],
    });
    await submitted(agent, line("Too late."), "line-9");
    await agent.idle();

    assert.deepEqual(await shown(agent), ["Too late."]);
    assert.equal(agent.faux.state.callCount, 0);
});

test("an agent reports open once per busy period", async (t) => {
    const { calls, sent } = fakeCalls({});
    const first = held();
    const agent = await wake(t, calls, [
        first.step,
        fauxAssistantMessage("Second."),
        fauxAssistantMessage("Third."),
        fauxAssistantMessage("Fourth."),
    ]);
    await submitted(agent, line("One."));
    await first.asked;
    await submitted(agent, line("Two."));
    await submitted(agent, {
        type: "letter",
        from: { agent: 12, name: "Ada" },
        text: "Three.",
    });
    first.answer(fauxAssistantMessage("First."));
    await agent.idle();
    await until(() => named(sent, "working").length === 2);
    await submitted(agent, line("Four."));
    await agent.idle();
    await until(() => named(sent, "working").length === 4);

    assert.deepEqual(
        named(sent, "working").map((call) => call.message),
        [{ open: true }, { open: false }, { open: true }, { open: false }],
    );
});

// The realm's try was cut off after the agent took the line, so its stop
// names the line as withdrawn and the try's repeat comes after the stop.
test("a line the agent took before the stop that names it shows once, and its repeat is answered", async (t) => {
    const slow = held();
    const agent = await wake(t, fakeCalls({}).calls, [slow.step]);
    await submitted(agent, line("Taken."), "line-7");
    await slow.asked;
    assert.deepEqual(
        await agent.actions.stop(agent.by(from.realm), {
            number: 1,
            withdrawn: [{ id: "line-7", line: line("Taken.") }],
        }),
        { turns: 1 },
    );
    await submitted(agent, line("Taken."), "line-7");
    await agent.idle();

    assert.deepEqual(await shown(agent), ["Taken."]);
    assert.equal(agent.faux.state.callCount, 1);
});

// A frozen host reads the line's `Submit` and then the `Stop` that names it:
// the stop waits for the admission under way and aborts its turn.
test("a stop waits for the admission under way of a line it withdrew, and aborts its turn", async (t) => {
    const slow = held();
    const agent = await wake(t, fakeCalls({}).calls, [slow.step]);
    const reading = agent.holdRead(
        "SELECT conversation FROM chat WHERE kind = 'chief' AND current = 1",
    );
    const admitted = submitted(agent, line("Racing."), "line-5");
    await reading.reached;
    const stopping = agent.actions.stop(agent.by(from.realm), {
        number: 1,
        withdrawn: [{ id: "line-5", line: line("Racing.") }],
    });
    reading.release();

    assert.equal(await admitted, null);
    assert.deepEqual(await stopping, { turns: 1 });
    await agent.idle();
    assert.deepEqual(await shown(agent), ["Racing."]);
});

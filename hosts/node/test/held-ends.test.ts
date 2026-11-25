import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    type AssistantMessage,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
    type ToolResultMessage,
} from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type EntryRecord,
    LiveDoc,
} from "@earendil-works/pi-durable";
import type { Calls } from "../src/actor/actor/mod.ts";
import type { Input, Outcome } from "../src/agent/messages/agent.ts";
import type { CallId } from "../src/agent/messages/shared.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    fakeCalls,
    from,
    type Sent,
    toolCall,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

async function wake(
    t: TestContext,
    calls: Calls,
    responses: FauxResponseStep[],
): Promise<Woken> {
    const agent = await wakeAgent(t, { calls, responses });
    t.after(() => agent.stop());
    return agent;
}

let inputs = 0;

async function send(agent: Woken, input: Input): Promise<void> {
    inputs += 1;
    await agent.actions.submit(agent.by(from.realm), {
        id: `held-${inputs}`,
        input,
    });
    await agent.idle();
}

function seen(asked: (readonly Message[])[], text: string): FauxResponseStep {
    return (request) => {
        asked.push(request.messages);
        return fauxAssistantMessage(text);
    };
}

/** A model request that waits until it is answered or aborted. */
function slow(): {
    readonly step: FauxResponseStep;
    readonly asked: Promise<void>;
} {
    const asked = Promise.withResolvers<void>();
    return {
        step: (_request, options) => {
            asked.resolve();
            return new Promise<AssistantMessage>((resolve) =>
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
        },
        asked: asked.promise,
    };
}

function callOf(sent: readonly Sent[], act: string): CallId {
    const call = sent.find((one) => one.name === act)?.message as
        | { readonly call?: CallId }
        | undefined;
    assert.ok(call?.call !== undefined, `the agent called ${act}`);
    return call.call;
}

/** What the model read as the result of its call `id`. */
function resultOf(messages: readonly Message[], id: string): string[] {
    return messages.flatMap((message) =>
        message.role === "toolResult" && message.toolCallId === id
            ? [
                  message.content
                      .map((part) => (part.type === "text" ? part.text : ""))
                      .join(""),
              ]
            : [],
    );
}

/** Today's words for a held call that ended without an answer, as the model reads them. */
const aborted = (tool: string) =>
    `<harness>\n[error] Tool ${tool} was aborted\n</harness>`;

async function chief(agent: Woken): Promise<ConversationId> {
    const id = await conversationOfChat(agent.by(from.client), { with: 7 });
    assert.ok(id !== undefined, "the chief has a current session");
    return id;
}

async function entriesOf(
    agent: Woken,
    id: ConversationId,
): Promise<readonly EntryRecord[]> {
    const page = await (
        await (await agent.harness()).conversation(id, context)
    )?.entries({ order: "ascending" }, 500, undefined, context);
    return page?.items ?? [];
}

test("pi-durable admits a write that replaces a held call's result, starts no run, and the next request reads it in place", async (t) => {
    const asked: (readonly Message[])[] = [];
    const agent = await wake(
        t,
        fakeCalls({ realm: { ask_chief: () => ({ type: "done" }) } }).calls,
        [
            toolCall("ask_chief", { questions: [{ question: "Which?" }] }),
            fauxAssistantMessage("Waiting on the chief."),
            seen(asked, "Heard."),
        ],
    );
    await send(agent, { type: "line", text: "Ask." });
    const id = await chief(agent);
    const held = (await entriesOf(agent, id)).find(
        (entry) => entry.kind === "pi.tool-result",
    );
    assert.ok(held !== undefined, "the held call has its result");
    const result = held.model?.[0] as ToolResultMessage;
    const harness = await agent.harness();
    const conversation = await harness.conversation(id, context);
    const written = await conversation?.submit(
        {
            type: "write",
            requestId: "proof",
            entry: {
                kind: "hamlet.notice",
                data: { text: "ended" },
                edits: [
                    {
                        target: held.id,
                        action: "replace",
                        messages: [
                            {
                                ...result,
                                content: [
                                    {
                                        type: "text",
                                        text: aborted("ask_chief"),
                                    },
                                ],
                                isError: true,
                            },
                        ],
                    },
                ],
            },
        },
        context,
    );
    assert.equal((await written?.status(context))?.status, "done");
    await agent.idle();
    const live = await harness.snapshot(LiveDoc, id, context);
    assert.equal(live?.run, undefined, "the write started no run");
    assert.equal(agent.faux.state.callCount, 2);

    await send(agent, { type: "line", text: "Go on." });
    const request = asked[0] ?? [];
    assert.deepEqual(resultOf(request, "c1"), [aborted("ask_chief")]);
    assert.deepEqual(
        request.map((message) => message.role),
        ["system", "user", "assistant", "toolResult", "assistant", "user"],
        "the result is read where the held one was",
    );
});

test("a stopped held call reads aborted and starts no run", async (t) => {
    const asked: (readonly Message[])[] = [];
    const { calls, sent } = fakeCalls({
        realm: { ask_chief: () => ({ type: "done" }) },
    });
    const agent = await wake(t, calls, [
        toolCall("ask_chief", { questions: [{ question: "Which?" }] }),
        fauxAssistantMessage("Waiting on the chief."),
        seen(asked, "Heard."),
    ]);
    await send(agent, { type: "line", text: "Ask." });
    await send(agent, {
        type: "notice",
        text: "the call's wait for a chief lapsed: the turn was stopped",
        held: { call: callOf(sent, "ask_chief"), end: "aborted" },
    });
    assert.equal(agent.faux.state.callCount, 2, "the lapse started no run");

    await send(agent, { type: "line", text: "Go on." });
    assert.deepEqual(resultOf(asked[0] ?? [], "c1"), [aborted("ask_chief")]);
});

// The end comes while another run of the conversation is under way, so it
// queues; the stop that follows aborts that run, and the next request reads
// the end in place.
test("a held call's end that lands while its conversation is busy is read after the abort", async (t) => {
    const asked: (readonly Message[])[] = [];
    const { calls, sent } = fakeCalls({
        realm: { ask_chief: () => ({ type: "done" }) },
    });
    const busy = slow();
    const agent = await wake(t, calls, [
        toolCall("ask_chief", { questions: [{ question: "Which?" }] }),
        busy.step,
        seen(asked, "Heard."),
    ]);
    inputs += 1;
    await agent.actions.submit(agent.by(from.realm), {
        id: `held-${inputs}`,
        input: { type: "line", text: "Ask." },
    });
    await busy.asked;
    inputs += 1;
    await agent.actions.submit(agent.by(from.realm), {
        id: `held-${inputs}`,
        input: {
            type: "notice",
            text: "the call's wait for a chief lapsed: the turn was stopped",
            held: { call: callOf(sent, "ask_chief"), end: "aborted" },
        },
    });
    assert.deepEqual(
        await agent.actions.stop(agent.by(from.realm), { number: 1 }),
        { turns: 1 },
    );
    await agent.idle();
    assert.equal(agent.faux.state.callCount, 2, "the end started no run");

    await send(agent, { type: "line", text: "Go on." });
    assert.deepEqual(resultOf(asked[0] ?? [], "c1"), [aborted("ask_chief")]);
});

test("a cancelled command starts no run", async (t) => {
    const asked: (readonly Message[])[] = [];
    const { calls, sent } = fakeCalls({
        workspace: { run: () => ({ type: "running", command: 4 }) },
    });
    const agent = await wake(t, calls, [
        toolCall("bash", { command: "sleep 60" }),
        fauxAssistantMessage("Running."),
        seen(asked, "Heard."),
    ]);
    await send(agent, { type: "line", text: "Run it." });
    const cancelled: Outcome = {
        type: "run",
        end: {
            type: "ran",
            command: 4,
            exit: { type: "cancelled" },
            output: "",
            policy: "sandbox",
            took_ms: 6004,
        },
    };
    await agent.actions.finished(agent.by(from.workspace), {
        call: callOf(sent, "run"),
        outcome: cancelled,
    });
    await agent.idle();
    assert.equal(agent.faux.state.callCount, 2, "the end started no run");

    await send(agent, { type: "line", text: "Go on." });
    assert.deepEqual(resultOf(asked[0] ?? [], "c1"), [aborted("bash")]);
});

import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type AssistantMessage,
    createAssistantMessageEventStream,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
    type Models,
} from "@earendil-works/pi-ai";
import { compactionFor } from "../src/agent/compaction.ts";
import {
    fakeCalls,
    from,
    named,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { fauxModels } from "./testing.ts";

const SUMMARY = "Lines 1 and 2 asked for words.";

/** About 10,000 tokens by pi-ai's estimate of 3.5 characters a token. */
function long(n: number): string {
    return `Line ${n}. ${"word ".repeat(7_000)}`;
}

function textOf(message: Message): string {
    if (typeof message.content === "string") return message.content;
    return message.content
        .map((part) => (part.type === "text" ? part.text : ""))
        .join("");
}

/**
 * Answers a summarising request with `SUMMARY` and every other request with
 * the next of `answers`, so a compaction never takes an answer meant for a
 * turn. `asked` keeps the texts of every request but the summaries'. A
 * summary is answered once `summarising` resolves, or fails when its
 * request is aborted first.
 */
function script(
    answers: AssistantMessage[],
    summarising: () => Promise<void> = () => Promise.resolve(),
) {
    const asked: string[][] = [];
    let summaries = 0;
    const step: FauxResponseStep = async (request, options) => {
        const texts = request.messages.map(textOf);
        if (texts.some((text) => text.startsWith("<conversation>"))) {
            summaries += 1;
            const aborted = Promise.withResolvers<never>();
            options?.signal?.addEventListener("abort", () =>
                aborted.reject(new Error("the summary was aborted")),
            );
            await Promise.race([summarising(), aborted.promise]);
            return fauxAssistantMessage(SUMMARY);
        }
        asked.push(texts);
        return answers.shift() ?? fauxAssistantMessage("No answer was left.");
    };
    return {
        asked,
        get summaries() {
            return summaries;
        },
        steps: Array.from({ length: 20 }, () => step),
    };
}

/**
 * The faux provider measures a request itself; an answer whose
 * `responseId` is `measured` reports `tokens` of context instead.
 */
function measuring(tokens: number): (models: Models) => Models {
    const usage = (message: AssistantMessage): AssistantMessage =>
        message.responseId === "measured"
            ? {
                  ...message,
                  usage: {
                      ...message.usage,
                      input: tokens,
                      output: 10,
                      cacheRead: 0,
                      cacheWrite: 0,
                      totalTokens: tokens + 10,
                  },
              }
            : message;
    return (models) =>
        new Proxy(models, {
            get(target, key, receiver) {
                if (key === "streamSimple") {
                    return (...args: Parameters<Models["streamSimple"]>) => {
                        const out = createAssistantMessageEventStream();
                        void (async () => {
                            const source = target.streamSimple(...args);
                            for await (const event of source) {
                                out.push(
                                    event.type === "done"
                                        ? {
                                              ...event,
                                              message: usage(event.message),
                                          }
                                        : event,
                                );
                            }
                            out.end(usage(await source.result()));
                        })();
                        return out;
                    };
                }
                const value: unknown = Reflect.get(target, key, receiver);
                return typeof value === "function" ? value.bind(target) : value;
            },
        });
}

async function say(agent: Woken, n: number, text: string): Promise<void> {
    await agent.actions.submit(agent.by(from.realm), {
        id: `input-${n}`,
        task: `task-${n}`,
        input: { type: "line", text },
    });
    await agent.idle();
}

test("a long faux run compacts and keeps 20000 tokens", async (t) => {
    // The faux model's window is 128,000 tokens: a fifth of it kept for the
    // answer blocks at 102,400. pi-durable's own 16,384 would block only
    // at 111,616, and compact the 105,000 below in the background.
    const faux = fauxModels().faux.getModel();
    const run = script([
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        fauxAssistantMessage("Three."),
        fauxAssistantMessage("Four.", { responseId: "measured" }),
        fauxAssistantMessage("Five."),
    ]);
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: run.steps,
        models: measuring(105_000),
        settings: { compaction: compactionFor([faux]) },
    });

    for (const n of [1, 2, 3, 4]) await say(agent, n, long(n));
    await say(agent, 5, "Line 5. Go on.");

    assert.equal(run.summaries, 1);
    const fifth = run.asked[4]?.join("\n") ?? "";
    assert.ok(
        fifth.includes(SUMMARY),
        "the fifth request waited for the summary",
    );
    for (const n of [3, 4, 5]) {
        assert.ok(fifth.includes(`Line ${n}.`), `line ${n} is kept whole`);
    }
    for (const n of [1, 2]) {
        assert.ok(!fifth.includes(`Line ${n}.`), `line ${n} is summarised`);
    }
});

/** An empty answer cut off for its length, as a model out of room gives. */
function cutOffShort(): AssistantMessage {
    return fauxAssistantMessage([], { stopReason: "length" });
}

const CARRY_ON = "Carry on from where you stopped.";

test("a short cut-off compacts once", async (t) => {
    const run = script([
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        fauxAssistantMessage("Three."),
        cutOffShort(),
        fauxAssistantMessage("Four."),
    ]);
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, { calls, responses: run.steps });

    for (const n of [1, 2, 3]) await say(agent, n, long(n));
    await say(agent, 4, "Line 4. Answer now.");

    assert.equal(run.summaries, 1);
    assert.equal(run.asked.length, 5);
    const again = run.asked[4] ?? [];
    assert.equal(again.at(-1), CARRY_ON);
    assert.ok(again.includes("Line 4. Answer now."));
    assert.ok(again.some((text) => text.includes(SUMMARY)));
    assert.ok(!again.some((text) => text.startsWith("Line 1.")));
    const entries = await agent.entries();
    assert.equal(
        entries.filter((entry) => entry.kind === "pi.compaction").length,
        1,
    );
    assert.equal(entries.at(-1)?.text, "Four.");
    const charged = named(sent, "charge").at(-1)?.message as { task: string };
    assert.equal(charged.task, "task-4", "it asks again under the same task");
});

test("a second short cut-off after the compaction fails with today's words", async (t) => {
    const run = script([
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        fauxAssistantMessage("Three."),
        cutOffShort(),
        cutOffShort(),
    ]);
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls, responses: run.steps });

    for (const n of [1, 2, 3]) await say(agent, n, long(n));
    await say(agent, 4, "Line 4. Answer now.");

    assert.equal(run.summaries, 1);
    assert.equal(run.asked.length, 5);
    const notices = (await agent.entries()).filter(
        (entry) => entry.kind === "hamlet.notice",
    );
    assert.deepEqual(
        notices.map((notice) => (notice.data as { text: string }).text),
        [
            "the model has no room left to answer, even with the conversation summarised. Start a new session, or choose a model that holds more",
        ],
    );
});

test("a kill while the summary is out compacts once and asks again once", async (t) => {
    const released = Promise.withResolvers<void>();
    const run = script(
        [
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Two."),
            fauxAssistantMessage("Three."),
            cutOffShort(),
            fauxAssistantMessage("Four."),
        ],
        () => released.promise,
    );
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls, responses: run.steps });
    for (const n of [1, 2, 3]) await say(agent, n, long(n));

    await agent.actions.submit(agent.by(from.realm), {
        id: "input-4",
        task: "task-4",
        input: { type: "line", text: "Line 4. Answer now." },
    });
    await until(() => run.summaries === 1);
    await agent.stop();
    const again = await agent.again();
    released.resolve();
    await again.idle();

    assert.equal(run.asked.length, 5);
    assert.equal(run.asked[4]?.at(-1), CARRY_ON);
    const entries = await again.entries();
    assert.equal(
        entries.filter((entry) => entry.kind === "pi.compaction").length,
        1,
    );
    assert.equal(
        entries.filter((entry) => entry.kind === "hamlet.cut-off").length,
        1,
    );
    assert.equal(entries.at(-1)?.text, "Four.");
});

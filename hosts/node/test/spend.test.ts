import assert from "node:assert/strict";
import { test } from "node:test";
import {
    fauxAssistantMessage,
    isRetryableAssistantError,
    type Models,
} from "@earendil-works/pi-ai";
import { gated, refusalOf } from "../src/agent/spend.ts";
import { Closing, peersOf } from "../src/agent/turn.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    named,
    priced,
    timedOut,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import { context, fauxModels } from "./testing.ts";

const onTask = (calls: Parameters<typeof peersOf>[0]["calls"]) =>
    peersOf({
        calls,
        files: fakeFiles({}),
        turns: {
            of: async () => ({ task: { from: "input", task: "task-1" } }),
            ofSession: async () => ({
                task: { from: "input", task: "task-1" },
            }),
        },
        report: () => {},
    });

const line = (id: string, text: string, task?: string) => ({
    id,
    ...(task === undefined ? {} : { task }),
    input: { type: "line" as const, text },
});

test("each provider request is asked for and charged once under its own id", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        models: (models) => priced(models, 0.00125),
        responses: [fauxAssistantMessage("One."), fauxAssistantMessage("Two.")],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Go.", "task-1"),
    );
    await agent.idle();
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-2", "On.", "task-1"),
    );
    await agent.idle();

    assert.deepEqual(
        sent
            .filter((call) => call.to === "realm/local")
            .map((call) => call.name)
            .filter((name) => name !== "read_copies" && name !== "secrets"),
        ["may_spend", "charge", "may_spend", "charge"],
    );
    assert.deepEqual(
        named(sent, "may_spend").map((call) => call.message),
        [{ task: "task-1" }, { task: "task-1" }],
    );
    const charges = named(sent, "charge").map(
        (call) =>
            call.message as {
                call: string;
                task: string;
                micros: number;
                took_ms: number;
            },
    );
    for (const charge of charges) {
        assert.match(charge.call, /^m[0-9a-f-]{36}$/);
        assert.equal(charge.task, "task-1");
        assert.equal(charge.micros, 1250);
        assert.ok(Number.isInteger(charge.took_ms) && charge.took_ms >= 0);
    }
    assert.notEqual(charges[0]?.call, charges[1]?.call);
    await agent.stop();
});

test("a refused model call ends the turn with the realm's words and sends no request", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            may_spend: () => ({
                type: "refused",
                why: { type: "over_time" },
                // Words that hold a status code, which pi-ai would retry.
                words: "the model time of the realm has run out: 500 of 500 ms spent; a chief starts it again with /reset-budget",
            }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("never sent")],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Go.", "task-1"),
    );
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 0);
    // Asked once: the refusal is not retried, and nothing is charged.
    assert.deepEqual(
        sent
            .map((call) => call.name)
            .filter(
                (name) =>
                    name !== "list_tools" &&
                    name !== "read_copies" &&
                    name !== "secrets",
            ),
        ["may_spend", "turn_failed"],
    );
    const harness = await agent.harness();
    const { submissions } = await harness.inspect(context);
    assert.deepEqual(submissions, []);
    const root = await harness.root(context);
    const failure = await harness.commit(
        (tx) => tx.submissionByRequest(root.id, "input-1"),
        context,
    );
    assert.equal(failure?.status, "unanswered");
    assert.equal(failure?.reason, "model_error");
    assert.equal(
        failure?.detail,
        "The realm's billing did not allow this model request, and it was not sent: the model time of the realm has run out: 500 of 500 ms spent; a chief starts it again with /reset-budget",
    );
    await agent.stop();
});

test("a failed charge leaves the turn going", async (t) => {
    const { calls } = fakeCalls({
        realm: {
            charge: () => {
                throw timedOut();
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("Done.")],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Go."));
    await agent.idle();

    assert.equal(
        (await agent.entries()).find((entry) => entry.kind === "pi.assistant")
            ?.text,
        "Done.",
    );
    assert.deepEqual(agent.reported, [
        "a model request's charge was not taken: actor.action_timed_out: The action timed out.",
    ]);
    await agent.stop();
});

test("a refusal is never retried, whatever its words hold; without its opening it would be", () => {
    const { faux } = fauxModels();
    const model = faux.getModel();
    const words = [
        // The realm's words, with a status code in them.
        "the model time of the realm has run out: 500 of 500 ms spent",
        "it could not be asked (actor.action_timed_out: The action timed out.)",
        "it could not be asked (actor.internal_error: connection refused)",
    ];
    for (const why of words) {
        const refusal = refusalOf(model, why);
        assert.equal(refusal.stopReason, "error");
        assert.equal(isRetryableAssistantError(refusal), false, why);
    }
    // The mutant: the same words alone are retried, so the request would be
    // asked for again up to three times.
    assert.equal(
        isRetryableAssistantError({
            ...refusalOf(model, ""),
            errorMessage: words[0] as string,
        }),
        true,
    );
});

test("a question that fails sends no request and ends the turn with Hamlet's words", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            may_spend: () => {
                throw timedOut();
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("never sent")],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Go."));
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 0);
    assert.equal(named(sent, "may_spend").length, 1);
    assert.equal(named(sent, "charge").length, 0);
    const harness = await agent.harness();
    const root = await harness.root(context);
    const failure = await harness.commit(
        (tx) => tx.submissionByRequest(root.id, "input-1"),
        context,
    );
    assert.equal(failure?.status, "unanswered");
    assert.equal(
        failure?.detail,
        "The realm's billing did not allow this model request, and it was not sent: it could not be asked (actor.action_timed_out: The action timed out.)",
    );
    await agent.stop();
});

test("a compaction's request names its session, and is asked for and charged to the standing task", async (t) => {
    const seen: { method: string; session: boolean }[] = [];
    const recording = (models: Models): Models =>
        new Proxy(models, {
            get(target, key, receiver) {
                const value: unknown = Reflect.get(target, key, receiver);
                if (typeof value !== "function") return value;
                if (key === "streamSimple" || key === "completeSimple") {
                    return (...args: unknown[]) => {
                        const options = args[2] as
                            | { sessionId?: string }
                            | undefined;
                        seen.push({
                            method: key,
                            session: typeof options?.sessionId === "string",
                        });
                        return (value as (...args: unknown[]) => unknown).apply(
                            target,
                            args,
                        );
                    };
                }
                return value.bind(target);
            },
        });
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        models: recording,
        // Keep nothing verbatim, so a manual compaction has a range to summarize.
        settings: { compaction: { keepRecentTokens: 0 } },
        responses: [
            fauxAssistantMessage("A long answer."),
            fauxAssistantMessage("Another."),
            fauxAssistantMessage("A summary."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Go.", "task-1"),
    );
    await agent.idle();
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-2", "On.", "task-1"),
    );
    await agent.idle();
    const harness = await agent.harness();
    const root = await harness.root(context);
    const compaction = await root.compact(undefined, context);
    await harness.waitForTask(compaction, context);

    assert.deepEqual(seen, [
        { method: "streamSimple", session: true },
        { method: "streamSimple", session: true },
        { method: "completeSimple", session: true },
    ]);
    // The compaction ran with no run live, so no input names a task.
    assert.deepEqual(
        named(sent, "may_spend").map((call) => call.message),
        [{ task: "task-1" }, { task: "task-1" }, { task: "standing-7" }],
    );
    assert.equal(named(sent, "charge").length, 3);
    await agent.stop();
});

test("a model request with no session is charged to the standing task", async () => {
    const { calls, sent } = fakeCalls({});
    const { faux, models } = fauxModels();
    faux.setResponses([fauxAssistantMessage("ok")]);
    const peers = peersOf({
        calls,
        files: fakeFiles({}),
        turns: {
            of: async () => ({ task: { from: "input", task: "task-1" } }),
            ofSession: async (session) =>
                session === undefined
                    ? { task: { from: "standing", task: "standing-7" } }
                    : { task: { from: "input", task: "task-1" } },
        },
        report: () => {},
    });
    const answer = await gated(models, peers).completeSimple(faux.getModel(), {
        messages: [{ role: "user", content: "hi", timestamp: 0 }],
    });
    assert.equal(answer.stopReason, "stop");
    assert.deepEqual(
        named(sent, "may_spend").map((call) => call.message),
        [{ task: "standing-7" }],
    );
});

test("a charge too large for a u32 is held at 2^32 - 1, whether its time is measured or given", async (t) => {
    const FIFTY_DAYS_MS = 50 * 24 * 60 * 60 * 1000;
    t.mock.timers.enable({ apis: ["Date"] });
    const { calls, sent } = fakeCalls({});
    const { faux, models } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage("measured"),
        fauxAssistantMessage("given"),
    ]);
    // 5,000 dollars: five billion micros. The first answer has no duration
    // and its request takes fifty days by the clock; the second says it took fifty days.
    const huge = new Proxy(models, {
        get(target, key, receiver) {
            if (key !== "completeSimple")
                return Reflect.get(target, key, receiver);
            return async (...args: Parameters<Models["completeSimple"]>) => {
                const { durationMs: _measured, ...message } =
                    await target.completeSimple(...args);
                const usage = {
                    ...message.usage,
                    cost: { ...message.usage.cost, total: 5000 },
                };
                if (
                    message.content.some(
                        (part) =>
                            part.type === "text" && part.text === "measured",
                    )
                ) {
                    t.mock.timers.tick(FIFTY_DAYS_MS);
                    return { ...message, usage };
                }
                return { ...message, usage, durationMs: FIFTY_DAYS_MS };
            };
        },
    });
    const gate = gated(huge, onTask(calls));
    for (const _ of ["measured", "given"]) {
        await gate.completeSimple(faux.getModel(), {
            messages: [{ role: "user", content: "hi", timestamp: 0 }],
        });
    }

    assert.deepEqual(
        named(sent, "charge").map((call) => {
            const { micros, took_ms } = call.message as {
                micros: number;
                took_ms: number;
            };
            return { micros, took_ms };
        }),
        [
            { micros: 2 ** 32 - 1, took_ms: 2 ** 32 - 1 },
            { micros: 2 ** 32 - 1, took_ms: 2 ** 32 - 1 },
        ],
    );
});

test("a request that finds the harness closing waits for the run's abort, and is no refusal", async () => {
    const { calls, sent } = fakeCalls({});
    const { faux, models } = fauxModels();
    const closing = new Closing("the agent's harness is closing (destroyed)");
    const peers = peersOf({
        calls,
        files: fakeFiles({}),
        turns: {
            of: () => Promise.reject(closing),
            ofSession: () => Promise.reject(closing),
        },
        report: () => {},
    });
    const run = new AbortController();
    const asked = gated(models, peers)
        .completeSimple(
            faux.getModel(),
            { messages: [] },
            { signal: run.signal },
        )
        .then((message) => message);
    assert.equal(
        await Promise.race([
            asked.then(() => "ended"),
            new Promise((r) => setTimeout(() => r("waits"), 50)),
        ]),
        "waits",
    );
    run.abort();
    const ended = await asked;
    assert.deepEqual(sent, []);
    assert.equal(ended.errorMessage, closing.message);
    assert.ok(!ended.errorMessage?.includes("billing"));
});

test("two agents share one model access, and each asks and is charged as itself", async (t) => {
    const seven = fakeCalls({});
    const eight = fakeCalls({});
    const a = await wakeAgent(t, {
        calls: seven.calls,
        key: "7",
        responses: [fauxAssistantMessage("seven")],
    });
    const b = await wakeAgent(t, {
        calls: eight.calls,
        key: "8",
        input: {
            kind: "hire",
            task: "standing-8",
            place: { version: 1, lead: 7, hires: [], roads: [], tools_off: [] },
            copies: {},
        },
        responses: [fauxAssistantMessage("eight")],
    });
    await a.actions.submit(a.by(from.realm), line("input-1", "Go.", "task-1"));
    await b.actions.submit(b.by(from.realm), line("input-1", "Go."));
    await Promise.all([a.idle(), b.idle()]);

    assert.deepEqual(
        named(seven.sent, "may_spend").map((call) => call.message),
        [{ task: "task-1" }],
    );
    assert.deepEqual(
        named(eight.sent, "may_spend").map((call) => call.message),
        [{ task: "standing-8" }],
    );
    assert.equal(named(seven.sent, "charge").length, 1);
    assert.equal(named(eight.sent, "charge").length, 1);
    await a.stop();
    await b.stop();
});

test("a provider's stream that breaks is worded as the provider's failure, retried as one, and not charged", async (t) => {
    let requests = 0;
    // The first request's stream throws after it was sent. The next ones answer.
    const breaking = (models: Models): Models =>
        new Proxy(models, {
            get(target, key, receiver) {
                const value: unknown = Reflect.get(target, key, receiver);
                if (typeof value !== "function") return value;
                if (key !== "streamSimple") return value.bind(target);
                return (...args: Parameters<Models["streamSimple"]>) => {
                    requests += 1;
                    if (requests > 1) return target.streamSimple(...args);
                    const broken = async () => {
                        throw new Error("socket hang up");
                    };
                    return {
                        [Symbol.asyncIterator]: () => ({ next: broken }),
                        result: broken,
                    } as unknown as ReturnType<Models["streamSimple"]>;
                };
            },
        });
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        models: breaking,
        responses: [fauxAssistantMessage("Done.")],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Go.", "task-1"),
    );
    await agent.idle();

    assert.deepEqual(
        sent
            .map((call) => call.name)
            .filter(
                (name) =>
                    name !== "list_tools" &&
                    name !== "read_copies" &&
                    name !== "secrets",
            ),
        ["may_spend", "may_spend", "charge"],
    );
    const texts = (await agent.entries()).map((entry) => entry.text);
    assert.ok(texts.includes("Done."));
    assert.ok(!texts.some((text) => text.includes("billing")));
    await agent.stop();
});

test("a stream's charge is taken before its last event reaches pi-durable", async () => {
    const order: string[] = [];
    const { calls } = fakeCalls({
        realm: {
            charge: async () => {
                await new Promise((resolve) => setTimeout(resolve, 30));
                order.push("charged");
                return null;
            },
        },
    });
    const { faux, models } = fauxModels();
    faux.setResponses([fauxAssistantMessage("ok")]);
    const stream = gated(models, onTask(calls)).streamSimple(faux.getModel(), {
        messages: [{ role: "user", content: "hi", timestamp: 0 }],
    });
    const result = stream.result().then(() => order.push("result"));
    for await (const event of stream) {
        if (event.type === "done") order.push("done");
    }
    await result;

    assert.equal(order[0], "charged");
    assert.deepEqual(order.slice(1).sort(), ["done", "result"]);
});

test("the run's abort cuts off a question the realm never answers", async () => {
    const { calls, sent } = fakeCalls({
        realm: { may_spend: () => new Promise(() => {}) },
    });
    const { faux, models } = fauxModels();
    faux.setResponses([fauxAssistantMessage("never sent")]);
    const run = new AbortController();
    setTimeout(() => run.abort(), 20);
    const ended = await within(
        gated(models, onTask(calls)).completeSimple(
            faux.getModel(),
            { messages: [] },
            { signal: run.signal },
        ),
        1_000,
        "still asking",
    );

    assert.notEqual(ended, "still asking");
    assert.equal(typeof ended === "string" ? ended : ended.stopReason, "error");
    assert.equal(faux.state.callCount, 0);
    assert.deepEqual(
        sent.map((call) => call.name),
        ["may_spend"],
    );
});

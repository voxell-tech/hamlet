import {
    createAssistantMessageEventStream,
    type Usage,
} from "@earendil-works/pi-ai";
import { createModels, createProvider } from "@earendil-works/pi-ai/models";
import {
    createFauxCore,
    fauxAssistantMessage,
} from "@earendil-works/pi-ai/providers/faux";
import type { Storage } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { describe, expect, it } from "vitest";
import { HOME } from "../src/hamlet.ts";
import { localDay } from "../src/home.ts";
import { booksOf, ledgerOf, resetBudget } from "../src/ledger.ts";
import { amountOf } from "../src/money.ts";
import { stateOf } from "../src/state.ts";
import { followHome } from "../src/talk.ts";
import {
    context,
    crashing,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

function metered(now = Date.now) {
    const faux = createFauxCore({
        models: [
            {
                id: "faux-1",
                cost: { input: 1, output: 10, cacheRead: 1, cacheWrite: 1 },
            },
        ],
    });
    const models = createModels();
    const stream: typeof faux.stream = (model, input, options) => {
        const source = faux.stream(model, input, options);
        const out = createAssistantMessageEventStream();
        void (async () => {
            for await (const event of source) {
                if (event.type === "done" || event.type === "error") {
                    const message =
                        event.type === "done" ? event.message : event.error;
                    message.usage = {
                        input: 100,
                        output: 20,
                        cacheRead: 0,
                        cacheWrite: 0,
                        totalTokens: 120,
                        cost: {
                            input: 0,
                            output: 0,
                            cacheRead: 0,
                            cacheWrite: 0,
                            total: 0.0015,
                        },
                    };
                    message.durationMs = 900;
                    message.timestamp = now();
                }
                out.push(event);
            }
            out.end(await source.result());
        })();
        return out;
    };
    models.setProvider(
        createProvider({
            id: "faux",
            models: faux.models,
            auth: {
                apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
            },
            api: { stream, streamSimple: stream },
        }),
    );
    return { models, faux };
}

describe("the spend ledger", () => {
    it.each(["model", "attempt"] as const)(
        "dates an uncounted %s on the request's day after a crash and reopen (H8)",
        async (what) => {
            let at = new Date(2026, 5, 4, 23).getTime();
            const answeredAt = at;
            const { models, faux } = metered(() => at);
            const dir = scratch();
            let home = await testHome(dir, {
                models,
                now: () => at,
                env: { HAMLET_REFLECT_EVERY: "0" },
                openStorage: crashing((writes) =>
                    writes.some(
                        (write) =>
                            write.type === "document.create" &&
                            write.record.kind === "hamlet.ledger",
                    ),
                ),
            });
            faux.setResponses([
                what === "model"
                    ? fauxAssistantMessage("Yes.")
                    : fauxAssistantMessage("", {
                          stopReason: "error",
                          errorMessage: "Bad request",
                      }),
            ]);
            await say(home, "Yesterday.");
            expect(await home.harness.closed).toMatchObject({
                reason: "failed",
            });
            at = new Date(2026, 5, 5, 1).getTime();

            home = await testHome(dir, {
                models,
                now: () => at,
                env: { HAMLET_REFLECT_EVERY: "0" },
            });

            expect(await ledgerOf(home, HOME, "2026-06-04", context)).toEqual([
                expect.objectContaining({
                    what,
                    at: answeredAt,
                    tokens: 120,
                    micros: 1500,
                    ms: 900,
                }),
            ]);
            expect(await ledgerOf(home, HOME, "2026-06-05", context)).toEqual(
                [],
            );
            expect(
                await booksOf(home, HOME, "day", null, context),
            ).toMatchObject({
                hamlet: { tokens: 0, micros: 0, ms: 0 },
            });
            expect(
                await booksOf(home, HOME, "month", null, context),
            ).toMatchObject({
                hamlet: { tokens: 120, micros: 1500, ms: 900 },
            });
        },
    );

    it("keeps the three meters, task, host and end reason across a fork and restart", async () => {
        const dir = scratch();
        const at = new Date(2026, 5, 4, 12).getTime();
        const { models, faux } = metered(() => at);
        let home = await testHome(dir, {
            models,
            now: () => at,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        expect(await booksOf(home, HOME, "day", null, context)).toMatchObject({
            hamlet: { tokens: 0, micros: 0, ms: 0 },
        });
        faux.setResponses([fauxAssistantMessage("Yes.")]);
        await (await say(home, "Hello.")).wait(context);
        const chat = await mayorChat(home);
        const entries = await transcript(chat);
        const first = entries[0];
        if (first === undefined) throw new Error("missing chief line");
        const task = (first.data as { task: string }).task;
        expect(await ledgerOf(home, HOME, "2026-06-04", context)).toEqual([
            expect.objectContaining({
                what: "model",
                agent: 0,
                task,
                model: "faux::faux-1",
                host: "localhost",
                tokens: 120,
                micros: 1500,
                ms: 900,
                stopReason: "stop",
            }),
        ]);
        await chat.fork(
            first.id,
            { ownership: { kind: "ownerless" } },
            context,
        );
        await home.close(context);
        home = await testHome(dir, {
            models,
            now: () => at,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        expect(await booksOf(home, HOME, "day", task, context)).toMatchObject({
            hamlet: { tokens: 120, micros: 1500, ms: 900 },
            task: { tokens: 120, micros: 1500, ms: 900 },
        });
    });

    it("starts a day again at midnight, counts the month, and resets no task", async () => {
        let at = new Date(2026, 5, 4, 23).getTime();
        const { models, faux } = metered(() => at);
        const home = await testHome(scratch(), { models, now: () => at });
        faux.setResponses([
            fauxAssistantMessage("one"),
            fauxAssistantMessage("two"),
        ]);
        await (await say(home, "Yesterday.")).wait(context);
        const first = (await transcript(await mayorChat(home)))[0];
        if (first === undefined) throw new Error("missing chief line");
        const task = (first.data as { task: string }).task;
        at = new Date(2026, 5, 5, 1).getTime();
        expect(await booksOf(home, HOME, "day", task, context)).toMatchObject({
            hamlet: { micros: 0 },
            task: { micros: 1500 },
        });
        expect(await booksOf(home, HOME, "month", null, context)).toMatchObject(
            { hamlet: { micros: 1500 } },
        );
        expect(
            (await booksOf(home, HOME, "month", task, context)).agents,
        ).toEqual({
            "0": { tokens: 120, micros: 1500, ms: 900 },
        });
        await resetBudget(home, HOME, "chief", context);
        expect(await booksOf(home, HOME, "month", task, context)).toEqual({
            hamlet: { tokens: 0, micros: 0, ms: 0 },
            agents: {},
            task: { tokens: 120, micros: 1500, ms: 900 },
        });
        await (await say(home, "Today.")).wait(context);
        expect(await booksOf(home, HOME, "month", task, context)).toMatchObject(
            { hamlet: { micros: 1500 }, task: { micros: 1500 } },
        );
        at = new Date(2026, 6, 1, 1).getTime();
        expect(await booksOf(home, HOME, "month", task, context)).toMatchObject(
            { hamlet: { micros: 0 }, task: { micros: 1500 } },
        );
        expect(localDay(at)).toBe("2026-07-01");
    });

    it("charges a conversation and reads the realm without scanning unrelated conversations", async () => {
        const { models, faux } = metered();
        let scans = 0;
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
            openStorage: async (file) => {
                const storage = await openNodeSqliteStorage(file);
                const scan: Storage["scanConversations"] = (...args) => {
                    scans++;
                    return storage.scanConversations(...args);
                };
                return new Proxy(storage, {
                    get: (target, property) =>
                        property === "scanConversations"
                            ? scan
                            : Reflect.get(target, property, target),
                });
            },
        });
        for (let i = 0; i < 40; i++)
            await home.harness.createConversation(
                { ownership: { kind: "ownerless" } },
                context,
            );
        faux.setResponses([
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Two."),
        ]);
        await (await say(home, "First.")).wait(context);
        await followHome(home, context);
        scans = 0;

        await (await say(home, "Next.")).wait(context);
        await followHome(home, context);
        const books = await booksOf(home, HOME, "day", null, context);
        const state = await stateOf(home, { epoch: 1, revision: 0 }, context);

        expect(books.hamlet).toEqual({ tokens: 240, micros: 3000, ms: 1800 });
        expect(state.hamlets[0]?.budget.spent).toEqual({
            tokens: 240,
            micros: 3000,
            ms: 1800,
        });
        expect(state.agents[0]?.spent).toMatchObject({
            tokens: 240,
            micros: 3000,
            ms: 1800,
        });
        expect(scans).toBe(0);
    });

    it("loses no charge from eight conversations answering together", async () => {
        const { models, faux } = metered();
        const home = await testHome(scratch(), { models });
        faux.setResponses(
            Array.from({ length: 8 }, () => fauxAssistantMessage("Yes.")),
        );
        await Promise.all(
            Array.from({ length: 8 }, async (_, i) =>
                (await say(home, "Hi.", `chief-${i}`)).wait(context),
            ),
        );
        expect(await booksOf(home, HOME, "day", null, context)).toMatchObject({
            hamlet: { tokens: 960, micros: 12_000, ms: 7200 },
        });
    });

    it("charges summaries and failed attempts too", async () => {
        const { models, faux } = metered();
        const home = await testHome(scratch(), { models });
        faux.setResponses([
            fauxAssistantMessage("x".repeat(100_000)),
            fauxAssistantMessage("yes"),
            fauxAssistantMessage("Summary."),
            fauxAssistantMessage("", {
                stopReason: "error",
                errorMessage: "Bad request",
            }),
        ]);
        await (await say(home, "A long answer.")).wait(context);
        await (await say(home, "Next.")).wait(context);
        const chat = await mayorChat(home);
        const compact = await chat.compact(undefined, context);
        await home.harness.waitForTask(compact, context);
        await (await say(home, "Fail.")).wait(context);
        const lines = await ledgerOf(home, HOME, localDay(home.now()), context);
        expect(lines.map((line) => line.what)).toEqual([
            "model",
            "model",
            "summary",
            "attempt",
        ]);
        expect(await booksOf(home, HOME, "day", null, context)).toMatchObject({
            hamlet: { micros: 6000, ms: 2700 },
        });
    });
});

it("prices an unbilled response and estimates missing tokens", () => {
    const { models } = metered();
    const usage: Usage = {
        input: 100,
        output: 20,
        cacheRead: 0,
        cacheWrite: 0,
        totalTokens: 120,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    };
    expect(amountOf(usage, 900, models.getModel("faux", "faux-1"))).toEqual({
        tokens: 120,
        micros: 300,
        ms: 900,
    });
    expect(
        amountOf(
            { ...usage, input: 0, output: 0, totalTokens: 0 },
            5,
            models.getModel("faux", "faux-1"),
            7,
        ),
    ).toEqual({ tokens: 7, micros: 7, ms: 5 });
});

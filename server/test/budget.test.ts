import {
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it } from "vitest";
import { PayerDoc, runErrand, startErrand } from "../src/chats.ts";
import { LimitsDoc } from "../src/limits.ts";
import { told } from "../src/prompt.ts";
import { HOME } from "../src/record.ts";
import {
    canSpend,
    chargeRoad,
    reportOf,
    spendFailureText,
} from "../src/spend.ts";
import {
    addHamlet,
    context,
    fauxModels,
    MAYOR,
    mayorChat,
    request,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

describe("the budget", () => {
    it("holds the budget words fixed through tool rounds and changes them at the next turn", async () => {
        const { models, faux } = fauxModels(["faux-1"], 0.1);
        const home = await testHome(scratch(), { models });
        const seen: string[] = [];
        faux.setResponses([
            ...Array.from(
                { length: 3 },
                (): FauxResponseFactory => (input, _options, _state, model) => {
                    seen.push(
                        request(input.messages, model).sections.budget ??
                            "missing",
                    );
                    return fauxAssistantMessage(
                        fauxToolCall("read", { path: "NOTES.md" }),
                        { stopReason: "toolUse" },
                    );
                },
            ),
            (input, _options, _state, model) => {
                seen.push(
                    request(input.messages, model).sections.budget ?? "missing",
                );
                return fauxAssistantMessage("Done.");
            },
            (input, _options, _state, model) => {
                seen.push(
                    request(input.messages, model).sections.budget ?? "missing",
                );
                return fauxAssistantMessage("Next.");
            },
        ]);
        const chat = await mayorChat(home);
        const prompt = await told(home, chat, context);
        expect(prompt.includes("- mayor: the hamlet's own model, $0.00")).toBe(
            true,
        );
        expect((await reportOf(home, HOME, context)).lines).toEqual([
            "- mayor: faux::faux-1, $0.00",
        ]);
        await (await say(home, "Read.")).wait(context);
        await (await say(home, "Next.")).wait(context);
        expect(new Set(seen.slice(0, 4)).size).toBe(1);
        expect(seen[4]).not.toBe(seen[0]);
        expect(
            (await reportOf(home, HOME, context)).spent.micros,
        ).toBeGreaterThan(0);
    });

    it("reads a lowered cap at the next request and prevents summarizing after it is spent", async () => {
        const { models, faux } = fauxModels(["faux-1"], 1);
        const home = await testHome(scratch(), { models });
        faux.setResponses([
            fauxAssistantMessage("x".repeat(100_000)),
            async () => {
                await home.harness.commit(async (tx) => {
                    (await tx.doc(LimitsDoc, HOME, HOME)).budget.cost = 0;
                }, context);
                return fauxAssistantMessage(
                    fauxToolCall("read", { path: "NOTES.md" }),
                    { stopReason: "toolUse" },
                );
            },
            fauxAssistantMessage("Never sent."),
        ]);
        await (await say(home, "A long answer.")).wait(context);
        expect(
            await (await say(home, "Lower the cap.")).wait(context),
        ).toMatchObject({ status: "unanswered" });
        const compact = await (await mayorChat(home)).compact(
            undefined,
            context,
        );
        expect(
            (await home.harness.waitForTask(compact, context)).state.outcome,
        ).toMatchObject({ status: "completed", result: {} });
        expect(faux.state.callCount).toBe(2);
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).budget.cost = 5;
        }, context);
        expect(
            await (await say(home, "After a raise.")).wait(context),
        ).toMatchObject({ status: "done" });
    });

    it("bounds one task, not the chief's next message", async () => {
        const { models, faux } = fauxModels(["faux-1"], 1000);
        const home = await testHome(scratch(), { models });
        await home.harness.commit(async (tx) => {
            const limits = await tx.doc(LimitsDoc, HOME, HOME);
            limits.budget.cost = null;
            limits.task.cost = 0.01;
        }, context);
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("read", { path: "NOTES.md" }), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage("A new task answers."),
        ]);
        expect(
            await (await say(home, "Read your notes.")).wait(context),
        ).toMatchObject({ status: "unanswered" });
        expect(
            (await transcript(await mayorChat(home))).at(-1)?.data,
        ).toMatchObject({
            text: expect.stringContaining("This task has spent its cap"),
        });
        expect(
            await (await say(home, "Try a new task.")).wait(context),
        ).toMatchObject({ status: "done" });
    });

    it("stops a turn that has run out before its next request, saying why", async () => {
        const { models, faux } = fauxModels(["faux-1"], 1_000_000);
        const home = await testHome(scratch(), { models, now: () => 7 });
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).budget.cost = 0.01;
        }, context);
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("read", { path: "NOTES.md" }), {
                stopReason: "toolUse",
                timestamp: 7,
            }),
            fauxAssistantMessage("Never sent."),
        ]);

        const submission = await say(home, "Look at your notes.");
        const settled = await submission.wait(context);

        expect(faux.state.callCount).toBe(1);
        expect(settled).toMatchObject({
            status: "unanswered",
            reason: "aborted",
        });
        const entries = await transcript(await mayorChat(home));
        expect(entries.map((entry) => entry.kind)).toEqual([
            "hamlet.chief",
            "pi.system",
            "pi.assistant",
            "pi.tool-result",
            "hamlet.notice",
        ]);
        expect(entries.at(-1)?.data).toEqual({
            text: expect.stringMatching(/budget.*\$0\.01.*chief/),
            at: 7,
        });
    });
});

it("checks an errand's payer and live task against current caps, with the budget first", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models, now: () => 7 });
    await addHamlet(home, "mill", "miller");
    const id = await home.harness.commit(async (tx) => {
        const limits = await tx.doc(LimitsDoc, "mill", "mill");
        limits.budget.cost = 0.25;
        limits.task.cost = 0.1;
        limits.road.cost = 0.1;
        const id = await startErrand(home, tx, {
            agent: MAYOR,
            kind: "notes",
            own: false,
            instructions: "Join notes.",
            tools: [],
        });
        (await tx.doc(PayerDoc, id)).hamlet = "mill";
        return id;
    }, context);
    const read = {
        snapshot: home.harness.snapshot.bind(home.harness),
        snapshotAsOf: home.harness.snapshotAsOf.bind(home.harness),
        conversationId: id,
    };
    expect(await canSpend(home, read, {}, context)).toEqual({
        ok: true,
        value: undefined,
    });
    faux.setResponses([
        async () => {
            expect(await canSpend(home, read, {}, context)).toEqual({
                ok: true,
                value: undefined,
            });
            await home.harness.commit(async (tx) => {
                const limits = await tx.doc(LimitsDoc, "mill", "mill");
                await chargeRoad(tx, "mill", {
                    task: "shared-task",
                    agent: 0,
                    at: 7,
                    limits,
                });
            }, context);
            const task = await canSpend(home, read, {}, context);
            expect(task).toMatchObject({
                ok: false,
                error: {
                    kind: "task-spent",
                    hamlet: "mill",
                    task: "shared-task",
                    spent: 100_000,
                    cap: 100_000,
                },
            });
            if (task.ok) throw new Error("expected task cap");
            expect(spendFailureText(task.error)).toBe(
                "This task has spent its cap ($0.10 of $0.10). The chief can give the rest of the work as a new task, or raise the cap under One Task in the hamlet's limits.",
            );
            await home.harness.commit(async (tx) => {
                (await tx.doc(LimitsDoc, "mill", "mill")).budget.cost = 0.05;
            }, context);
            expect(await canSpend(home, read, {}, context)).toMatchObject({
                ok: false,
                error: {
                    kind: "budget-spent",
                    hamlet: "mill",
                    spent: 100_000,
                    cap: 50_000,
                    period: "day",
                },
            });
            expect(
                await canSpend(
                    home,
                    read,
                    { hamlet: HOME, task: null },
                    context,
                ),
            ).toEqual({ ok: true, value: undefined });
            return fauxAssistantMessage("joined");
        },
    ]);
    expect(
        await runErrand(
            home,
            id,
            {
                requestId: "payer-check",
                text: "Both notes",
                task: "shared-task",
            },
            context,
        ),
    ).toEqual({ status: "done", text: "joined" });
});

it("stops exactly at a model-time cap and renders the reached meter", async () => {
    const home = await testHome(scratch(), { now: () => 7 });
    const chat = await mayorChat(home);
    const read = {
        snapshot: home.harness.snapshot.bind(home.harness),
        snapshotAsOf: home.harness.snapshotAsOf.bind(home.harness),
        conversationId: chat.id,
    };
    await home.harness.commit(async (tx) => {
        const limits = await tx.doc(LimitsDoc, HOME, HOME);
        limits.budget.minutes = 1;
        limits.road.cost = 0;
        limits.road.minutes = 59_999 / 60_000;
        await chargeRoad(tx, HOME, { task: null, agent: 0, at: 7, limits });
    }, context);
    expect((await canSpend(home, read, {}, context)).ok).toBe(true);
    await home.harness.commit(async (tx) => {
        const limits = await tx.doc(LimitsDoc, HOME, HOME);
        limits.road.minutes = 1 / 60_000;
        await chargeRoad(tx, HOME, { task: null, agent: 0, at: 7, limits });
    }, context);
    const time = await canSpend(home, read, {}, context);
    if (time.ok) throw new Error("expected model time cap");
    expect(spendFailureText(time.error)).toContain(
        "1m model time of 1m model time",
    );
    await home.harness.commit(async (tx) => {
        const limits = await tx.doc(LimitsDoc, HOME, HOME);
        limits.budget.minutes = null;
        limits.budget.cost = 0.5;
        limits.road.cost = 0.5;
        await chargeRoad(tx, HOME, { task: null, agent: 0, at: 7, limits });
    }, context);
    const cost = await canSpend(home, read, {}, context);
    if (cost.ok) throw new Error("expected cost cap");
    expect(spendFailureText(cost.error)).toContain("$0.50 of $0.50");
});

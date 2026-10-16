import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { PayerDoc, runErrand, startErrand } from "../src/chats.ts";
import { localDay } from "../src/home/clock.ts";
import { booksOf, ledgerOf, reportOf } from "../src/spend.ts";
import {
    addHamlet,
    context,
    fauxModels,
    MAYOR,
    request,
    scratch,
    testHome,
} from "./support.ts";

it("runs bare and owned errands, charges the payer and task, and reuses the kept answer after reopening", async () => {
    const { models, faux } = fauxModels(["faux-1"], 10000);
    const dir = scratch();
    const home = await testHome(dir, { models });
    await addHamlet(home, "mill", "miller");
    const bare = await home.harness.commit(async (tx) => {
        const id = await startErrand(home, tx, {
            agent: MAYOR,
            kind: "notes",
            own: false,
            instructions: "Join the notes.",
            tools: [],
        });
        (await tx.doc(PayerDoc, id)).hamlet = "mill";
        return id;
    }, context);
    faux.setResponses([
        (prompt, _o, _s, model) => {
            const sent = request(prompt.messages, model);
            expect(sent.sections).toEqual({
                instructions:
                    "<instructions>\nJoin the notes.\n</instructions>",
            });
            expect(sent.tools).toEqual([]);
            return fauxAssistantMessage("joined");
        },
    ]);
    const input = {
        requestId: "one-notes",
        text: "A and B",
        task: "merge-task",
    };
    expect(await runErrand(home, bare, input, context)).toEqual({
        status: "done",
        text: "joined",
    });
    const books = await booksOf(home, "mill", "day", input.task, context);
    expect(books.hamlet.micros).toBeGreaterThan(0);
    expect(books.task).toEqual(books.hamlet);
    expect(books.agents.get("mill:0")?.micros ?? 0).toBe(0);
    expect(books.agents.get("home:0")).toEqual(books.hamlet);
    const report = await reportOf(home, "mill", context);
    expect(report.lines).toContain("- mayor: faux::faux-1, $0.00");
    expect(report.lines).toContain(
        "- mayor@home: $0.19 (paid for another hamlet)",
    );
    expect(
        (await booksOf(home, "home", "day", null, context)).hamlet.micros,
    ).toBe(0);
    expect(
        await ledgerOf(home, "mill", localDay(home.now()), context),
    ).toMatchObject([{ what: "model", of: "home", task: input.task }]);
    await home.close(context);
    const again = await testHome(dir, { models });
    expect(await runErrand(again, bare, input, context)).toEqual({
        status: "done",
        text: "joined",
    });
    expect(faux.state.callCount).toBe(1);
    const owned = await again.harness.commit(
        (tx) =>
            startErrand(again, tx, {
                agent: MAYOR,
                kind: "answer",
                own: true,
                instructions: "Answer once.",
                tools: [],
            }),
        context,
    );
    faux.setResponses([
        (prompt, _o, _s, model) => {
            const sent = request(prompt.messages, model);
            expect(sent.sections).toHaveProperty("charter");
            expect(sent.sections).toHaveProperty("notes");
            expect(sent.tools).toEqual([]);
            return fauxAssistantMessage("answer");
        },
    ]);
    expect(
        await runErrand(
            again,
            owned,
            { requestId: "answer", text: "Why?", task: "answer-task" },
            context,
        ),
    ).toMatchObject({ status: "done", text: "answer" });
});

it("returns a classified failure reason when an errand cannot answer", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const id = await home.harness.commit(
        (tx) =>
            startErrand(home, tx, {
                agent: MAYOR,
                kind: "notes",
                own: false,
                instructions: "Join notes.",
                tools: [],
            }),
        context,
    );
    faux.setResponses([
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Bad request",
        }),
    ]);
    expect(
        await runErrand(
            home,
            id,
            { requestId: "failed-notes", text: "A and B", task: "notes-task" },
            context,
        ),
    ).toEqual({ status: "failed", reason: "model_error" });
});

it("times out a waiting errand and aborts its conversation", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const id = await home.harness.commit(
        (tx) =>
            startErrand(home, tx, {
                agent: MAYOR,
                kind: "answer",
                own: true,
                instructions: "Answer once.",
                tools: [],
            }),
        context,
    );
    faux.setResponses([
        (_input, options) =>
            new Promise((_resolve, reject) => {
                options?.signal?.addEventListener(
                    "abort",
                    () => reject(new Error("aborted")),
                    { once: true },
                );
            }),
    ]);
    expect(
        await runErrand(
            home,
            id,
            {
                requestId: "slow-answer",
                text: "Why?",
                task: "slow-task",
                timeoutMs: 100,
            },
            context,
        ),
    ).toEqual({ status: "timeout", reason: "it did not answer in time" });
    expect(
        await runErrand(
            home,
            id,
            { requestId: "slow-answer", text: "Why?", task: "slow-task" },
            context,
        ),
    ).toEqual({ status: "aborted", reason: "aborted" });
});

import {
    type FauxResponseStep,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { describe, expect, it } from "vitest";
import { editDoc } from "../src/documents.ts";
import { openHome } from "../src/home.ts";
import { QuestionsDoc } from "../src/letters.ts";
import { RecordDoc } from "../src/record.ts";
import {
    context,
    crashing,
    fauxModels,
    scratch,
    transcript,
} from "./support.ts";

const model = { provider: "faux", modelId: "faux-1" };

const ask = fauxAssistantMessage(
    fauxToolCall("ask_agent", {
        agent: "tester",
        question: "Do the tests pass?",
    }),
    { stopReason: "toolUse" },
);

describe("a question between agents", () => {
    it("survives a restart while the asked agent works on it", async () => {
        let working = () => {};
        const started = new Promise<void>((resolve) => {
            working = resolve;
        });
        const blocked: FauxResponseStep = (_transcript, options) =>
            new Promise((_resolve, reject) => {
                working();
                options?.signal?.addEventListener("abort", () =>
                    reject(new Error("stopped")),
                );
            });
        const answer = await askAcrossRestart([ask, blocked], async (home) => {
            await started;
            await home.close(context);
        });
        expect(answer).toContain("Yes, all green.");
    });

    it("survives a restart after the asked agent answered", async () => {
        const answer = await askAcrossRestart(
            [ask, fauxAssistantMessage("Yes, all green.")],
            async (home) => {
                expect(await home.closed).toMatchObject({ reason: "failed" });
            },
            (writes) =>
                writes.some(
                    (write) =>
                        write.type === "entry" &&
                        write.value.kind === "pi.tool-result",
                ),
        );
        expect(answer).toContain("Yes, all green.");
    });
});

/**
 * The mayor asks `tester` a question, the server stops as `restart` says,
 * and opens again. Returns the result the mayor's `ask_agent` gave.
 */
async function askAcrossRestart(
    before: FauxResponseStep[],
    restart: (home: Awaited<ReturnType<typeof openHome>>) => Promise<void>,
    crashes?: Parameters<typeof crashing>[0],
): Promise<string> {
    const dir = scratch();
    const { models, faux } = fauxModels();
    const first = await openHome(
        dir,
        {
            models,
            openStorage: crashes ? crashing(crashes) : openNodeSqliteStorage,
        },
        context,
    );
    await editDoc(
        first,
        RecordDoc,
        { agents: [{ number: 1, name: "tester", purpose: "Runs the tests." }] },
        context,
    );
    const mayor = await first.root(context, { agent: { model } });
    faux.setResponses(before);
    const submission = await mayor.submit(
        { type: "input", content: "Ask the tester." },
        context,
    );
    await restart(first);

    faux.appendResponses([
        fauxAssistantMessage("Yes, all green."),
        fauxAssistantMessage("The tester says all green."),
    ]);
    const home = await openHome(dir, { models }, context);
    const settled = await (await home.submission(submission.id, context))?.wait(
        context,
    );
    expect(settled?.status).toBe("done");
    const asked = (await home.snapshot(QuestionsDoc, context))?.conversations[
        "tester:mayor"
    ];
    if (asked === undefined) throw new Error("tester was never asked");
    const questions = (await transcript(asked, home)).filter(
        (entry) => entry.kind === "pi.user",
    );
    expect(questions).toHaveLength(1);
    const result = (await transcript(mayor.id, home)).find(
        (entry) => entry.kind === "pi.tool-result",
    );
    await home.close(context);
    return JSON.stringify(result?.model);
}

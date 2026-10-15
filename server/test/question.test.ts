import {
    type FauxResponseStep,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { describe, expect, it } from "vitest";
import type { Home } from "../src/home.ts";
import { PeersDoc } from "../src/society/docs.ts";
import {
    context,
    crashing,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const create = fauxAssistantMessage(
    fauxToolCall("create_agent", {
        name: "tester",
        purpose: "Runs the tests.",
        reason: "Nobody runs them.",
    }),
    { stopReason: "toolUse" },
);

const ask = fauxAssistantMessage(
    fauxToolCall("ask_agent", {
        name: "tester",
        message: "Do the tests pass?",
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
        const answer = await askAcrossRestart(
            [create, ask, blocked],
            async (home) => {
                await started;
                await home.close(context);
            },
        );
        expect(answer).toContain("Yes, all green.");
    });

    it("survives a restart after the asked agent answered", async () => {
        const answer = await askAcrossRestart(
            [create, ask, fauxAssistantMessage("Yes, all green.")],
            async (home) => {
                expect(await home.harness.closed).toMatchObject({
                    reason: "failed",
                });
            },
            (writes) =>
                writes.some(
                    (write) =>
                        write.type === "entry" &&
                        write.value.kind === "pi.tool-result" &&
                        JSON.stringify(write.value.model).includes("all green"),
                ),
        );
        expect(answer).toContain("Yes, all green.");
    });
});

/**
 * The mayor creates `tester` and asks it a question, the server stops as `restart` says,
 * and opens again. Returns the result the mayor's `ask_agent` gave.
 */
async function askAcrossRestart(
    before: FauxResponseStep[],
    restart: (home: Home) => Promise<void>,
    crashes?: Parameters<typeof crashing>[0],
): Promise<string> {
    const dir = scratch();
    const { models, faux } = fauxModels();
    const first = await testHome(dir, {
        models,
        openStorage: crashes ? crashing(crashes) : openNodeSqliteStorage,
    });
    faux.setResponses(before);
    const submission = await say(first, "Ask a tester.");
    await restart(first);

    faux.appendResponses([
        fauxAssistantMessage("Yes, all green."),
        fauxAssistantMessage("The tester says all green."),
    ]);
    const home = await testHome(dir, { models });
    const settled = await (
        await home.harness.submission(submission.id, context)
    )?.wait(context);
    expect(settled?.status).toBe("done");
    const asked = (await home.harness.snapshot(PeersDoc, "home:1", context))
        ?.questions["home:0"];
    if (asked === undefined) throw new Error("tester was never asked");
    const questions = (await transcript(asked, home.harness)).filter(
        (entry) => entry.kind === "hamlet.question",
    );
    expect(questions).toHaveLength(1);
    const result = (await transcript(await mayorChat(home))).find(
        (entry) =>
            entry.kind === "pi.tool-result" &&
            JSON.stringify(entry.model).includes("`tester`:"),
    );
    return JSON.stringify(result?.model);
}

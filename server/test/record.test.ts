import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import type { StorageWrite } from "@earendil-works/pi-durable";
import { describe, expect, it } from "vitest";
import { HOME, RecordDoc } from "../src/record.ts";
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

/** The commit that makes a record tool's change, with its marker. */
const change = (writes: readonly StorageWrite[]) =>
    writes.some(
        (write) =>
            write.type === "document.create" &&
            write.record.kind === "hamlet.record-call",
    );

/** The commit that appends a tool's result. */
const result = (writes: readonly StorageWrite[]) =>
    writes.some(
        (write) =>
            write.type === "entry" && write.value.kind === "pi.tool-result",
    );

describe("a record tool", () => {
    it.each([
        ["before its change", change],
        ["between its change and its result", result],
    ])(
        "cut off %s makes the change once and gives its result",
        async (_when, crashes) => {
            const dir = scratch();
            const { models, faux } = fauxModels();
            faux.setResponses([
                fauxAssistantMessage(
                    fauxToolCall("create_agent", {
                        name: "tester",
                        purpose: "Runs the tests.",
                        reason: "Nobody runs them.",
                    }),
                    { stopReason: "toolUse" },
                ),
                fauxAssistantMessage("Done."),
            ]);
            const before = await testHome(dir, {
                models,
                openStorage: crashing(crashes),
            });
            const submission = await say(before, "Hire a tester.");
            expect(await before.harness.closed).toMatchObject({
                reason: "failed",
            });

            const after = await testHome(dir, { models });
            const settled = await (
                await after.harness.submission(submission.id, context)
            )?.wait(context);

            expect(settled?.status).toBe("done");
            const record = await after.harness.snapshot(
                RecordDoc,
                HOME,
                context,
            );
            expect(record?.agents.map((agent) => agent.name)).toEqual([
                "founder",
                "tester",
            ]);
            const results = (await transcript(await mayorChat(after))).filter(
                (entry) => entry.kind === "pi.tool-result",
            );
            expect(results).toHaveLength(1);
            expect(JSON.stringify(results[0]?.model)).toContain("tester");
            expect(JSON.stringify(results[0]?.model)).not.toContain(
                "interrupted",
            );
        },
    );

    it("stays safe to run again as an agent is given it", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models });
        const tool = (await (await mayorChat(home)).agent(context)).tools.find(
            (candidate) => candidate.name === "create_agent",
        );
        expect(tool?.replay).toBe("safe");
    });
});

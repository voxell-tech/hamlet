import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import type { StorageWrite } from "@earendil-works/pi-durable";
import { describe, expect, it } from "vitest";
import { openHome } from "../src/home.ts";
import { RecordDoc } from "../src/record.ts";
import {
    context,
    crashing,
    fauxModels,
    scratch,
    transcript,
} from "./support.ts";

const model = { provider: "faux", modelId: "faux-1" };

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
            const before = await openHome(
                dir,
                { models, openStorage: crashing(crashes) },
                context,
            );
            const root = await before.root(context, { agent: { model } });
            const submission = await root.submit(
                { type: "input", content: "Hire a tester." },
                context,
            );
            expect(await before.closed).toMatchObject({ reason: "failed" });

            const after = await openHome(dir, { models }, context);
            const settled = await (
                await after.submission(submission.id, context)
            )?.wait(context);

            expect(settled?.status).toBe("done");
            const record = await after.snapshot(RecordDoc, context);
            expect(record?.agents.map((agent) => agent.name)).toEqual([
                "tester",
            ]);
            const results = (await transcript(root.id, after)).filter(
                (entry) => entry.kind === "pi.tool-result",
            );
            expect(results).toHaveLength(1);
            expect(JSON.stringify(results[0]?.model)).toContain("tester");
            expect(JSON.stringify(results[0]?.model)).not.toContain(
                "interrupted",
            );
            await after.close(context);
        },
    );

    it("stays safe to run again as an agent is given it", async () => {
        const { models } = fauxModels();
        const home = await openHome(scratch(), { models }, context);
        const root = await home.root(context, { agent: { model } });
        const tool = (await root.agent(context)).tools.find(
            (candidate) => candidate.name === "create_agent",
        );
        expect(tool?.replay).toBe("safe");
        await home.close(context);
    });
});

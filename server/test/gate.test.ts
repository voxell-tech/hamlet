import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it, vi } from "vitest";
import { editDoc } from "../src/documents.ts";
import { answerWait, OversightDoc } from "../src/gate.ts";
import { WorkDoc } from "../src/hamlet.ts";
import { openHome } from "../src/home.ts";
import { context, fauxModels, scratch, transcript } from "./support.ts";

const model = { provider: "faux", modelId: "faux-1" };

describe("the gate", () => {
    it("keeps a command waiting for a chief across a restart", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        const before = await openHome(dir, { models }, context);
        await editDoc(
            before,
            WorkDoc,
            { folder: scratch(), mode: "manual" },
            context,
        );
        const root = await before.root(context, { agent: { model } });
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("bash", { command: "ls" }), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage("Done."),
        ]);
        const submission = await root.submit(
            { type: "input", content: "List the folder." },
            context,
        );
        await vi.waitFor(async () => {
            const waits = (await before.snapshot(OversightDoc, context))?.waits;
            expect(Object.values(waits ?? {})).toHaveLength(1);
        });
        await before.close(context);

        const after = await openHome(dir, { models }, context);
        const waits = (await after.snapshot(OversightDoc, context))?.waits;
        const [id, wait] = Object.entries(waits ?? {})[0] ?? [];
        expect(wait).toMatchObject({ tool: "bash", answer: null });
        await answerWait(after, id ?? "", "deny", context);
        const settled = await (
            await after.submission(submission.id, context)
        )?.wait(context);

        expect(settled?.status).toBe("done");
        const result = (await transcript(root.id, after)).find(
            (entry) => entry.kind === "pi.tool-result",
        );
        expect(JSON.stringify(result?.model)).toContain("A chief refused");
        expect(JSON.stringify(result?.model)).not.toContain("interrupted");
        expect(
            Object.values(
                (await after.snapshot(OversightDoc, context))?.waits ?? {},
            ),
        ).toHaveLength(1);
        await after.close(context);
    });
});

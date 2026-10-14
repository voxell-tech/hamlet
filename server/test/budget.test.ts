import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it } from "vitest";
import { LimitsDoc } from "../src/budget.ts";
import { editDoc } from "../src/documents.ts";
import { openHome } from "../src/home.ts";
import { context, fauxModels, scratch, transcript } from "./support.ts";

describe("the budget", () => {
    it("stops a turn that has run out before its next request, saying why", async () => {
        const { models, faux } = fauxModels(["faux-1"], 1_000_000);
        const home = await openHome(scratch(), { models }, context);
        await editDoc(home, LimitsDoc, { cost: 0.01 }, context);
        const root = await home.root(context, {
            agent: { model: { provider: "faux", modelId: "faux-1" } },
        });
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("read", { path: "NOTES.md" }), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage("Never sent."),
        ]);

        const submission = await root.submit(
            { type: "input", content: "Look at your notes." },
            context,
        );
        const settled = await submission.wait(context);

        expect(faux.state.callCount).toBe(1);
        expect(settled).toMatchObject({
            status: "unanswered",
            reason: "aborted",
        });
        const entries = await transcript(root);
        expect(entries.map((entry) => entry.kind)).toEqual([
            "pi.user",
            "pi.system",
            "pi.assistant",
            "pi.tool-result",
            "hamlet.notice",
        ]);
        expect(entries.at(-1)?.data).toMatchObject({
            text: expect.stringMatching(/budget.*\$0\.01.*chief/),
        });
        await home.close(context);
    });
});

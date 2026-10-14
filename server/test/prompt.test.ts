import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it } from "vitest";
import { DEFAULT_CHARTER, editCharter } from "../src/charter.ts";
import { openHome } from "../src/home.ts";
import {
    context,
    fauxModels,
    type Request,
    request,
    scratch,
} from "./support.ts";

describe("the prompt", () => {
    it("takes a chief's changes from the agent's next request in the same run", async () => {
        const { models, faux } = fauxModels(["small", "large"]);
        const home = await openHome(scratch(), { models }, context);
        const root = await home.root(context, {
            agent: { model: { provider: "faux", modelId: "small" } },
        });
        const read = (await root.agent(context)).tools.find(
            (tool) => tool.name === "read",
        );
        const seen: Request[] = [];
        faux.setResponses([
            async (transcript, _options, _state, model) => {
                seen.push(request(transcript.messages, model));
                await editCharter(home, "Be brief.", context);
                await root.configure(
                    {
                        model: { provider: "faux", modelId: "large" },
                        tools: read === undefined ? [] : [read],
                    },
                    context,
                );
                return fauxAssistantMessage(
                    fauxToolCall("read", { path: "NOTES.md" }),
                    { stopReason: "toolUse" },
                );
            },
            (transcript, _options, _state, model) => {
                seen.push(request(transcript.messages, model));
                return fauxAssistantMessage("Done.");
            },
        ]);

        const submission = await root.submit(
            { type: "input", content: "Look at your notes." },
            context,
        );
        expect((await submission.wait(context)).status).toBe("done");

        const [first, second] = seen;
        expect(first?.model).toBe("small");
        expect(first?.sections.charter).toContain(DEFAULT_CHARTER.trim());
        expect(first?.tools).toEqual(
            expect.arrayContaining(["read", "edit", "bash"]),
        );
        expect(second?.model).toBe("large");
        expect(second?.sections.charter).toContain("Be brief.");
        expect(second?.tools).toEqual(["read"]);
        await home.close(context);
    });
});

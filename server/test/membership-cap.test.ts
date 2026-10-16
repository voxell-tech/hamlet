import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { merge, remove, restore, undo } from "../src/agents.ts";
import { LimitsDoc } from "../src/limits.ts";
import { recordRefusalText } from "../src/record.ts";
import {
    addAgent,
    context,
    fauxModels,
    livingAgents,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const given = { hamlet: "home", chief: "chief", rest: "scout Finished" };
const full =
    "the hamlet is full (2 agents); remove one first, or raise mostAgents in the limits";

it("create_agent fits two agents beside the founder and refuses the third with its existing text", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).mostAgents = 2;
    }, context);
    for (const name of ["scout", "smith", "poet"]) {
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("create_agent", {
                    name,
                    purpose: "Works",
                    reason: "Needed",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done"),
        ]);
        await (await say(home, `Create ${name}`)).wait(context);
    }
    expect((await livingAgents(home)).map((a) => a.name)).toEqual([
        "founder",
        "scout",
        "smith",
    ]);
    const results = (await transcript(await mayorChat(home))).filter(
        (e) => e.kind === "pi.tool-result",
    );
    const texts = results.map((e) =>
        e.model
            ?.flatMap((m) =>
                m.role === "toolResult"
                    ? m.content.flatMap((c) =>
                          c.type === "text" ? [c.text] : [],
                      )
                    : [],
            )
            .join("\n"),
    );
    expect(texts).toEqual([
        "Created `scout`. Give it work with ask_agent.",
        "Created `smith`. Give it work with ask_agent.",
        "the hamlet is full (2 agents). Only the chief can raise mostAgents on the hamlet's Bylaws page",
    ]);
});

it.each([
    ["restore", restore],
    ["undo", undo],
] as const)(
    "/%s fits two agents beside the founder and refuses a third",
    async (_name, deed) => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        await addAgent(home, "home", "scout", "Maps");
        await addAgent(home, "home", "smith", "Builds");
        expect((await remove(home, given, context)).ok).toBe(true);
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, "home", "home")).mostAgents = 2;
        }, context);
        expect(
            (await deed(home, { ...given, rest: "scout" }, context)).ok,
        ).toBe(true);
        expect((await livingAgents(home)).map((a) => a.name)).toEqual([
            "founder",
            "smith",
            "scout",
        ]);
        expect((await remove(home, given, context)).ok).toBe(true);
        await addAgent(home, "home", "cook", "Cooks");
        const refused = await deed(home, { ...given, rest: "scout" }, context);
        expect(refused).toMatchObject({
            ok: false,
            error: { kind: "hamlet-full", most: 2 },
        });
        if (!refused.ok && refused.error.kind === "hamlet-full")
            expect(recordRefusalText(refused.error, "chief")).toBe(full);
    },
);

it.each([false, true])(
    "split counts only agents beside the mayor, with a full hamlet: %s",
    async (isFull) => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        await addAgent(home, "home", "scout", "Maps");
        await addAgent(home, "home", "smith", "Builds");
        expect(
            (
                await merge(
                    home,
                    { ...given, rest: "scout into smith Finished" },
                    context,
                )
            ).ok,
        ).toBe(true);
        if (isFull) await addAgent(home, "home", "cook", "Cooks");
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, "home", "home")).mostAgents = 2;
        }, context);
        const result = await restore(
            home,
            { ...given, rest: "scout" },
            context,
        );
        if (isFull) {
            expect(result).toMatchObject({
                ok: false,
                error: { kind: "hamlet-full", most: 2 },
            });
            if (!result.ok && result.error.kind === "hamlet-full")
                expect(recordRefusalText(result.error, "chief")).toBe(full);
        } else {
            expect(result).toMatchObject({
                ok: true,
                value: expect.stringContaining("Split `scout`"),
            });
            expect((await livingAgents(home)).map((a) => a.name)).toEqual([
                "founder",
                "smith",
                "scout",
            ]);
        }
    },
);

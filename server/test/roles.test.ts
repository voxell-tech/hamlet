import { Type } from "@earendil-works/pi-ai";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { defineExtension, defineTool } from "@earendil-works/pi-durable";
import { expect, it, vi } from "vitest";
import { runErrand, SOCIETY_TOOLS, startErrand } from "../src/chats.ts";
import { answer } from "../src/gate.ts";
import { LimitsDoc } from "../src/limits.ts";
import { reconfigure } from "../src/models.ts";
import { RecordDoc } from "../src/record.ts";
import {
    addAgent,
    chatWith,
    context,
    fauxModels,
    MAYOR,
    openWaits,
    request,
    say,
    scratch,
    testHome,
} from "./support.ts";

const tools = SOCIETY_TOOLS.map((name) =>
    defineTool({
        name,
        description: name,
        parameters: Type.Object({
            roads: Type.Optional(Type.Array(Type.String())),
        }),
        replay: "safe",
        execute: async () => ({ output: [{ type: "text", text: "done" }] }),
    }),
);
const dummy = () => ({
    extension: defineExtension({ name: "test.society", tools }),
});

it("offers tools by role, swaps offers on office changes, and confines a ballot to cast_ballot", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models, parts: [dummy] });
    const scout = await addAgent(home, "home", "scout");
    const run = async (ref: typeof MAYOR, mayor: boolean) => {
        faux.setResponses([
            (prompt, _o, _s, model) => {
                const offered = request(prompt.messages, model).tools;
                expect(offered.includes("create_agent")).toBe(mayor);
                expect(offered.includes("request_road")).toBe(!mayor);
                expect(offered).toContain("ask_chief");
                expect(offered).toContain("object");
                expect(offered).not.toContain("cast_ballot");
                return fauxAssistantMessage("done");
            },
        ]);
        const chat = await chatWith(home, ref);
        expect(
            (
                await (
                    await chat.submit(
                        { type: "input", content: "work" },
                        context,
                    )
                ).wait(context)
            ).status,
        ).toBe("done");
    };
    await run(MAYOR, true);
    await run(scout, false);
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, "home", "home")).mayor = scout.number;
        await reconfigure(home, tx);
    }, context);
    await run(MAYOR, false);
    await run(scout, true);
    const id = await home.harness.commit(
        (tx) =>
            startErrand(home, tx, {
                agent: scout,
                kind: "ballot",
                own: true,
                instructions: "Vote",
                tools: tools.filter((t) => t.name === "cast_ballot"),
            }),
        context,
    );
    faux.setResponses([
        (prompt, _o, _s, model) => {
            expect(request(prompt.messages, model).tools).toEqual([
                "cast_ballot",
            ]);
            return fauxAssistantMessage("yes");
        },
    ]);
    expect(
        await runErrand(
            home,
            id,
            { requestId: "ballot", text: "Cast", task: "vote-task" },
            context,
        ),
    ).toMatchObject({ status: "done" });
});

it.each([false, true])(
    "holds a reserved create_agent, including roads when only building is reserved: %s",
    async (roadsOnly) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models, parts: [dummy] });
        await home.harness.commit(async (tx) => {
            const l = await tx.doc(LimitsDoc, "home", "home");
            l.mayor.createAgents = roadsOnly;
            l.mayor.buildRoads = !roadsOnly;
        }, context);
        for (const choice of ["deny", "allow"] as const) {
            faux.setResponses([
                fauxAssistantMessage(
                    fauxToolCall("create_agent", { roads: ["scout"] }),
                    { stopReason: "toolUse" },
                ),
                fauxAssistantMessage("done"),
            ]);
            const sub = await say(home, "create");
            await vi.waitFor(async () =>
                expect(await openWaits(home)).toMatchObject([
                    { danger: "reserved", answers: ["allow", "deny"] },
                ]),
            );
            const wait = (await openWaits(home))[0];
            if (!wait) throw new Error("no wait");
            await answer(
                home,
                { wait: wait.id, choice, chief: home.chief },
                context,
            );
            expect((await sub.wait(context)).status).toBe("done");
        }
    },
);

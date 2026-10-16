import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { QuestionEntry, startConversation, submitLine } from "../src/chats.ts";
import type { Home } from "../src/home.ts";
import { raise } from "../src/oversight.ts";
import type { Address } from "../src/protocol.ts";
import { RealmDoc } from "../src/realm.ts";
import { say } from "../src/say.ts";
import {
    addAgent,
    addHamlet,
    context,
    fauxModels,
    MAYOR,
    openWaits,
    scratch,
    testHome,
} from "./support.ts";

it.each(["scout", "scout@mill", "mayor@mill"])(
    "stops %s from another hamlet's chat",
    async (address) => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), {
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const mayor = await addHamlet(home, "mill", "miller");
        const scout = await addAgent(home, "mill", "scout");
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("bash", { command: "true", network: true }),
                { stopReason: "toolUse" },
            ),
        ]);
        const sub = await submitLine(
            home,
            {
                agent: address.startsWith("mayor") ? mayor : scout,
                chief: home.chief,
            },
            { id: randomUUID(), text: "Work" },
            context,
        );
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        expect(await typed(home, `/stop ${address}`)).toMatchObject({
            kind: "notice",
            text: `Stopping ${address}.`,
        });
        expect((await sub.status(context)).status).toBe("unanswered");
        expect(await openWaits(home)).toEqual([]);
    },
);

it("refuses /stop for a hamlet the current chief does not lead", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await addHamlet(home, "mill", "miller");
    await home.harness.commit(async (tx) => {
        const mill = (await tx.doc(RealmDoc)).hamlets.find(
            (h) => h.id === "mill",
        );
        if (mill) mill.chiefs = ["another-chief"];
    }, context);
    expect(await typed(home, "/stop mayor@mill")).toMatchObject({
        kind: "error",
        text: expect.stringContaining("Only a chief"),
    });
});

function typed(
    home: Home,
    text: string,
    to: Address = { hamlet: "home", agent: "mayor" },
) {
    return say(home, { type: "say", id: randomUUID(), to, text }, context);
}

it("answers another hamlet's mayor's own question outside a chief chat with plain words", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const mayor = await addHamlet(home, "mill", "miller");
    await home.harness.commit(
        (tx) =>
            raise(
                tx,
                {
                    kind: "question",
                    danger: null,
                    by: "mayor@mill",
                    agent: mayor,
                    about: "Which grain?",
                    change: null,
                    answers: ["reply"],
                    chat: null,
                    call: null,
                },
                home.now(),
            ),
        context,
    );
    expect(
        await typed(home, "Barley", { hamlet: "mill", agent: "mayor" }),
    ).toMatchObject({ text: expect.stringContaining("Answered /reply") });
    expect(await openWaits(home)).toEqual([]);
});

it("holds plain words and routes /allow to the addressed mayor's real non-chief wait, not the older scout wait", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const scout = await addAgent(home, "home", "scout");
    const mayor = await addHamlet(home, "mill", "miller");
    await home.harness.commit(
        (tx) =>
            raise(
                tx,
                {
                    kind: "approval",
                    danger: "destructive",
                    by: "scout",
                    agent: scout,
                    about: "rm -rf build",
                    change: null,
                    answers: ["allow", "deny"],
                    chat: null,
                    call: null,
                },
                home.now(),
            ),
        context,
    );
    const id = await home.harness.commit(
        (tx) => startConversation(home, tx, { ...mayor, chief: null, chat: 0 }),
        context,
    );
    const conversation = await home.harness.conversation(id, context);
    if (!conversation) throw new Error("no conversation");
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "true", network: true }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done."),
    ]);
    const sub = await conversation.submit(
        {
            type: "input",
            content: "Work",
            entry: {
                kind: QuestionEntry.kind,
                data: { from: MAYOR, task: "mill-question", at: home.now() },
            },
        },
        context,
    );
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(2));
    const address = { hamlet: "mill", agent: "mayor" };
    expect
        .soft(await typed(home, "Go on", address))
        .toMatchObject({ text: expect.stringContaining("Not sent.") });
    expect(await typed(home, "/allow", address)).toMatchObject({
        text: expect.stringContaining("mayor@mill"),
    });
    expect((await sub.wait(context)).status).toBe("done");
    expect((await openWaits(home)).map((w) => w.by)).toEqual(["scout"]);
});

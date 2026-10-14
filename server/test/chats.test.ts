import { randomUUID } from "node:crypto";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it } from "vitest";
import {
    chatOf,
    reflectionInput,
    retryInput,
    submitLine,
    WithDoc,
} from "../src/chats.ts";
import { notify } from "../src/notice.ts";
import {
    CHIEF,
    context,
    fauxModels,
    MAYOR,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

describe("a chief's chat with the mayor", () => {
    it("keeps the chief's line, and is found again after reopening", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        faux.setResponses([fauxAssistantMessage("Hello, chief.")]);

        const settled = await (await say(home, "Hello.")).wait(context);

        expect(settled.status).toBe("done");
        const chat = await mayorChat(home);
        const [line] = await transcript(chat);
        expect(line).toMatchObject({
            kind: "hamlet.chief",
            data: {
                chief: CHIEF,
                text: "Hello.",
                task: expect.any(String),
                at: expect.any(Number),
            },
        });
        expect(line?.model).toEqual([
            expect.objectContaining({ role: "user", content: "Hello." }),
        ]);
        await home.close(context);

        const again = await testHome(dir, { models });
        expect((await mayorChat(again)).id).toBe(chat.id);
    });

    it("is the chief's own: another chief has a chat of its own", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models });

        const own = await mayorChat(home);
        const other = await chatOf(
            home,
            { agent: MAYOR, chief: "bea" },
            context,
        );

        expect(other.id).not.toBe(own.id);
        expect(await home.harness.snapshot(WithDoc, other.id, context)).toEqual(
            { hamlet: "home", number: 0, chief: "bea", chat: 1 },
        );
    });

    it("admits a line sent twice under the same id once", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        faux.setResponses([fauxAssistantMessage("Once.")]);
        const chat = { agent: MAYOR, chief: CHIEF };
        const line = { id: randomUUID(), text: "Only once." };

        const first = await submitLine(home, chat, line, context);
        const second = await submitLine(home, chat, line, context);
        await first.wait(context);

        expect(second.id).toBe(first.id);
        const lines = (await transcript(await mayorChat(home))).filter(
            (entry) => entry.kind === "hamlet.chief",
        );
        expect(lines).toHaveLength(1);
        expect(faux.state.callCount).toBe(1);
    });

    it("dates a notice, a retry and a reflection by the time they were made", async () => {
        let now = 1_000;
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models, now: () => now });
        faux.setResponses([
            fauxAssistantMessage("Carrying on."),
            fauxAssistantMessage("Noted."),
        ]);
        const chat = await mayorChat(home);

        now = 2_000;
        await home.harness.commit(
            (tx) => notify(tx, chat.id, "A notice.", home.now()),
            context,
        );
        now = 3_000;
        await (
            await chat.submit(retryInput("task-1", home.now()), context)
        ).wait(context);
        now = 4_000;
        await (
            await chat.submit(
                reflectionInput(1, 1, "Reflect.", "task-2", home.now()),
                context,
            )
        ).wait(context);

        const entries = await transcript(chat);
        expect(
            entries
                .filter((entry) => entry.kind.startsWith("hamlet."))
                .map((entry) => [entry.kind, entry.data]),
        ).toEqual([
            ["hamlet.notice", { text: "A notice.", at: 2_000 }],
            ["hamlet.retry", { task: "task-1", at: 3_000 }],
            ["hamlet.reflection", { task: "task-2", at: 4_000 }],
        ]);
    });
});

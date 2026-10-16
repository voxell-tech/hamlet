import { expect, it } from "vitest";
import { carryOn, current, past, resume, start } from "../src/chats.ts";
import { context, fauxModels, MAYOR, scratch, testHome } from "./support.ts";

it("owns session numbering, receipts and unanswered-turn refusals", async () => {
    const home = await testHome(scratch(), {
        models: fauxModels().models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    const chat = { agent: MAYOR, chief: home.chief };
    const first = await current(home, chat, context);
    expect(await carryOn(home, chat, context)).toMatchObject({
        ok: false,
        error: {
            kind: "nothing-to-carry-on",
            operation: "retry",
            retrySafe: true,
        },
    });
    expect(await past(home, chat, 99, context)).toMatchObject({
        ok: false,
        error: { kind: "no-past-session", number: 99 },
    });
    expect(await start(home, chat, context)).toEqual({
        ok: true,
        value: { text: "Started a new session." },
    });
    expect((await current(home, chat, context)).id).not.toBe(first.id);
    expect(await resume(home, chat, 1, context)).toEqual({
        ok: true,
        value: { text: "Went back to an earlier session." },
    });
    expect((await current(home, chat, context)).id).toBe(first.id);
    await home.follow.settled(context);
});

it("does not create a chat when refusing an unknown session", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const chat = { agent: MAYOR, chief: home.chief };
    expect(await resume(home, chat, 99, context)).toMatchObject({
        ok: false,
        error: { kind: "no-past-session" },
    });
    expect(await past(home, chat, 1, context)).toMatchObject({
        ok: false,
        error: { kind: "no-past-session" },
    });
});

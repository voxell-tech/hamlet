import { expect, it } from "vitest";
import { pageChat, requireWritable, watchChat } from "../src/transcript.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

it("returns watch and paging refusals without throwing or opening a watch", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const chat = { with: { hamlet: "home", agent: "mayor" } };
    expect(
        await watchChat(home, { ...chat, past: 99 }, () => {}, "open", context),
    ).toMatchObject({
        ok: false,
        error: { kind: "no-past-session", number: 99 },
    });
    expect(await pageChat(home, chat, "unknown", context)).toMatchObject({
        ok: false,
        error: { kind: "no-item", before: "unknown" },
    });
    expect(
        requireWritable({ ...chat, from: { hamlet: "home", number: 0 } }),
    ).toMatchObject({ ok: false, error: { kind: "read-only-peer" } });
});

it.each(["current", "past", "peer"] as const)(
    "faults with the farewell when opening a departed agent's %s conversation",
    async (kind) => {
        const { randomUUID } = await import("node:crypto");
        const { remove } = await import("../src/agents.ts");
        const { current } = await import("../src/chats.ts");
        const { request, served } = await import("./connection-support.ts");
        const { addAgent } = await import("./support.ts");
        const run = await served({
            models: fauxModels().models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        const scout = await addAgent(run.home, "home", "scout");
        await current(
            run.home,
            { agent: scout, chief: run.home.chief },
            context,
        );
        await remove(
            run.home,
            { hamlet: "home", chief: run.home.chief, rest: "scout Work ended" },
            context,
        );
        const id = randomUUID();
        expect(
            await request(run.connection, run.messages, {
                type: "open",
                id,
                chat: {
                    with: { hamlet: "home", agent: "scout" },
                    ...(kind === "past" ? { past: 1 } : {}),
                    ...(kind === "peer"
                        ? {
                              from: { hamlet: "home", number: 0 },
                              kind: "questions" as const,
                          }
                        : {}),
                },
            }),
        ).toEqual({
            type: "fault",
            to: id,
            text: '`scout` was removed from the hamlet by the chief chief ("Work ended"). Its house is archived and its roads went with it; only a chief can restore it.',
        });
    },
);

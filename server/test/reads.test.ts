import { randomUUID } from "node:crypto";
import { getSystemMessageText } from "@earendil-works/pi-ai";
import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { DEFAULT_CHARTER } from "../src/charter.ts";
import {
    changes,
    chat,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import { fauxModels, request as modelRequest } from "./support.ts";

it("reads the chief's edited papers and charter, shows exactly the next prompt, and lists conversations in history (C14, C24)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    for (const edit of [
        { doc: "charter" as const, text: "Be brief." },
        { doc: "character" as const, agent: mayor, text: "Calm." },
        { doc: "notes" as const, agent: mayor, text: "# Notes\nTea." },
    ])
        expect(
            await request(run.connection, run.messages, {
                type: "edit",
                id: randomUUID(),
                ...edit,
            }),
        ).toMatchObject({ type: "reply", kind: "notice" });
    const read = async (what: "prompt" | "papers" | "history") =>
        request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what,
            agent: mayor,
        });
    expect(await read("papers")).toMatchObject({
        type: "read",
        papers: { character: "Calm.", notes: "# Notes\nTea.", skills: [] },
    });
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what: "charter",
        }),
    ).toMatchObject({
        type: "read",
        charter: { text: "Be brief.", own: true },
    });
    const shown = await read("prompt");
    let sent: string | undefined;
    faux.setResponses([
        (input, _options, _state, model) => {
            const sections = modelRequest(input.messages, model).sections;
            sent = getSystemMessageText({
                role: "system",
                content: "",
                sections,
                timestamp: 1,
            });
            return fauxAssistantMessage("Done.");
        },
    ]);
    say(run.connection, "Hello.\nMore words.");
    await vi.waitFor(() => expect(sent).toBeDefined());
    expect(shown).toMatchObject({ prompt: sent });
    expect(sent).toContain("# Your character\nCalm.");
    expect(sent).toContain("# NOTES.md\n# Notes\nTea.");
    await vi.waitFor(async () =>
        expect(await read("history")).toMatchObject({
            type: "read",
            history: [
                {
                    number: 1,
                    current: true,
                    title: "Hello.",
                    by: "chief",
                    messages: 2,
                    tokens: expect.any(Number),
                },
            ],
        }),
    );
    expect(
        await request(run.connection, run.messages, {
            type: "new",
            id: randomUUID(),
            to: mayor,
        }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    expect(await read("history")).toMatchObject({
        history: [{ number: 1, current: false, messages: 2 }],
    });
    expect(
        await request(run.connection, run.messages, {
            type: "resume",
            id: randomUUID(),
            to: mayor,
            number: 1,
        }),
    ).toMatchObject({ kind: "notice" });
    expect(await read("history")).toMatchObject({
        history: [{ number: 1, current: true }],
    });
    expect(
        await request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "charter",
            text: DEFAULT_CHARTER,
        }),
    ).toMatchObject({ kind: "notice" });
    expect(
        await request(run.connection, run.messages, {
            type: "read",
            id: randomUUID(),
            what: "charter",
        }),
    ).toMatchObject({ charter: { own: false, text: DEFAULT_CHARTER } });
    expect(changes(run.messages).length).toBeGreaterThan(0);
    expect(chat).toEqual({ with: mayor });
});

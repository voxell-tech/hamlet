import { randomUUID } from "node:crypto";
import { expect, it, vi } from "vitest";
import { ChatDoc, chiefInput } from "../src/chats.ts";
import { scriptedModels } from "../src/faux.ts";
import { notify } from "../src/notice.ts";
import type { Item, ServerMessage, UsageView } from "../src/protocol.ts";
import {
    chat,
    fold,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import { context, mayorChat } from "./support.ts";

function held(messages: ServerMessage[]) {
    const items: Item[] = [];
    let usage: UsageView | undefined;
    const status = { working: false, pending: 0 };
    for (const message of messages) {
        if (message.type === "transcript") {
            items.splice(0, items.length, ...structuredClone(message.items));
            usage = message.usage;
            status.working = message.working;
        }
        if (message.type !== "turn") continue;
        for (const op of message.ops) {
            fold(items, status, op);
            if (op.op === "usage") usage = op.usage;
        }
    }
    return { items, usage, working: status.working };
}

async function opened(run: Awaited<ReturnType<typeof served>>) {
    const message = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    if (message.type !== "transcript")
        throw new Error(`No transcript: ${JSON.stringify(message)}`);
    return message;
}

it("streams tool exits, failures, changes and usage exactly as they read back, including reflection (C30, C31)", async () => {
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "1" },
        models: scriptedModels({
            tokensPerSecond: 200,
            responses: [
                {
                    calls: [
                        {
                            name: "write",
                            arguments: { path: "a.txt", content: "one\n" },
                        },
                    ],
                },
                {
                    calls: [
                        {
                            name: "edit",
                            arguments: {
                                path: "a.txt",
                                edits: [{ oldText: "missing", newText: "two" }],
                            },
                        },
                        {
                            name: "bash",
                            arguments: { command: "echo failed; exit 7" },
                        },
                    ],
                },
                "Hello there.",
                "Remembered.",
            ],
        }),
    });
    await opened(run);
    say(run.connection, "Make a file.");
    await vi.waitFor(
        () => {
            const live = held(run.messages);
            expect(live.items.at(-1)).toMatchObject({
                kind: "assistant",
                text: "Remembered.",
            });
            expect(live.working).toBe(false);
            expect(
                live.items.filter((item) => item.kind === "tool"),
            ).toMatchObject([
                {
                    tool: {
                        name: "write",
                        exit: 0,
                        change: {
                            path: "a.txt",
                            created: true,
                            added: 1,
                            removed: 0,
                        },
                    },
                },
                {
                    tool: {
                        name: "edit",
                        exit: 1,
                        failure: expect.stringContaining("Could not find"),
                        change: { added: 1, removed: 1 },
                    },
                },
                { tool: { name: "bash", exit: 7, failure: "failed" } },
            ]);
            expect(live.usage).toMatchObject({ calls: 4 });
        },
        { timeout: 10_000 },
    );
    const before = held(run.messages);
    const readBack = await opened(run);
    expect(readBack.items).toEqual(before.items);
    expect(readBack.usage).toEqual(before.usage);
    expect(readBack.items.every((item) => item.at > 0)).toBe(true);
    expect(
        readBack.items.filter((item) => item.text === "reflection"),
    ).toHaveLength(1);
    expect(
        readBack.items.filter((item) => item.kind === "assistant"),
    ).toHaveLength(2);
}, 20_000);

it("resets an open chat after rewind or new, gives back the chief's words, and never reuses a kept id (C27)", async () => {
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({ responses: ["First.", "Second.", "Again."] }),
    });
    await opened(run);
    say(run.connection, "one");
    say(run.connection, "two");
    await vi.waitFor(() =>
        expect(held(run.messages).items.at(-1)?.text).toBe("Second."),
    );
    const before = held(run.messages).items;
    const target = before.find((item) => item.text === "two");
    expect(target?.rewindable).toBe(true);
    const rewinding = run.messages.length;
    expect(
        await request(run.connection, run.messages, {
            type: "rewind",
            id: randomUUID(),
            chat,
            item: target?.id ?? "missing",
        }),
    ).toMatchObject({ type: "reply", kind: "rewound", text: "two" });
    await vi.waitFor(() =>
        expect(held(run.messages).items.map((item) => item.text)).toEqual([
            "one",
            "First.",
        ]),
    );
    const rebound = run.messages.slice(rewinding);
    const reset = rebound.findIndex(
        (message) =>
            message.type === "turn" &&
            message.ops.some((op) => op.op === "reset"),
    );
    expect(reset).toBeGreaterThanOrEqual(0);
    expect(rebound[reset + 1]).toMatchObject({ type: "transcript", chat });
    say(run.connection, "again");
    await vi.waitFor(() =>
        expect(held(run.messages).items.at(-1)?.text).toBe("Again."),
    );
    const after = held(run.messages).items;
    expect(after.slice(0, 2)).toEqual(before.slice(0, 2));
    expect(
        after
            .slice(2)
            .every((item) => !before.some((old) => old.id === item.id)),
    ).toBe(true);
    const reread = await opened(run);
    expect(reread.items).toEqual(after);
    expect(reread.usage.calls).toBe(3);
    expect(
        await request(run.connection, run.messages, {
            type: "new",
            id: randomUUID(),
            to: mayor,
        }),
    ).toMatchObject({ kind: "notice" });
    await vi.waitFor(() =>
        expect(held(run.messages).items).toMatchObject([
            { kind: "notice", text: "Started a new session." },
        ]),
    );
    expect(
        await request(run.connection, run.messages, {
            type: "resume",
            id: randomUUID(),
            to: mayor,
            number: 1,
        }),
    ).toMatchObject({ kind: "notice" });
    await vi.waitFor(() =>
        expect(held(run.messages).items.map((item) => item.text)).toEqual([
            "one",
            "First.",
            "again",
            "Again.",
            "Went back to an earlier session.",
        ]),
    );
});

it("replaces a streamed answer even when a notice arrives between its chunks", async () => {
    const text = "one two three four five ".repeat(8).trim();
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({ tokensPerSecond: 100, responses: [text] }),
    });
    await opened(run);
    say(run.connection, "Speak slowly.");
    await vi.waitFor(() =>
        expect(
            held(run.messages).items.some(
                (item) => item.id.startsWith("live:") && item.text.length > 0,
            ),
        ).toBe(true),
    );
    const conversation = await mayorChat(run.home);
    await run.home.harness.commit(
        (tx) => notify(tx, conversation.id, "Still here.", run.home.now()),
        context,
    );
    await vi.waitFor(
        () => {
            expect(held(run.messages).working).toBe(false);
            expect(held(run.messages).items.at(-1)?.text).toBe(text);
        },
        { timeout: 10_000 },
    );
    const live = held(run.messages);
    const reread = await opened(run);
    expect(live.items).toEqual(reread.items);
}, 15_000);

it("pages the transcript in forty-item windows with an exclusive older boundary", async () => {
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({
            responses: Array.from({ length: 23 }, () => "Done."),
        }),
    });
    const conversation = await mayorChat(run.home);
    for (let number = 1; number <= 23; number++) {
        const submitted = await conversation.submit(
            chiefInput(
                run.home.chief,
                { id: randomUUID(), text: `Line ${number}.` },
                randomUUID(),
                run.home.now(),
            ),
            context,
        );
        await submitted.wait(context);
    }
    const latest = await opened(run);
    expect(latest.items).toHaveLength(40);
    expect(latest.items[0]?.text).toBe("Line 4.");
    expect(latest.earlier).toBe(latest.items[0]?.id);
    expect(
        await request(run.connection, run.messages, {
            type: "open",
            id: randomUUID(),
            chat,
            before: latest.earlier ?? "missing",
        }),
    ).toMatchObject({
        type: "transcript",
        earlier: null,
        items: [
            expect.objectContaining({ text: "Line 1." }),
            expect.objectContaining({ text: "Done." }),
            expect.objectContaining({ text: "Line 2." }),
            expect.objectContaining({ text: "Done." }),
            expect.objectContaining({ text: "Line 3." }),
            expect.objectContaining({ text: "Done." }),
        ],
    });
    expect(
        await request(run.connection, run.messages, {
            type: "open",
            id: randomUUID(),
            chat,
            before: "unknown",
        }),
    ).toMatchObject({
        type: "fault",
        text: "There is no item `unknown` here.",
    });
});

it("keeps credentials out of chief entries and out of answers split across streamed chunks (S7)", async () => {
    const secret = "sk-chunked-very-long-secret-value";
    const tail = " Again.".repeat(10);
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0", HAMLET_API_KEY: secret },
        models: scriptedModels({
            tokensPerSecond: 40,
            responses: [`Keep ${secret} private.${tail}`],
        }),
    });
    await opened(run);
    say(run.connection, `Chief ${secret}.`);
    await vi.waitFor(
        () =>
            expect(held(run.messages).items.at(-1)?.text).toBe(
                `Keep [REDACTED] private.${tail}`,
            ),
        { timeout: 5000 },
    );
    const chunks = new Map<string, string>();
    for (const message of run.messages) {
        if (message.type !== "turn") continue;
        for (const op of message.ops) {
            if (op.op === "add") chunks.set(op.item.id, op.item.text);
            if (op.op === "append")
                chunks.set(op.item, (chunks.get(op.item) ?? "") + op.text);
            expect([...chunks.values()].join("\n")).not.toContain(secret);
        }
    }
    const conversation = await mayorChat(run.home);
    const entries = await conversation.entries(
        { order: "ascending" },
        100,
        undefined,
        context,
    );
    expect(
        entries.items.find((entry) => entry.kind === "hamlet.chief")?.data,
    ).toMatchObject({ text: "Chief [REDACTED]." });
}, 10_000);

it("keeps the chat live after an older page is refused (S5)", async () => {
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({ responses: [] }),
    });
    await opened(run);
    expect(
        await request(run.connection, run.messages, {
            type: "open",
            id: randomUUID(),
            chat,
            before: "unknown",
        }),
    ).toMatchObject({
        type: "fault",
        text: "There is no item `unknown` here.",
    });
    const conversation = await mayorChat(run.home);
    await run.home.harness.commit(
        (tx) => notify(tx, conversation.id, "Still watched.", run.home.now()),
        context,
    );
    await vi.waitFor(() =>
        expect(held(run.messages).items.at(-1)?.text).toBe("Still watched."),
    );
});

it("sends last activity live and on late opens, keeping notices from replacing it (S5)", async () => {
    const run = await served({
        env: { HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({
            responses: [
                {
                    calls: [
                        { name: "bash", arguments: { command: "echo inside" } },
                    ],
                },
                "Done.",
            ],
        }),
    });
    await opened(run);
    say(run.connection, "Work.");
    await vi.waitFor(() => {
        expect(held(run.messages).working).toBe(false);
        expect(held(run.messages).items.at(-1)?.text).toBe("Done.");
    });
    const ops = run.messages.flatMap((message) =>
        message.type === "turn" ? message.ops : [],
    );
    const tool = held(run.messages).items.find((item) => item.kind === "tool");
    const reply = held(run.messages).items.at(-1);
    expect(ops).toContainEqual(
        expect.objectContaining({
            op: "status",
            last: { what: "tool bash", at: tool?.at },
        }),
    );
    expect(ops).toContainEqual(
        expect.objectContaining({
            op: "status",
            last: { what: "replied", at: reply?.at },
        }),
    );
    const conversation = await mayorChat(run.home);
    await run.home.harness.commit(
        (tx) => notify(tx, conversation.id, "A notice.", run.home.now()),
        context,
    );
    expect(await opened(run)).toMatchObject({
        last: { what: "replied", at: reply?.at },
    });
});

it("reads back the last error after a failed turn", async () => {
    const { fauxModels } = await import("./support.ts");
    const { fauxAssistantMessage } = await import(
        "@earendil-works/pi-ai/providers/faux"
    );
    const { models, faux } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "Faux failure.",
        }),
    ]);
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await opened(run);
    say(run.connection, "Fail.");
    await vi.waitFor(() =>
        expect(held(run.messages).items.at(-1)?.kind).toBe("error"),
    );
    const error = held(run.messages).items.at(-1);
    expect(await opened(run)).toMatchObject({
        last: { what: "error", at: error?.at },
    });
});

it("never loses a step committed while an older page is read (S5, C30)", async () => {
    const run = await served({ models: scriptedModels({ responses: [] }) });
    const conversation = await mayorChat(run.home);
    await run.home.harness.commit(async (tx) => {
        for (let n = 0; n < 46; n++)
            await notify(tx, conversation.id, `Earlier ${n}.`, run.home.now());
    }, context);
    const latest = await opened(run);
    let committed = false;
    const commit = async () => {
        if (committed) return;
        committed = true;
        await run.home.harness.commit(
            (tx) => notify(tx, conversation.id, "Between.", run.home.now()),
            context,
        );
    };
    const watchDoc = run.home.harness.watchDoc.bind(run.home.harness);
    const watching = vi
        .spyOn(run.home.harness, "watchDoc")
        .mockImplementation(async (...args) => {
            if (Object.is(args[0], ChatDoc)) await commit();
            return watchDoc(...args);
        });
    try {
        await request(run.connection, run.messages, {
            type: "open",
            id: randomUUID(),
            chat,
            before: latest.earlier ?? "missing",
        });
        await commit();
        await vi.waitFor(() =>
            expect(
                run.messages.flatMap((message) =>
                    message.type === "turn" ? message.ops : [],
                ),
            ).toContainEqual(
                expect.objectContaining({
                    op: "add",
                    item: expect.objectContaining({ text: "Between." }),
                }),
            ),
        );
    } finally {
        watching.mockRestore();
    }
});

it("keeps a tool call as last activity when the same response explains the call", async () => {
    const { fauxAssistantMessage, fauxToolCall } = await import(
        "@earendil-works/pi-ai/providers/faux"
    );
    const { fauxModels } = await import("./support.ts");
    const { models, faux } = fauxModels();
    let finish: (() => void) | undefined;
    faux.setResponses([
        fauxAssistantMessage(
            [
                fauxToolCall("bash", { command: "true" }),
                { type: "text", text: "I will run this." },
            ],
            { stopReason: "toolUse" },
        ),
        async () => {
            await new Promise<void>((resolve) => {
                finish = resolve;
            });
            return fauxAssistantMessage("Done.");
        },
    ]);
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await opened(run);
    say(run.connection, "Work.");
    try {
        await vi.waitFor(() => expect(finish).toBeDefined());
        const snapshot = await opened(run);
        expect(snapshot.last?.what).toBe("tool bash");
    } finally {
        finish?.();
    }
});

import { randomUUID } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import {
    changes,
    chat,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import { fauxModels, scratch, setFolder } from "./support.ts";

it("answers typed commands, usage errors and held words over the connection", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    for (const [text, kind, words] of [
        ["/queue", "notice", "Nothing waits for you."],
        ["/mode fast", "error", "usage: /mode <manual|auto>"],
    ]) {
        expect(
            await request(run.connection, run.messages, {
                type: "say",
                id: randomUUID(),
                to: mayor,
                text: text ?? "",
            }),
        ).toMatchObject({ type: "reply", kind, text: words });
    }
    await setFolder(run.home, scratch());
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Done."),
    ]);
    say(run.connection, "Run.");
    await vi.waitFor(() =>
        expect(changes(run.messages).some((item) => item.type === "wait")).toBe(
            true,
        ),
    );
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "More words.",
        }),
    ).toMatchObject({
        type: "reply",
        kind: "notice",
        text: expect.stringContaining(
            "Not sent. mayor is waiting for your answer:",
        ),
    });
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "/allow",
        }),
    ).toMatchObject({
        type: "reply",
        text: "Answered /allow: mayor wants to run `echo held`",
    });
    expect(changes(run.messages)).toContainEqual(
        expect.objectContaining({ type: "wait-closed", how: "allow" }),
    );
}, 15_000);

it("scrubs a protocol refusal before the model or the wait record reads it (C32)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({
        models,
        env: { HAMLET_REFLECT_EVERY: "0", HAMLET_API_KEY: "secret-in-reason" },
    });
    await setFolder(run.home, scratch());
    let next = "";
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        (input) => {
            next = JSON.stringify(input.messages);
            return fauxAssistantMessage("Done.");
        },
    ]);
    say(run.connection, "Run.");
    await vi.waitFor(() =>
        expect(changes(run.messages).some((item) => item.type === "wait")).toBe(
            true,
        ),
    );
    const held = changes(run.messages).find((item) => item.type === "wait");
    if (held?.type !== "wait") throw new Error("No wait");
    await request(run.connection, run.messages, {
        type: "answer",
        id: randomUUID(),
        wait: held.wait.id,
        choice: "deny",
        text: "use secret-in-reason instead",
    });
    await vi.waitFor(() =>
        expect(next).toContain("the chief refused it: use [REDACTED] instead"),
    );
    expect(next).not.toContain("secret-in-reason");
});

it("answers stop immediately while the same client's model hello is pending (H23)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models });
    let release: (() => void) | undefined;
    faux.setResponses([
        async () => {
            await new Promise<void>((resolve) => {
                release = resolve;
            });
            return fauxAssistantMessage("Hello.");
        },
    ]);
    const id = randomUUID();
    run.connection.send({ type: "check", id, what: "model" });
    await vi.waitFor(() => expect(release).toBeDefined());
    const stop = randomUUID();
    run.connection.send({ type: "stop", id: stop });
    try {
        await vi.waitFor(() =>
            expect(run.messages).toContainEqual({
                type: "reply",
                to: stop,
                kind: "notice",
                text: "Stopping everything.",
            }),
        );
        expect(
            run.messages.some(
                (message) => "to" in message && message.to === id,
            ),
        ).toBe(false);
    } finally {
        release?.();
    }
    await vi.waitFor(() =>
        expect(run.messages).toContainEqual({
            type: "reply",
            to: id,
            kind: "notice",
            text: "Hello.",
        }),
    );
});

it("refuses typed lines to an unfounded hamlet before a command or model turn", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    faux.setResponses([fauxAssistantMessage("Never called.")]);
    for (const text of [
        "/mode auto",
        `/folder ${scratch()}`,
        "/reset-budget",
        "hello",
    ]) {
        expect(
            await request(run.connection, run.messages, {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "nowhere", agent: "mayor" },
                text,
            }),
        ).toMatchObject({
            type: "reply",
            kind: "error",
            text: "Cannot say: no hamlet named `nowhere`. Choose a living hamlet.",
        });
    }
    expect(faux.state.callCount).toBe(0);
});

it("shows a binary overwrite as a replacement in its wait and kept transcript (C30)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const folder = scratch();
    writeFileSync(join(folder, "a.bin"), Buffer.from([255, 254, 0, 129, 130]));
    await setFolder(run.home, folder);
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("write", { path: "a.bin", content: "text" }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done."),
    ]);
    await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    say(run.connection, "Replace the file.");
    await vi.waitFor(() =>
        expect(changes(run.messages).some((item) => item.type === "wait")).toBe(
            true,
        ),
    );
    const held = changes(run.messages).find((item) => item.type === "wait");
    if (held?.type !== "wait") throw new Error("No wait");
    const change = {
        path: "a.bin",
        created: false,
        lines: [
            { mark: "-", text: "[a file that is not UTF-8 text, 5 bytes]" },
            { mark: "+", text: "text" },
        ],
        total: 2,
        added: 1,
        removed: 1,
        count: "1 line added, 1 removed",
    };
    expect(held.wait.change).toEqual(change);
    const added = run.messages
        .flatMap((message) => (message.type === "turn" ? message.ops : []))
        .find((op) => op.op === "add" && op.item.kind === "tool");
    expect(added).toMatchObject({ item: { tool: { change: null } } });
    await request(run.connection, run.messages, {
        type: "answer",
        id: randomUUID(),
        wait: held.wait.id,
        choice: "allow",
    });
    await vi.waitFor(() =>
        expect(
            run.messages.flatMap((message) =>
                message.type === "turn" ? message.ops : [],
            ),
        ).toContainEqual(
            expect.objectContaining({ op: "close", exit: 0, change }),
        ),
    );
    const readBack = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    expect(readBack).toMatchObject({
        type: "transcript",
        items: expect.arrayContaining([
            expect.objectContaining({
                kind: "tool",
                tool: expect.objectContaining({ change }),
            }),
        ]),
    });
});

it("never broadcasts a spoken credential to clients that did not open the chat (S7)", async () => {
    const secret = "secret-spoken-key";
    const { models, faux } = fauxModels();
    const run = await served({
        models,
        env: { HAMLET_REFLECT_EVERY: "0", HAMLET_API_KEY: secret },
    });
    faux.setResponses([fauxAssistantMessage(`The key is ${secret}.`)]);
    say(run.connection, "Speak.");
    await vi.waitFor(() =>
        expect(changes(run.messages)).toContainEqual(
            expect.objectContaining({
                type: "agent",
                agent: expect.objectContaining({
                    busy: false,
                    saying: { text: "The key is [REDACTED].", thought: false },
                }),
            }),
        ),
    );
    expect(JSON.stringify(run.messages)).not.toContain(secret);
});

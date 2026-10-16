import { randomUUID } from "node:crypto";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import {
    agentConversation,
    conversationViews,
    requireWritable,
} from "../src/agent-chats.ts";
import { QuestionEntry, submitLine } from "../src/chats.ts";
import type { ChatRef } from "../src/protocol.ts";
import { request, served } from "./connection-support.ts";
import {
    addAgent,
    context,
    fauxAgents,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

it("resolves a read-only peer conversation without opening a chief conversation", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, "home", "scout");
    const smith = await addAgent(home, "home", "smith");
    await home.credentials.modify("test", async () => ({
        type: "api_key",
        key: "secret-letter-credential",
    }));
    fauxAgents(faux, {
        "home:2": [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", {
                    name: "mayor",
                    message: "secret-letter-credential",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ],
        "home:0": [fauxAssistantMessage("answer")],
    });
    const sub = await submitLine(
        home,
        { agent: smith, chief: home.chief },
        { id: "ask", text: "Ask mayor" },
        context,
    );
    await sub.wait(context);
    const chat = {
        with: { hamlet: "home", agent: "mayor" },
        from: smith,
        kind: "questions" as const,
    };
    const conversation = await agentConversation(home, chat, context);
    expect(JSON.stringify(await transcript(conversation))).toContain("answer");
    expect(JSON.stringify(await transcript(conversation))).not.toContain(
        "secret-letter-credential",
    );
    expect(
        await conversationViews(home, { hamlet: "home", number: 0 }, context),
    ).toEqual([
        { with: smith, kind: "questions", count: 1, last: expect.any(Number) },
    ]);
    expect(() => requireWritable(chat)).toThrow("read-only");
    expect(() => requireWritable({ with: chat.with })).not.toThrow();
    await expect(
        agentConversation(home, { ...chat, from: scout }, context),
    ).rejects.toThrow("no conversation");
});

it("refuses a peer watch by a chief of neither agent", async () => {
    const { RealmDoc } = await import("../src/realm.ts");
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    await addAgent(home, "home", "scout");
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", { name: "scout", message: "Check" }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("done"),
        ],
        "home:1": [fauxAssistantMessage("answer")],
    });
    await (await say(home, "Ask")).wait(context);
    await home.harness.commit(async (tx) => {
        for (const h of (await tx.doc(RealmDoc)).hamlets)
            h.chiefs = ["another-chief"];
    }, context);
    await expect(
        agentConversation(
            home,
            {
                with: { hamlet: "home", agent: "scout" },
                from: { hamlet: "home", number: 0 },
                kind: "questions",
            },
            context,
        ),
    ).rejects.toThrow("Only a chief");
});

it("watches questions live, pages them and refuses rewind without changing the chief's chat", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    const scout = await addAgent(run.home, "home", "scout");
    await run.home.credentials.modify("test", async () => ({
        type: "api_key",
        key: "peer-secret-credential",
    }));
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", {
                    name: "scout",
                    message: "peer-secret-credential",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done"),
        ],
        "home:1": [
            fauxAssistantMessage("First answer peer-secret-credential"),
            ...Array.from({ length: 22 }, (_, i) =>
                fauxAssistantMessage(`Answer ${i + 2}`),
            ),
        ],
    });
    await (await say(run.home, "Ask")).wait(context);
    const chat: ChatRef = {
        with: { hamlet: "home", agent: "scout" },
        from: { hamlet: "home", number: 0 },
        kind: "questions",
    };
    const open = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    expect(open.type).toBe("transcript");
    if (open.type !== "transcript") throw new Error("No peer transcript");
    expect(open.items.map((i) => i.kind)).toEqual(["user", "assistant"]);
    expect(open.items[0]).toMatchObject({ author: "founder@home" });
    expect(open.items[0]?.rewindable).not.toBe(true);
    expect(JSON.stringify(open)).not.toContain("peer-secret-credential");
    const peer = await agentConversation(run.home, chat, context);
    const before = await transcript(peer);
    expect(
        await request(run.connection, run.messages, {
            type: "rewind",
            id: randomUUID(),
            chat,
            item: open.items[0]?.id ?? "missing",
        }),
    ).toMatchObject({
        kind: "error",
        text: "Conversations between agents are read-only.",
    });
    expect(await transcript(peer)).toEqual(before);
    for (let n = 2; n <= 23; n++)
        await (
            await peer.submit(
                {
                    type: "input",
                    content: `Question ${n}`,
                    requestId: `peer-page:${n}`,
                    entry: {
                        kind: QuestionEntry.kind,
                        data: {
                            from: chat.from ?? scout,
                            task: `peer-page:${n}`,
                            at: run.home.now(),
                        },
                    },
                },
                context,
            )
        ).wait(context);
    await vi.waitFor(
        () =>
            expect(
                JSON.stringify(
                    run.messages.filter(
                        (m) => m.type === "turn" && m.chat.from !== undefined,
                    ),
                ),
            ).toContain("Answer 23"),
        { timeout: 10000 },
    );
    const latest = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    if (latest.type !== "transcript") throw new Error("No latest page");
    expect(latest.items).toHaveLength(40);
    const earlier = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
        before: latest.earlier ?? "missing",
    });
    expect(earlier).toMatchObject({ type: "transcript", earlier: null });
    if (earlier.type !== "transcript") throw new Error("No older page");
    expect(earlier.items).toHaveLength(6);
    expect(earlier.items[1]?.text).toContain("First answer");
    const count = () =>
        run.messages.filter(
            (m) => m.type === "turn" && m.chat.from !== undefined,
        ).length;
    run.connection.send({ type: "close", chat });
    await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: { with: { hamlet: "home", agent: "mayor" } },
    });
    const stopped = count();
    fauxAgents(faux, { "home:1": [fauxAssistantMessage("After close")] });
    await (
        await peer.submit({ type: "input", content: "After close" }, context)
    ).wait(context);
    await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: { with: { hamlet: "home", agent: "mayor" } },
    });
    expect(count()).toBe(stopped);
    const views = await conversationViews(run.home, scout, context);
    expect(views).toEqual([
        {
            with: chat.from,
            kind: "questions",
            count: 23,
            last: expect.any(Number),
        },
    ]);
}, 30000);

it("defaults a peer watch to letters and selects questions explicitly for the same pair", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await addAgent(run.home, "home", "scout");
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("tell_agent", {
                    name: "scout",
                    message: "A letter",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Sent"),
            fauxAssistantMessage("Reply received"),
        ],
        "home:1": [fauxAssistantMessage("Letter answer")],
    });
    await (await say(run.home, "Tell")).wait(context);
    await vi.waitFor(async () =>
        expect(
            (
                await conversationViews(
                    run.home,
                    { hamlet: "home", number: 1 },
                    context,
                )
            ).map((v) => v.kind),
        ).toEqual(["letters"]),
    );
    await vi.waitFor(
        async () =>
            expect(
                JSON.stringify(await transcript(await mayorChat(run.home))),
            ).toContain("Reply received"),
        { timeout: 10000 },
    );
    fauxAgents(faux, {
        "home:0": [
            fauxAssistantMessage(
                fauxToolCall("ask_agent", {
                    name: "scout",
                    message: "A question",
                }),
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done"),
        ],
        "home:1": [fauxAssistantMessage("Question answer")],
    });
    await (await say(run.home, "Ask")).wait(context);
    const chat: ChatRef = {
        with: { hamlet: "home", agent: "scout" },
        from: { hamlet: "home", number: 0 },
    };
    const letters = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat,
    });
    const questions = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: { ...chat, kind: "questions" },
    });
    if (letters.type !== "transcript" || questions.type !== "transcript")
        throw new Error("No peer transcripts");
    expect(letters.items.map((i) => i.text)).toEqual([
        "Letter from `mayor`:\nA letter",
        "Letter answer",
    ]);
    expect(letters.items[0]).toMatchObject({ author: "founder@home" });
    const chief = await request(run.connection, run.messages, {
        type: "open",
        id: randomUUID(),
        chat: { with: { hamlet: "home", agent: "mayor" } },
    });
    if (chief.type !== "transcript") throw new Error("No chief transcript");
    expect(
        chief.items.find((i) => i.text.startsWith("Reply from")),
    ).toMatchObject({ author: "scout@home" });
    expect(questions.items.map((i) => i.text)).toEqual([
        "Question from `mayor` (waiting for your answer):\nA question",
        "Question answer",
    ]);
});

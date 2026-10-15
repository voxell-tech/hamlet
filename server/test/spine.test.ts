import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import {
    defineExtension,
    GenerationTask,
    hook,
} from "@earendil-works/pi-durable";
import { expect, it, vi } from "vitest";
import {
    ErrandEntry,
    LetterEntry,
    QuestionEntry,
    ReplyEntry,
    startConversation,
} from "../src/chats.ts";
import { agentKey, checkName, HOME, tidyName } from "../src/hamlet.ts";
import { InstructionsDoc, owed } from "../src/instructions.ts";
import { booksOf, chargeRoad } from "../src/ledger.ts";
import { defaultLimits } from "../src/limits.ts";
import { level, RealmDoc, resolve, roadsOf } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { followHome, stopAgent } from "../src/talk.ts";
import { TaskDoc } from "../src/tasks.ts";
import { turnOf } from "../src/turn.ts";
import { raise } from "../src/waits.ts";
import { mayor, request, served } from "./connection-support.ts";
import {
    addAgent,
    addHamlet,
    context,
    fauxModels,
    MAYOR,
    openWaits,
    say,
    scratch,
    testHome,
} from "./support.ts";

it("founds and reopens the realm, builds every living house, and normalizes names", async () => {
    const dir = scratch();
    const models = fauxModels().models;
    const home = await testHome(dir, { models });
    expect(await home.harness.snapshot(RealmDoc, context)).toMatchObject({
        hamlets: [{ id: HOME, chiefs: [home.chief] }],
    });
    await addHamlet(home, "mill", "miller");
    await addAgent(home, "mill", "scout");
    await home.close(context);
    const again = await testHome(dir, { models });
    expect(existsSync(join(dir, "librarian/NOTES.md"))).toBe(true);
    expect(existsSync(join(dir, "houses/mill/1/NOTES.md"))).toBe(true);
    expect(
        await resolve(again.harness, HOME, "miller@mill", context),
    ).toMatchObject({
        ref: { hamlet: "mill", number: 0 },
        address: "mayor@mill",
    });
    expect(tidyName(" FelineBiologist__ ")).toBe("feline-biologist");
    for (const bad of [
        "",
        "Bad Name",
        "a".repeat(25),
        "mayor",
        "founder",
        "all",
    ])
        expect(() => checkName(bad)).toThrow();
    checkName("feline-biologist");
});

it("keeps instructions until settlement and for the protected interval", async () => {
    const { models, faux } = fauxModels();
    let release = () => {};
    let entered = false;
    faux.setResponses([
        async () => {
            entered = true;
            await new Promise<void>((r) => {
                release = r;
            });
            return fauxAssistantMessage("done");
        },
    ]);
    const home = await testHome(scratch(), { models, now: () => 100 });
    const submitted = await say(home, "inspect");
    const doc = await home.harness.snapshot(
        InstructionsDoc,
        agentKey(MAYOR),
        context,
    );
    expect(owed(doc, 100, 0)?.text).toBe("inspect");
    await vi.waitFor(() => expect(entered).toBe(true));
    release();
    await submitted.wait(context);
    await followHome(home, context);
    const after = await home.harness.snapshot(
        InstructionsDoc,
        agentKey(MAYOR),
        context,
    );
    expect(owed(after, 100, 50)?.text).toBe("inspect");
    expect(owed(after, 150, 50)).toBeUndefined();
});

it("charges a road to both its hamlet and its task", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await home.harness.commit(
        (tx) =>
            chargeRoad(tx, HOME, {
                task: "road-task",
                agent: 0,
                at: home.now(),
                limits: defaultLimits(),
            }),
        context,
    );
    expect(
        await booksOf(home, HOME, "day", "road-task", context),
    ).toMatchObject({ hamlet: { micros: 10000 }, task: { micros: 10000 } });
});

it("carries a letter's sender and task, and checks that task before calling the model", async () => {
    const { models, faux } = fauxModels();
    const seen: unknown[] = [];
    const home = await testHome(scratch(), {
        models,
        parts: [
            (home) => ({
                extension: defineExtension({
                    name: "test.turn",
                    hooks: [
                        hook(GenerationTask, {
                            beforeRequest: async (_r, api, c) => {
                                seen.push(await turnOf(home.harness, api, c));
                            },
                        }),
                    ],
                }),
            }),
        ],
    });
    const scout = await addAgent(home, HOME, "scout");
    const id = await home.harness.commit(async (tx) => {
        (await tx.doc(TaskDoc, "letter-task", "letter-task")).spent.micros =
            1000000;
        return startConversation(home, tx, { ...scout, chief: null, chat: 0 });
    }, context);
    const conversation = await home.harness.conversation(id, context);
    if (!conversation) throw new Error("no conversation");
    const sub = await conversation.submit(
        {
            type: "input",
            content: "look",
            requestId: randomUUID(),
            entry: {
                kind: LetterEntry.kind,
                data: {
                    from: MAYOR,
                    task: "letter-task",
                    at: home.now(),
                    replyTo: null,
                },
            },
        },
        context,
    );
    expect((await sub.wait(context)).status).not.toBe("done");
    expect(faux.state.callCount).toBe(0);
    expect(seen).toHaveLength(0);
    const record = await home.harness.snapshot(RecordDoc, HOME, context);
    if (!record) throw new Error("no record");
    expect(roadsOf(record)).toMatchObject([{ a: "office", b: 1 }]);
    expect(level({ messages: 3, lastUsed: 0 }, 10, 7)).toBe("paved");
});

it("dispatches road refusals and /to over a WebSocket to another agent and hamlet", async () => {
    const { models, faux } = fauxModels();
    const { home, connection, messages } = await served({ models });
    await addAgent(home, HOME, "scout");
    await addHamlet(home, "mill", "miller");
    const stub = await request(connection, messages, {
        type: "say",
        id: randomUUID(),
        to: mayor,
        text: "/road scout smith",
    });
    expect(stub).toMatchObject({
        type: "reply",
        kind: "error",
        text: "no agent named `smith` in this hamlet",
    });
    faux.setResponses([
        (prompt) => {
            expect(JSON.stringify(prompt)).toContain("explore");
            return fauxAssistantMessage("ready");
        },
    ]);
    expect(
        await request(connection, messages, {
            type: "say",
            id: randomUUID(),
            to: mayor,
            text: "/to scout explore",
        }),
    ).toMatchObject({ type: "reply" });
    faux.setResponses([fauxAssistantMessage("milling")]);
    expect(
        await request(connection, messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: "mill", agent: "mayor" },
            text: "grind",
        }),
    ).toMatchObject({ type: "reply" });
});

it("stops a running letter even after its agent leaves the living record, and lapses its waits", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models });
    const scout = await addAgent(home, HOME, "scout");
    const id = await home.harness.commit(
        (tx) => startConversation(home, tx, { ...scout, chief: null, chat: 0 }),
        context,
    );
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "true", network: true }),
            { stopReason: "toolUse" },
        ),
    ]);
    const conversation = await home.harness.conversation(id, context);
    if (!conversation) throw new Error("no conversation");
    const sub = await conversation.submit(
        {
            type: "input",
            content: "work",
            entry: {
                kind: LetterEntry.kind,
                data: {
                    from: MAYOR,
                    task: "stop-letter",
                    at: home.now(),
                    replyTo: null,
                },
            },
        },
        context,
    );
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, HOME, HOME)).agents = (
            await tx.doc(RecordDoc, HOME, HOME)
        ).agents.filter((a) => a.number !== scout.number);
    }, context);
    await stopAgent(home, scout, context);
    await vi.waitFor(async () =>
        expect((await sub.status(context)).status).not.toBe("placed"),
    );
    expect(await openWaits(home)).toHaveLength(0);
});

it("refuses a departed agent with its farewell and answers an agent's own question with a plain line", async () => {
    const run = await served({ models: fauxModels().models });
    const scout = await addAgent(run.home, HOME, "scout");
    await run.home.harness.commit(async (tx) => {
        await raise(
            tx,
            {
                kind: "question",
                danger: null,
                by: "scout",
                agent: scout,
                about: "Which map?",
                change: null,
                answers: ["reply"],
                chat: null,
                call: null,
            },
            run.home.now(),
        );
    }, context);
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: HOME, agent: "scout" },
            text: "The coast",
        }),
    ).toMatchObject({
        type: "reply",
        text: expect.stringContaining("Answered /reply"),
    });
    await run.home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, HOME, HOME);
        const agent = record.agents.find((a) => a.number === scout.number);
        if (!agent) throw new Error("no scout");
        record.archive.push({
            id: "scout-1",
            agent: { ...agent },
            how: "removed",
            into: null,
            at: 1,
            by: "mayor",
            reason: "Work ended",
            roads: [],
            merge: null,
            back: null,
        });
        record.agents = record.agents.filter((a) => a.number !== scout.number);
    }, context);
    expect(
        await request(run.connection, run.messages, {
            type: "say",
            id: randomUUID(),
            to: { hamlet: HOME, agent: "scout" },
            text: "Work",
        }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: expect.stringContaining(
            'was removed from the hamlet by the mayor ("Work ended")',
        ),
    });
});

it("migrates a step-2 record, task and ledger without losing numbers or spend", async () => {
    const dir = scratch();
    const models = fauxModels().models;
    const home = await testHome(dir, { models });
    await home.harness.commit(
        (tx) =>
            chargeRoad(tx, HOME, {
                task: "legacy",
                agent: 0,
                at: home.now(),
                limits: defaultLimits(),
            }),
        context,
    );
    await home.close(context);
    const db = new DatabaseSync(join(dir, "home.sqlite"));
    const storeOld = (kind: string, content: unknown) => {
        const row = db
            .prepare("SELECT id FROM documents WHERE kind = ?")
            .get(JSON.stringify(kind));
        if (!row) throw new Error(`no ${kind}`);
        const latest = db
            .prepare(
                "SELECT MAX(seq) AS seq FROM document_revisions WHERE document_id = ?",
            )
            .get(row.id as number);
        db.prepare("DELETE FROM document_revisions WHERE document_id = ?").run(
            row.id as number,
        );
        db.prepare(
            "INSERT INTO document_revisions (document_id, seq, kind, version, content) VALUES (?, ?, 'base', 1, ?)",
        ).run(row.id as number, latest?.seq as number, JSON.stringify(content));
    };
    storeOld("hamlet.record", {
        agents: [
            { number: 0, name: "founder", purpose: "", since: 0 },
            { number: 9, name: "scout", purpose: "Maps", since: 1 },
        ],
        mayor: 0,
        decisions: [
            { kind: "created", agent: 9, at: 1, by: "mayor", reason: "Maps" },
        ],
    });
    storeOld("hamlet.task", {
        spent: { tokens: 2, micros: 10000, ms: 0 },
        messages: 3,
    });
    storeOld("hamlet.ledger", {
        lines: [
            {
                what: "model",
                at: 1,
                agent: 0,
                task: "legacy",
                model: "faux::faux-1",
                host: "faux",
                tokens: 2,
                micros: 10000,
                ms: 0,
                input: 1,
                cached: 0,
                output: 1,
                reasoning: 0,
                estimated: false,
                stopReason: "stop",
                billed: null,
                firstTokenMs: null,
            },
        ],
    });
    db.close();
    const again = await testHome(dir, { models });
    expect(
        await again.harness.snapshot(RecordDoc, HOME, context),
    ).toMatchObject({
        next: 10,
        roads: [],
        archive: [],
        agents: [{ model: null }, { model: null }],
        decisions: [{ kind: "created", name: "scout", purpose: "Maps" }],
    });
    expect(
        await again.harness.snapshot(TaskDoc, "legacy", context),
    ).toMatchObject({ chief: null, messages: 3, spent: { micros: 10000 } });
});

it("maps each society input to its task and sender", async () => {
    const { models, faux } = fauxModels();
    const seen: unknown[] = [];
    const home = await testHome(scratch(), {
        models,
        parts: [
            (home) => ({
                extension: defineExtension({
                    name: "test.inputs",
                    hooks: [
                        hook(GenerationTask, {
                            beforeRequest: async (_r, api, c) => {
                                seen.push(await turnOf(home.harness, api, c));
                            },
                        }),
                    ],
                }),
            }),
        ],
    });
    for (const [token, data, from] of [
        [
            LetterEntry,
            { from: MAYOR, task: "input-task", at: 1, replyTo: null },
            MAYOR,
        ],
        [QuestionEntry, { from: MAYOR, task: "input-task", at: 1 }, MAYOR],
        [ReplyEntry, { from: MAYOR, task: "input-task", at: 1 }, MAYOR],
        [ErrandEntry, { task: "input-task", at: 1 }, null],
    ] as const) {
        const id = await home.harness.commit(
            (tx) =>
                startConversation(home, tx, { ...MAYOR, chief: null, chat: 0 }),
            context,
        );
        const conversation = await home.harness.conversation(id, context);
        if (!conversation) throw new Error("no conversation");
        faux.setResponses([fauxAssistantMessage("done")]);
        const sub = await conversation.submit(
            {
                type: "input",
                content: "work",
                entry: { kind: token.kind, data },
            },
            context,
        );
        expect((await sub.wait(context)).status).toBe("done");
        expect(seen.at(-1)).toEqual({
            input: sub.id,
            kind: token.kind,
            task: "input-task",
            chief: null,
            from,
        });
    }
});

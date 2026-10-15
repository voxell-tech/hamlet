import { randomUUID } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import { type Connection, connect } from "../src/connection.ts";
import { type Script, scriptedModels } from "../src/faux.ts";
import type {
    Item,
    Op,
    Say,
    ServerMessage,
    Transcript,
} from "../src/protocol.ts";
import { serveHome } from "../src/server.ts";
import { SettingsDoc } from "../src/settings.ts";
import { fold } from "./connection-support.ts";
import {
    CHIEF,
    closeAfter,
    context,
    mayorChat,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const mayor = { hamlet: "home", agent: "mayor" };
const chat = { with: mayor };

/** A home served on a free port, whose mayor answers with `script`. */
async function served(script: Script) {
    const home = await testHome(scratch(), {
        models: scriptedModels(script),
    });
    const server = await serveHome(home, 0, context);
    closeAfter(() => server.close());
    return { home, url: server.url, token: home.credentials.token };
}

/** A chief's client with the mayor's chat open: the items it holds, folded from the transcript and the turn ops. */
type Viewer = {
    readonly connection: Connection;
    /** The transcript the chat opened with. */
    readonly opened: Transcript;
    readonly items: Item[];
    readonly ops: Op[];
    readonly status: { working: boolean; pending: number };
    say(text: string, id?: string): void;
    /** Opens the chat again and gives the transcript's items. */
    reread(): Promise<Item[]>;
};

/** Connects to the served home, says hello and opens the mayor's chat. */
async function view(home: { url: string; token: string }): Promise<Viewer> {
    const connection = await connect(home.url);
    closeAfter(() => connection.close());
    connection.send({ type: "hello", v: 0, token: home.token });
    expect(await connection.next()).toMatchObject({ type: "state" });
    const transcripts: Transcript[] = [];
    const items: Item[] = [];
    const ops: Op[] = [];
    const status = { working: false, pending: 0 };
    void (async () => {
        for (;;) {
            const message = await connection.next().catch(() => undefined);
            if (message === undefined) return;
            if (message.type === "transcript") {
                transcripts.push(message);
                items.splice(
                    0,
                    items.length,
                    ...structuredClone(message.items),
                );
                Object.assign(status, {
                    working: message.working,
                    pending: message.pending,
                });
            }
            if (message.type === "turn") {
                for (const op of message.ops) {
                    ops.push(op);
                    fold(items, status, op);
                }
            }
        }
    })();
    const open = async () => {
        const before = transcripts.length;
        connection.send({ type: "open", id: randomUUID(), chat });
        await vi.waitFor(() => expect(transcripts.length).toBe(before + 1));
        return transcripts[before] as Transcript;
    };
    const opened = await open();
    return {
        connection,
        opened,
        items,
        ops,
        status,
        say: (text, id = randomUUID()) => {
            const message: Say = { type: "say", id, to: mayor, text };
            connection.send(message);
        },
        reread: async () => (await open()).items,
    };
}

/** Waits until `viewer` holds an answer and nothing runs or waits. */
async function settled(viewer: Viewer, answers = 1): Promise<void> {
    await vi.waitFor(
        () => {
            const answered = viewer.items.filter(
                (item) => item.kind === "assistant" && !isLive(item),
            );
            expect(answered.length).toBeGreaterThanOrEqual(answers);
            expect(viewer.status).toEqual({ working: false, pending: 0 });
        },
        { timeout: 10_000 },
    );
}

function isLive(item: Item): boolean {
    return item.id.startsWith("live:");
}

const listed: Script = {
    tokensPerSecond: 20,
    responses: [
        { calls: [{ name: "bash", arguments: { command: "echo hi" } }] },
        "The folder holds one file, and it says hello to whoever reads it.",
    ],
};

describe("a served home", () => {
    it("gives fresh state on reconnect even when seen matches the unchanged revision (S5)", async () => {
        const home = await served({ responses: [] });
        const first = await connect(home.url);
        closeAfter(() => first.close());
        first.send({ type: "hello", v: 0, token: home.token });
        const before = await first.next();
        if (before.type !== "state") throw new Error("no state");
        expect(before.settings.allowNet).toBe(false);
        await first.close();
        await home.home.harness.commit(async (tx) => {
            (await tx.doc(SettingsDoc)).allowNet = true;
        }, context);
        const next = await connect(home.url);
        closeAfter(() => next.close());
        next.send({
            type: "hello",
            v: 0,
            token: home.token,
            seen: { epoch: before.epoch, revision: before.revision },
        });
        let received: ServerMessage | undefined;
        void next
            .next()
            .then((message) => {
                received = message;
            })
            .catch(() => {});

        await vi.waitFor(() =>
            expect(received).toMatchObject({
                type: "state",
                settings: { allowNet: true },
            }),
        );
    });

    it("answers a line said to the mayor, and the turn ops it streams equal the transcript read again", async () => {
        const home = await served(listed);
        const viewer = await view(home);
        expect(viewer.opened).toMatchObject({ chat, items: [], earlier: null });

        viewer.say("What is in the folder?");
        await settled(viewer);

        expect(viewer.items).toMatchObject([
            { kind: "user", text: "What is in the folder?", chief: CHIEF },
            {
                kind: "tool",
                tool: {
                    name: "bash",
                    summary: '{"command":"echo hi"}',
                    exit: 0,
                },
            },
            {
                kind: "assistant",
                text: "The folder holds one file, and it says hello to whoever reads it.",
            },
        ]);
        expect(viewer.ops.map((op) => op.op)).toContain("append");
        expect(await viewer.reread()).toEqual(viewer.items);
    });

    it("keeps a completed write's change on its live tool item, exactly as read back (C30)", async () => {
        const home = await served({
            responses: [
                {
                    calls: [
                        {
                            name: "write",
                            arguments: {
                                path: "result.txt",
                                content: "approved\n",
                            },
                        },
                    ],
                },
                "Written.",
            ],
        });
        const viewer = await view(home);
        viewer.say("Write a file in your house.");
        await settled(viewer);
        const live = structuredClone(viewer.items);
        expect(live.find((item) => item.kind === "tool")).toMatchObject({
            tool: {
                name: "write",
                exit: 0,
                change: {
                    path: "result.txt",
                    created: true,
                    lines: [{ mark: "+", text: "approved" }],
                    added: 1,
                    removed: 0,
                },
            },
        });
        expect(await viewer.reread()).toEqual(live);
    });

    it("gives a client that opens the chat mid-turn the transcript as it stands, then the rest live (S5)", async () => {
        const text =
            "Here is a long answer, told slowly, so that someone can join while it is being written.";
        const home = await served({ tokensPerSecond: 20, responses: [text] });
        const first = await view(home);

        first.say("Tell me slowly.");
        await vi.waitFor(() =>
            expect(first.items.some((item) => isLive(item))).toBe(true),
        );
        const late = await view(home);

        const partial = late.opened.items.find((item) => isLive(item));
        expect(late.opened.items[0]).toMatchObject({ kind: "user" });
        expect(partial?.kind).toBe("assistant");
        expect(text.startsWith(partial?.text ?? "x")).toBe(true);
        expect(partial?.text.length).toBeLessThan(text.length);
        expect(late.opened.working).toBe(true);
        await settled(late);
        await settled(first);
        expect(late.items).toEqual(first.items);
        expect(await late.reread()).toEqual(late.items);
    });

    it("runs a turn to its end when every client has gone (S1)", async () => {
        const home = await served({
            responses: [
                {
                    calls: [
                        { name: "bash", arguments: { command: "sleep 1" } },
                    ],
                },
                "Slept.",
            ],
        });
        const viewer = await view(home);

        viewer.say("Sleep a while.");
        await vi.waitFor(() =>
            expect(viewer.items.map((item) => item.kind)).toContain("tool"),
        );
        await viewer.connection.close();

        const chat = await mayorChat(home.home);
        await vi.waitFor(
            async () => {
                const answers = (await transcript(chat)).filter(
                    (entry) => entry.kind === "pi.assistant",
                );
                expect(JSON.stringify(answers)).toContain("Slept.");
            },
            { timeout: 10_000 },
        );
    });

    it("answers lines said while the mayor works one at a time, in order, and a line sent again under its id once (C23)", async () => {
        const home = await served({
            tokensPerSecond: 50,
            responses: ["First.", "Second.", "Third.", "Once."],
        });
        const viewer = await view(home);

        for (const text of ["one", "two", "three"]) viewer.say(text);
        await settled(viewer, 3);

        expect(viewer.items.map((item) => [item.kind, item.text])).toEqual([
            ["user", "one"],
            ["assistant", "First."],
            ["user", "two"],
            ["assistant", "Second."],
            ["user", "three"],
            ["assistant", "Third."],
        ]);

        const id = randomUUID();
        viewer.say("Once.", id);
        await settled(viewer, 4);
        await viewer.connection.close();
        const again = await view(home);
        again.say("Once.", id);
        await again.reread();

        expect(
            again.items.filter((item) => item.text === "Once."),
        ).toHaveLength(2);
        expect(again.items.at(-1)).toMatchObject({
            kind: "assistant",
            text: "Once.",
        });
        expect(again.status).toEqual({ working: false, pending: 0 });
    });
});

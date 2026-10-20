import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it } from "vitest";
import {
    applyOp,
    type ClientMessage,
    type Item,
    parseClient,
    parseServer,
    type ServerMessage,
} from "../src/protocol.ts";

const scenarios = [
    "society",
    "leaving",
    "vote",
    "realm",
    "library",
    "hello",
    "turn",
    "tool",
    "approval",
    "fault",
    "stop",
    "rewind",
    "new-and-resume",
    "paging",
    "reads-and-edits",
    "login-key",
    "budget-stop",
    "give",
] as const;
type Scenario = (typeof scenarios)[number];
type Frame =
    | { from: "client"; message: ClientMessage }
    | { from: "server"; message: ServerMessage };

for (const name of scenarios)
    it(`keeps a real ${name} session in protocol v0`, () => {
        const file = process.env.HAMLET_FRAME_DIR
            ? resolve(process.env.HAMLET_FRAME_DIR, `${name}.jsonl`)
            : new URL(`../../frames/${name}.jsonl`, import.meta.url);
        const text = readFileSync(file, "utf8");
        expect(text.endsWith("\n")).toBe(true);
        const frames = text
            .trimEnd()
            .split("\n")
            .map((line): Frame => {
                const frame = JSON.parse(line);
                expect(["client", "server"]).toContain(frame.from);
                const parsed = (
                    frame.from === "client" ? parseClient : parseServer
                )(JSON.stringify(frame.message));
                expect(parsed).toHaveProperty("value", frame.message);
                return frame;
            });
        expect(frames[0]).toEqual({
            from: "client",
            message: { type: "hello", v: 0, token: "[REDACTED]" },
        });
        expect(frames[1]).toMatchObject({
            from: "server",
            message: { type: "state" },
        });
        if (name === "give") {
            const messages = frames.flatMap((f) =>
                f.from === "server" ? [f.message] : [],
            );
            expect(messages[0]).toMatchObject({
                starters: expect.arrayContaining([
                    expect.objectContaining({ name: "web-search" }),
                ]),
            });
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "reply",
                    kind: "notice",
                    text: "Handed over `inbox/folder`.",
                }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "reply",
                    kind: "notice",
                    text: "Gave `mayor` the skill `web-search`.",
                }),
            );
            expect(agents(messages).at(-1)?.skills).toEqual(["web-search"]);
        } else checkScenario(name, frames);
    });

function changes(messages: ServerMessage[]) {
    return messages.flatMap((m) => (m.type === "change" ? m.changes : []));
}

function agents(messages: ServerMessage[]) {
    return messages.flatMap((m) =>
        m.type === "state"
            ? m.agents
            : m.type === "change"
              ? m.changes.flatMap((c) => (c.type === "agent" ? [c.agent] : []))
              : [],
    );
}

function live(messages: ServerMessage[]): Item[] {
    const items: Item[] = [];
    const status = { working: false, pending: 0 };
    for (const message of messages) {
        if (message.type === "transcript")
            items.splice(0, items.length, ...structuredClone(message.items));
        if (message.type !== "turn") continue;
        for (const op of message.ops) applyOp(items, status, op);
    }
    return items;
}

function checkScenario(name: Scenario, frames: Frame[]) {
    const sent = frames.flatMap((frame) =>
        frame.from === "client" ? [frame.message] : [],
    );
    const messages = frames.flatMap((frame) =>
        frame.from === "server" ? [frame.message] : [],
    );
    const ops = messages.flatMap((message) =>
        message.type === "turn" ? message.ops : [],
    );
    switch (name) {
        case "society": {
            expect(agents(messages).map((a) => a.name)).toEqual(
                expect.arrayContaining(["scout", "smith", "poet"]),
            );
            expect(changes(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "hamlet",
                        hamlet: expect.objectContaining({
                            roads: expect.arrayContaining([
                                expect.objectContaining({ messages: 2 }),
                            ]),
                        }),
                    }),
                ]),
            );
            const items = messages.flatMap((m) =>
                m.type === "transcript"
                    ? m.items
                    : m.type === "turn"
                      ? m.ops.flatMap((o) => (o.op === "add" ? [o.item] : []))
                      : [],
            );
            expect(items).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        author: "smith@home",
                        text: expect.stringContaining("Hinge ready"),
                    }),
                ]),
            );
            expect(sent).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "open",
                        chat: expect.objectContaining({ kind: "questions" }),
                    }),
                ]),
            );
            break;
        }
        case "leaving": {
            expect(agents(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        name: "scout",
                        left: expect.objectContaining({ how: "removed" }),
                    }),
                    expect.objectContaining({
                        name: "scout",
                        left: expect.objectContaining({
                            how: "merged",
                            into: { hamlet: "home", number: 2 },
                        }),
                    }),
                ]),
            );
            expect(sent).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "say",
                        text: "/restore scout",
                    }),
                ]),
            );
            break;
        }
        case "vote":
            expect(changes(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "hamlet",
                        hamlet: expect.objectContaining({
                            votes: expect.arrayContaining([
                                expect.objectContaining({
                                    open: false,
                                    report: expect.stringContaining("Passed:"),
                                }),
                            ]),
                        }),
                    }),
                ]),
            );
            expect(agents(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({ name: "smith", mayor: true }),
                ]),
            );
            break;
        case "realm":
            expect(changes(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "hamlet",
                        hamlet: expect.objectContaining({
                            id: "mill",
                            at: { x: 60, z: 80 },
                        }),
                    }),
                ]),
            );
            expect(messages).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "state",
                        closed: expect.arrayContaining([
                            expect.objectContaining({ id: "mill" }),
                        ]),
                    }),
                ]),
            );
            expect(
                messages.filter((m) => m.type === "state").at(-1),
            ).toMatchObject({
                closed: [],
                hamlets: [expect.objectContaining({ id: "home" })],
            });
            break;
        case "library":
            expect(messages).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        type: "state",
                        library: expect.arrayContaining([
                            expect.objectContaining({ name: "map-iron" }),
                        ]),
                    }),
                ]),
            );
            expect(
                messages.filter((m) => m.type === "state").at(-1),
            ).toMatchObject({
                library: expect.not.arrayContaining([
                    expect.objectContaining({ name: "map-iron" }),
                ]),
            });
            expect(agents(messages)).toEqual(
                expect.arrayContaining([
                    expect.objectContaining({
                        name: "scout",
                        consulting: true,
                    }),
                ]),
            );
            break;
        case "hello":
            expect(frames).toHaveLength(2);
            break;
        case "turn": {
            expect(ops.some((op) => op.op === "append")).toBe(true);
            const last = frames.at(-1)?.message;
            expect(last?.type).toBe("transcript");
            if (last?.type !== "transcript")
                throw new Error("No final transcript");
            expect(last.items).toEqual(live(messages.slice(0, -1)));
            const activity = { what: "replied", at: last.items.at(-1)?.at };
            expect(last.last).toEqual(activity);
            expect(ops.filter((op) => op.op === "status").at(-1)).toMatchObject(
                { last: activity },
            );
            expect(last.items.map((item) => item.kind)).toEqual([
                "user",
                "reasoning",
                "tool",
                "assistant",
            ]);
            break;
        }
        case "tool":
            expect(ops.filter((op) => op.op === "close")).toMatchObject([
                { exit: 0 },
                { exit: 7, failure: "broken" },
            ]);
            break;
        case "approval": {
            const updates = changes(messages);
            const wait = updates.find((update) => update.type === "wait");
            expect(wait).toMatchObject({
                wait: {
                    answers: ["allow", "always", "deny"],
                    detail: { kind: "approval", danger: "command" },
                },
            });
            if (wait?.type !== "wait") throw new Error("No approval");
            expect(updates).toContainEqual({
                type: "wait-closed",
                id: wait.wait.id,
                how: "allow",
            });
            expect(ops).toContainEqual(
                expect.objectContaining({ op: "close", exit: 0 }),
            );
            const edit = updates.find(
                (update) =>
                    update.type === "wait" &&
                    update.wait.detail.kind === "approval" &&
                    update.wait.detail.danger === "edit",
            );
            expect(edit).toMatchObject({
                wait: {
                    detail: { kind: "approval", danger: "edit" },
                    change: {
                        path: "result.txt",
                        created: false,
                        lines: [
                            { mark: " ", text: "heading" },
                            { mark: "-", text: "approved" },
                            { mark: "+", text: "reviewed" },
                        ],
                        total: 3,
                        added: 1,
                        removed: 1,
                        count: "1 line added, 1 removed",
                    },
                },
            });
            if (edit?.type !== "wait") throw new Error("No held edit");
            expect(updates).toContainEqual({
                type: "wait-closed",
                id: edit.wait.id,
                how: "allow",
            });
            const tool = ops.find(
                (op) => op.op === "add" && op.item.tool?.name === "write",
            );
            if (tool?.op !== "add") throw new Error("No write item");
            expect(ops).toContainEqual(
                expect.objectContaining({
                    op: "close",
                    item: tool.item.id,
                    exit: 0,
                    change: edit.wait.change,
                }),
            );
            expect(
                live(messages).find((item) => item.id === tool.item.id),
            ).toMatchObject({ tool: { change: edit.wait.change } });
            expect(updates).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({ mode: "auto" }),
                }),
            );
            break;
        }
        case "fault": {
            expect(agents(messages)).toContainEqual(
                expect.objectContaining({ busy: false, faulted: true }),
            );
            const wait = changes(messages).find(
                (update) =>
                    update.type === "wait" &&
                    update.wait.detail.kind === "fault",
            );
            if (wait?.type !== "wait") throw new Error("No fault wait");
            expect(changes(messages)).toContainEqual({
                type: "wait-closed",
                id: wait.wait.id,
                how: "dismiss",
            });
            expect(ops).toContainEqual(
                expect.objectContaining({
                    op: "add",
                    item: expect.objectContaining({
                        kind: "error",
                        text: expect.stringContaining("Faux model unavailable"),
                    }),
                }),
            );
            break;
        }
        case "stop":
            expect(ops).toContainEqual(
                expect.objectContaining({
                    op: "add",
                    item: expect.objectContaining({
                        kind: "notice",
                        text: expect.stringContaining("Stopped:"),
                    }),
                }),
            );
            expect(
                live(messages).some(
                    (item) => item.text === "This answer must never arrive.",
                ),
            ).toBe(false);
            expect(agents(messages).at(-1)?.busy).toBe(false);
            break;
        case "rewind": {
            const reply = messages.findIndex(
                (message) =>
                    message.type === "reply" && message.kind === "rewound",
            );
            expect(reply).toBeGreaterThan(0);
            const reset = messages.findIndex(
                (message) =>
                    message.type === "turn" &&
                    message.ops.some((op) => op.op === "reset"),
            );
            expect(reply).toBeGreaterThanOrEqual(0);
            expect(reset).toBeGreaterThanOrEqual(0);
            expect(messages[reset + 1]).toMatchObject({
                type: "transcript",
                items: [
                    expect.objectContaining({ text: "Keep this line." }),
                    expect.objectContaining({ text: "First answer." }),
                ],
            });
            break;
        }
        case "new-and-resume":
            expect(sent).toContainEqual(
                expect.objectContaining({ type: "say", text: "/new" }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "read",
                    history: expect.arrayContaining([
                        expect.objectContaining({
                            number: 1,
                            current: false,
                            messages: 2,
                        }),
                    ]),
                }),
            );
            expect(sent).toContainEqual(
                expect.objectContaining({ type: "say", text: "/resume 1" }),
            );
            expect(live(messages).map((item) => item.text)).toEqual([
                "Remember our first conversation.",
                "The first conversation.",
                "Went back to an earlier session.",
            ]);
            break;
        case "paging": {
            const pages = messages.filter(
                (message) => message.type === "transcript",
            );
            expect(pages.map((page) => page.items.length)).toEqual([40, 6]);
            const [latest, older] = pages;
            expect(latest?.items[0]?.text).toBe("Line 4.");
            expect(latest?.earlier).toBe(latest?.items[0]?.id);
            expect(sent).toContainEqual(
                expect.objectContaining({
                    type: "open",
                    before: latest?.earlier,
                }),
            );
            expect(older?.earlier).toBeNull();
            const all = [...(older?.items ?? []), ...(latest?.items ?? [])];
            expect(new Set(all.map((item) => item.id)).size).toBe(46);
            expect(all.map((item) => item.text)).toEqual(
                Array.from({ length: 23 }, (_, i) => [
                    `Line ${i + 1}.`,
                    `Answer ${i + 1}.`,
                ]).flat(),
            );
            break;
        }
        case "reads-and-edits":
            for (const key of ["prompt", "papers", "charter", "models"])
                expect(
                    messages.some(
                        (message) => message.type === "read" && key in message,
                    ),
                ).toBe(true);
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({
                        limits: expect.objectContaining({
                            budget: expect.objectContaining({ cost: 2 }),
                        }),
                    }),
                }),
            );
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "hamlet",
                    hamlet: expect.objectContaining({
                        limits: expect.objectContaining({
                            mostAgents: 4,
                            budget: expect.objectContaining({ cost: 3 }),
                        }),
                    }),
                }),
            );
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "settings",
                    settings: expect.objectContaining({ allowNet: true }),
                }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "reply",
                    kind: "error",
                    text: "budget.cost: a cost is a number of zero or more",
                }),
            );
            expect(messages.at(-1)).toMatchObject({
                type: "read",
                charter: { own: true, text: "Serve the chief with care." },
            });
            break;
        case "login-key":
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "login",
                    step: {
                        // Historical and Rust recordings use different wording.
                        prompt: expect.stringMatching(
                            /^(Enter OpenAI API key|Paste your key\.)$/,
                        ),
                        secret: true,
                    },
                }),
            );
            expect(sent).toContainEqual(
                expect.objectContaining({
                    type: "login",
                    reply: expect.objectContaining({ value: "[REDACTED]" }),
                }),
            );
            expect(messages).toContainEqual(
                expect.objectContaining({
                    type: "login",
                    step: { done: "openai" },
                }),
            );
            expect(changes(messages)).toContainEqual(
                expect.objectContaining({
                    type: "providers",
                    providers: expect.arrayContaining([
                        expect.objectContaining({
                            id: "openai",
                            configured: true,
                        }),
                    ]),
                }),
            );
            expect(JSON.stringify(frames)).not.toContain(
                "sk-fake-frames-not-a-real-key",
            );
            break;
        case "budget-stop":
            expect(live(messages).at(-1)).toMatchObject({
                kind: "notice",
                text: expect.stringMatching(/budget.*\$0\.01.*chief/),
            });
            expect(agents(messages).at(-1)?.busy).toBe(false);
            expect(
                live(messages).some(
                    (item) => item.text === "This answer must never arrive.",
                ),
            ).toBe(false);
            break;
    }
}

import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    type AssistantMessage,
    type FauxResponseStep,
    fauxAssistantMessage,
    fauxToolCall,
    type ToolResultMessage,
} from "@earendil-works/pi-ai";
import type { Calls } from "../src/actor/actor/mod.ts";
import type {
    ChatEvent,
    ChatItem,
    ChatPage,
    ChatRef,
    Input,
} from "../src/agent/messages/agent.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    reflecting,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

const CHIEF: ChatRef = { with: 7 };

async function wake(
    t: TestContext,
    responses: FauxResponseStep[],
    calls: Calls = fakeCalls({}).calls,
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles({}),
        responses,
    });
    t.after(() => agent.stop());
    return agent;
}

/** pi-durable admits a request id once, over every wake. */
let inputs = 0;

async function send(agent: Woken, input: Input): Promise<void> {
    inputs += 1;
    await agent.actions.submit(agent.by(from.realm), {
        id: `input-${inputs}`,
        input,
    });
    await agent.idle();
}

function read(agent: Woken, chat: ChatRef = CHIEF): Promise<ChatPage> {
    return agent.actions.read_chat(agent.by(from.client), {
        id: "11111111-1111-4111-8111-111111111111",
        chat,
    });
}

function added(agent: Woken): ChatItem[] {
    return agent.events
        .filter(([name]) => name === "chat")
        .flatMap(([, event]) =>
            (event as ChatEvent).ops.flatMap((op) =>
                op.type === "add" ? [op.item] : [],
            ),
        );
}

const IMAGE = "ab".repeat(32);
const FILE = "12".repeat(32);

test("a line's images and files show on its item, in its events and in its rewind", async (t) => {
    const agent = await wake(t, [fauxAssistantMessage("Seen.")]);
    await send(agent, {
        type: "line",
        text: "Look at these.",
        images: [{ id: IMAGE, at: 4 }],
        files: [{ id: FILE, name: "notes/plan.md", size: 2048 }],
    });
    const images = [{ id: IMAGE, at: 4 }];
    const files = [{ id: FILE, name: "notes/plan.md", size: 2048 }];

    const line = (await read(agent)).items.find((item) => item.kind === "user");
    assert.deepEqual(
        { images: line?.images, files: line?.files },
        { images, files },
    );
    const event = added(agent).find((item) => item.kind === "user");
    assert.deepEqual(
        { images: event?.images, files: event?.files },
        { images, files },
    );
    assert.deepEqual(
        await agent.actions.rewind(agent.by(from.client), {
            id: "rewind-1",
            chat: CHIEF,
            item: String(line?.id),
        }),
        { type: "rewound", text: "Look at these.", images, files },
    );
});

test("a sender's item shows its name as the realm gives it", async (t) => {
    const agent = await wake(
        t,
        ["One.", "Dear Ada.", "Three.", "Thanks.", "Noted.", "Yes."].map(
            (text) => fauxAssistantMessage(text),
        ),
        fakeCalls({ realm: { reply: () => ({ type: "done" }) } }).calls,
    );
    await send(agent, { type: "line", text: "a" });
    await send(agent, {
        type: "letter",
        from: { agent: 3, name: "hire#3 (Ada)" },
        text: "Hello.",
    });
    await send(agent, {
        type: "question",
        letter: 5,
        from: { agent: 4 },
        text: "How many?",
    });
    const chief = String(
        await conversationOfChat(agent.by(from.client), CHIEF),
    );
    await send(agent, {
        type: "answer",
        question: 6,
        from: { agent: 3, name: "hire#3 (Ada)" },
        text: "Three.",
        conversation: chief,
    });
    await send(agent, {
        type: "worker_answer",
        from: { agent: 9, name: "worker#9 (Scout)" },
        text: "Found it.",
    });
    await send(agent, {
        type: "letter",
        from: { agent: 1, name: "hand" },
        text: "Carry on.",
    });

    const authors = (items: readonly ChatItem[]) =>
        items.filter((item) => item.kind === "user").map((item) => item.author);
    assert.deepEqual(
        authors(
            (await read(agent, { with: 7, from: 3, kind: "letters" })).items,
        ),
        ["hire#3 (Ada)"],
    );
    assert.deepEqual(
        authors(
            (await read(agent, { with: 7, from: 4, kind: "questions" })).items,
        ),
        ["hire#4"],
    );
    assert.deepEqual(
        authors(
            (await read(agent, { with: 7, from: 1, kind: "letters" })).items,
        ),
        ["hand"],
    );
    assert.deepEqual(authors((await read(agent)).items), [
        undefined,
        "hire#3 (Ada)",
        "worker#9 (Scout)",
    ]);
    assert.deepEqual(authors(added(agent)), [
        undefined,
        "hire#3 (Ada)",
        "hire#4",
        "hire#3 (Ada)",
        "worker#9 (Scout)",
        "hand",
    ]);
});

test("a write that makes a file shows its change, as today", async (t) => {
    const agent = await wake(
        t,
        [
            toolCall("write", {
                path: "/work/project/notes.md",
                content: "one\ntwo\n",
            }),
            fauxAssistantMessage("Written."),
        ],
        fakeCalls({
            workspace: {
                write_file: () => ({
                    type: "ended",
                    end: {
                        type: "changed",
                        path: "/work/project/notes.md",
                        created: true,
                        hash: "1f",
                    },
                }),
            },
        }).calls,
    );
    await send(agent, { type: "line", text: "Write it." });

    const tool = (await read(agent)).items.find(
        (item) => item.tool !== undefined,
    )?.tool;
    // As `crates/hamlet_server/src/work/cut.rs`'s `change_between` shows a new file.
    assert.deepEqual(tool?.change, {
        path: "/work/project/notes.md",
        created: true,
        added: 2,
        removed: 0,
        total: 2,
        count: "New file, 2 lines",
        lines: [
            { mark: "added", text: "one" },
            { mark: "added", text: "two" },
        ],
    });
});

/** Every answer's time, from the chief's session. */
async function answerTimes(agent: Woken): Promise<number[]> {
    const harness = await agent.harness();
    const conversation = await conversationOfChat(agent.by(from.client), CHIEF);
    assert.ok(conversation !== undefined);
    const kept = await harness.conversation(conversation, BACKGROUND_CONTEXT);
    const page = await kept?.entries(
        { order: "ascending" },
        1000,
        undefined,
        BACKGROUND_CONTEXT,
    );
    return (page?.items ?? []).flatMap((entry) =>
        entry.kind === "pi.assistant"
            ? [
                  (entry.model?.[0] as AssistantMessage | undefined)
                      ?.durationMs ?? 0,
              ]
            : [],
    );
}

test("a chat's usage counts its calls and their time, also from before a wake's window", async (t) => {
    const turns = 30;
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        responses: Array.from({ length: turns }, (_, n) =>
            fauxAssistantMessage(`Answer ${n}.`),
        ),
    });
    for (let n = 0; n < turns; n += 1) {
        await send(agent, { type: "line", text: `Line ${n}.` });
    }
    const times = await answerTimes(agent);
    assert.ok(times.length >= turns);
    const calls = `${times.length}`;
    const ms = `${times.reduce((sum, time) => sum + time, 0)}`;
    const usage = (await read(agent)).usage;
    assert.deepEqual({ calls: usage.calls, ms: usage.ms }, { calls, ms });
    await agent.stop();

    // The wake's watch sees the newest 50 entries of the 60.
    const woken = await agent.again();
    t.after(() => woken.stop());
    const again = (await read(woken)).usage;
    assert.deepEqual({ calls: again.calls, ms: again.ms }, { calls, ms });
});

test("a session's history is modified when its last entry was written", async (t) => {
    const agent = await wake(t, reflecting(["One.", "Two."]));
    const client = agent.by(from.client);
    await send(agent, { type: "line", text: "a" });
    await agent.actions.new_session(client, { id: "new-1", to: 7 });
    await send(agent, { type: "line", text: "b" });
    const later = Date.now() + 3_600_000;
    t.mock.timers.enable({ apis: ["Date"], now: later });
    await agent.actions.resume(client, { id: "resume-1", number: 1, to: 7 });
    await agent.idle();
    t.mock.timers.reset();

    const sessions = (await agent.actions.read_history(client)).sessions;
    assert.equal(
        sessions.find((session) => session.number === 1)?.modified,
        Math.floor(later / 1000),
    );
});

/** An `ask_chief` call and its result, written into the chief's session. */
async function asked(agent: Woken, result: string): Promise<void> {
    const harness = await agent.harness();
    const conversation = await conversationOfChat(agent.by(from.client), CHIEF);
    assert.ok(conversation !== undefined);
    const answer: ToolResultMessage = {
        role: "toolResult",
        toolCallId: "ask-1",
        toolName: "ask_chief",
        content: [{ type: "text", text: result }],
        isError: false,
        timestamp: 0,
    };
    await harness.commit(async (tx) => {
        await tx.appendEntry(conversation, {
            kind: "pi.assistant",
            model: [
                fauxAssistantMessage(
                    [
                        fauxToolCall(
                            "ask_chief",
                            { question: "Which colour?" },
                            { id: "ask-1" },
                        ),
                    ],
                    { stopReason: "toolUse" },
                ),
            ],
        });
        await tx.appendEntry(conversation, {
            kind: "pi.tool-result",
            model: [answer],
        });
    }, BACKGROUND_CONTEXT);
}

const answered = (items: readonly ChatItem[]) =>
    items.filter((item) => item.kind === "answered").map((item) => item.text);

test("the chief's answer to an ask_chief shows as its own item, as today", async (t) => {
    const agent = await wake(t, [fauxAssistantMessage("Seen.")]);
    await send(agent, { type: "line", text: "a" });
    await asked(agent, "The chief answers:  Blue. ");

    assert.deepEqual(answered((await read(agent)).items), ["Blue."]);
    await until(() => answered(added(agent)).length > 0);
    assert.deepEqual(answered(added(agent)), ["Blue."]);
});

test("an ask_chief result that is not the chief's answer shows no answered item, as today", async (t) => {
    const agent = await wake(t, [fauxAssistantMessage("Seen.")]);
    await send(agent, { type: "line", text: "a" });
    await asked(agent, "The chief answers:   ");

    const items = (await read(agent)).items;
    assert.ok(items.some((item) => item.tool?.name === "ask_chief"));
    assert.deepEqual(answered(items), []);
});

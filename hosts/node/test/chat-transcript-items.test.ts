import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    type AssistantMessage,
    type FauxResponseStep,
    fauxAssistantMessage,
    type ToolResultMessage,
} from "@earendil-works/pi-ai";
import type { EntryRecord } from "@earendil-works/pi-durable";
import type { Calls } from "../src/actor/actor/mod.ts";
import { timeOf } from "../src/agent/chat-items.ts";
import type {
    ChatEvent,
    ChatItem,
    ChatPage,
    ChatRef,
    Input,
    Outcome,
} from "../src/agent/messages/agent.ts";
import type { CallId } from "../src/agent/messages/shared.ts";
import type {
    Exit,
    NotRunWhy,
    Run,
    RunAnswer,
} from "../src/agent/messages/workspace.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    named,
    reflecting,
    type Sent,
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
        images: [
            {
                image: { id: IMAGE, at: 4 },
                file: {
                    parcel: 1,
                    file: 0,
                    path: `images/${IMAGE}.png`,
                    size: 64,
                },
            },
        ],
        files: [
            {
                attached: { id: FILE, name: "notes/plan.md", size: 2048 },
                file: { parcel: 1, file: 1, path: "notes/plan.md", size: 2048 },
            },
        ],
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

test("a chief's line shows the chief who said it, in the page and its events, as today", async (t) => {
    const agent = await wake(t, [fauxAssistantMessage("Seen.")]);
    await send(agent, { type: "line", text: "Go on.", chief: "ada" });

    const chiefOf = (items: readonly ChatItem[]) =>
        items.filter((item) => item.kind === "user").map((item) => item.chief);
    assert.deepEqual(chiefOf((await read(agent)).items), ["ada"]);
    assert.deepEqual(chiefOf(added(agent)), ["ada"]);
});

test("a chat item's time is in milliseconds, as today", async (t) => {
    const before = Date.now();
    const agent = await wake(t, [fauxAssistantMessage("Seen.")]);
    await send(agent, { type: "line", text: "Go on." });
    await agent.actions.new_session(agent.by(from.client), {
        id: "new-ms",
        to: 7,
    });
    const after = Date.now();

    const times = (items: readonly ChatItem[]) =>
        items.map((item) => [item.kind, Number(item.at)] as const);
    const within = ([, at]: readonly [string, number]) =>
        at >= before && at <= after;
    const past = times((await read(agent, { with: 7, past: 1 })).items);
    assert.deepEqual(
        past.map(([kind]) => kind),
        ["user", "assistant"],
    );
    assert.ok(past.every(within), JSON.stringify(past));
    const current = times((await read(agent)).items);
    assert.deepEqual(
        current.map(([kind]) => kind),
        ["notice"],
    );
    assert.ok(current.every(within), JSON.stringify(current));
    const events = times(added(agent));
    assert.ok(
        events.length > 0 && events.every(within),
        JSON.stringify(events),
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

/** A turn that asks the chief, and the realm's notice that closes the question. */
async function askedChief(t: TestContext, notice: string): Promise<Woken> {
    const { calls, sent } = fakeCalls({
        realm: { ask_chief: () => ({ type: "done" }) },
    });
    const agent = await wake(
        t,
        [
            toolCall("ask_chief", {
                questions: [{ question: "Which colour?" }],
            }),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("On it."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Paint it." });
    await send(agent, {
        type: "notice",
        text: notice,
        held: { call: callOf(sent, "ask_chief"), end: "ran" },
    });
    return agent;
}

function callOf(sent: readonly Sent[], act: string): CallId {
    const call = sent.find((one) => one.name === act)?.message as
        | { readonly call?: CallId }
        | undefined;
    assert.ok(call?.call !== undefined, `the agent called ${act}`);
    return call.call;
}

const tools = (items: readonly ChatItem[]) =>
    items.flatMap((item) =>
        item.tool === undefined
            ? []
            : [[item.tool.name, item.tool.exit, item.tool.failure ?? ""]],
    );

const answered = (items: readonly ChatItem[]) =>
    items.filter((item) => item.kind === "answered").map((item) => item.text);

test("the chief's answer to an ask_chief shows as its own item, as today", async (t) => {
    const agent = await askedChief(t, "The chief answers: Blue.");

    const items = (await read(agent)).items;
    assert.deepEqual(answered(items), ["Blue."]);
    assert.ok(!items.some((item) => item.text === "The chief answers: Blue."));
    await until(() => answered(added(agent)).length > 0);
    assert.deepEqual(answered(added(agent)), ["Blue."]);
});

test("a picked answer shows the chief's words, and a dismissed question shows no answered item, as today", async (t) => {
    const picked = await askedChief(
        t,
        "The chief answers:\nWhich colour? Blue",
    );
    assert.deepEqual(answered((await read(picked)).items), [
        "Which colour? Blue",
    ]);
    await picked.stop();

    const dismissed = await askedChief(
        t,
        "The chief did not answer; go on with what you know.",
    );
    const items = (await read(dismissed)).items;
    assert.deepEqual(answered(items), []);
    assert.deepEqual(
        items.filter((item) => item.kind === "notice"),
        [],
    );
    assert.deepEqual(tools(items), [["ask_chief", 0, ""]]);
});

test("a call that waits for a chief stays open until its ruling closes the same item, also after a restart", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            ask_chief: () => ({ type: "done" }),
            hire: () => ({
                type: "refused",
                why: { type: "reserved", wait: 1 },
                words: "This call waits for a chief's answer.",
            }),
            end: () => ({
                type: "refused",
                why: { type: "reserved", wait: 2 },
                words: "This call waits for a chief's answer.",
            }),
        },
    });
    const agent = await wake(
        t,
        [
            toolCall(
                "ask_chief",
                { questions: [{ question: "Which colour?" }] },
                "c1",
            ),
            toolCall("hire", { purpose: "Check the links" }, "c2"),
            toolCall("end_hire", { hire: "hire#3" }, "c3"),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("Blue it is."),
            fauxAssistantMessage("Hired."),
            fauxAssistantMessage("Kept."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Paint it, hire and end." });
    const open = [
        ["ask_chief", undefined, ""],
        ["hire", undefined, ""],
        ["end_hire", undefined, ""],
    ];
    assert.deepEqual(tools((await read(agent)).items), open);
    await agent.stop();

    const woken = await agent.again();
    t.after(() => woken.stop());
    assert.deepEqual(tools((await read(woken)).items), open);
    const rulings = [
        ["The chief answers: Blue.", "ask_chief", false],
        ["Hired hire#4 (Hire 1).", "hire", false],
        ["the chief refused it: keep it", "end", true],
    ] as const;
    for (const [text, act, refused] of rulings) {
        await send(woken, {
            type: "notice",
            text,
            held: { call: callOf(sent, act), end: refused ? "blocked" : "ran" },
        });
    }

    const closed = [
        ["ask_chief", 0, ""],
        ["hire", 0, ""],
        ["end_hire", 1, "Tool call blocked: the chief refused it: keep it"],
    ];
    const items = (await read(woken)).items;
    assert.deepEqual(tools(items), closed);
    assert.deepEqual(answered(items), ["Blue."]);
    assert.deepEqual(
        items.filter((item) => item.kind === "notice"),
        [],
    );
    const ended = added(woken).filter((item) => item.tool?.exit !== undefined);
    assert.deepEqual(tools(ended), closed);
    const shown = items.filter((item) => item.tool !== undefined);
    assert.deepEqual(
        ended.map((item) => item.id),
        shown.map((item) => item.id),
        "each ruling closes the call's own item",
    );
    const closing = woken.events
        .filter(([name]) => name === "chat")
        .flatMap(([, event]) => (event as ChatEvent).ops)
        .flatMap((op) =>
            op.type === "add" && op.item.tool?.exit !== undefined ? [op] : [],
        );
    assert.deepEqual(
        closing.map((op) => op.replaces),
        closing.map((op) => op.item.id),
        "a client replaces the open item with the closed one",
    );
});

test("a stop ends each call held for a chief as today's stop aborts it, and starts no run", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            ask_chief: () => ({ type: "done" }),
            hire: () => ({
                type: "refused",
                why: { type: "reserved", wait: 1 },
                words: "This call waits for a chief's answer.",
            }),
        },
    });
    const agent = await wake(
        t,
        [
            toolCall(
                "ask_chief",
                { questions: [{ question: "Which colour?" }] },
                "c1",
            ),
            toolCall("hire", { purpose: "Check the links" }, "c2"),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("Ran after the stop."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Ask and hire." });
    const stopped = "the call's wait for a chief lapsed: the turn was stopped";
    for (const act of ["ask_chief", "hire"]) {
        await send(agent, {
            type: "notice",
            text: stopped,
            held: { call: callOf(sent, act), end: "aborted" },
        });
    }

    const aborted = [
        ["ask_chief", 1, undefined, "Tool ask_chief was aborted"],
        ["hire", 1, undefined, "Tool hire was aborted"],
    ];
    const closed = () =>
        ends(added(agent).filter((item) => item.tool?.exit !== undefined));
    await until(() => closed().length === aborted.length);
    assert.deepEqual(closed(), aborted);
    await agent.idle();
    const items = (await read(agent)).items;
    assert.deepEqual(ends(items), aborted);
    assert.deepEqual(answered(items), []);
    assert.deepEqual(
        items.filter((item) => item.kind !== "tool").map((item) => item.text),
        ["Ask and hire.", "Waiting on the chief."],
    );
    await agent.stop();
    const woken = await agent.again();
    t.after(() => woken.stop());
    assert.deepEqual(ends((await read(woken)).items), aborted);
});

async function chiefEntries(agent: Woken): Promise<readonly EntryRecord[]> {
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
    return page?.items ?? [];
}

async function callTimes(agent: Woken): Promise<Map<string, number>> {
    return new Map(
        (await chiefEntries(agent)).flatMap((entry) => {
            const result = entry.model?.[0] as ToolResultMessage | undefined;
            return entry.kind === "pi.tool-result" &&
                result?.durationMs !== undefined
                ? [[result.toolName, result.durationMs] as const]
                : [];
        }),
    );
}

const ends = (items: readonly ChatItem[]) =>
    items.flatMap(({ tool }) =>
        tool === undefined
            ? []
            : [[tool.name, tool.exit, tool.ms, tool.failure]],
    );

test("a ruling closes its call as today does: a refusal is blocked with no time, an allowance takes the call's own time", async (t) => {
    const reserved = (wait: number) => () => ({
        type: "refused" as const,
        why: { type: "reserved" as const, wait },
        words: "This call waits for a chief's answer.",
    });
    const { calls, sent } = fakeCalls({
        realm: { hire: reserved(1), end: reserved(2) },
    });
    const agent = await wake(
        t,
        [
            toolCall("hire", { purpose: "Check the links" }, "c1"),
            toolCall("end_hire", { hire: "hire#3" }, "c2"),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("Hired."),
            fauxAssistantMessage("Kept."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Hire and end." });
    await send(agent, {
        type: "notice",
        text: "Hired hire#4 (Hire 1).",
        held: { call: callOf(sent, "hire"), end: "ran" },
    });
    await send(agent, {
        type: "notice",
        text: "the chief refused it: keep it",
        held: { call: callOf(sent, "end"), end: "blocked" },
    });

    const own = await callTimes(agent);
    assert.ok(own.get("hire") !== undefined);
    assert.deepEqual(ends((await read(agent)).items), [
        ["hire", 0, own.get("hire"), undefined],
        [
            "end_hire",
            1,
            undefined,
            "Tool call blocked: the chief refused it: keep it",
        ],
    ]);
});

test("remove_hamlet stays open until a chief rules, and the ruling closes it with words that say how it ended", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            remove_hamlet: () => ({
                type: "refused",
                why: { type: "reserved", wait: 1 },
                words: "This call waits for a chief's answer.",
            }),
        },
    });
    const agent = await wake(
        t,
        [
            toolCall("remove_hamlet", { hamlet: "h1" }, "c1"),
            toolCall("remove_hamlet", { hamlet: "h2" }, "c2"),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("Removed."),
            fauxAssistantMessage("Kept."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Remove both." });
    const open = [
        ["remove_hamlet", undefined, ""],
        ["remove_hamlet", undefined, ""],
    ];
    assert.deepEqual(tools((await read(agent)).items), open);

    const [first, second] = named(sent, "remove_hamlet").map(
        (one) => (one.message as { call: CallId }).call,
    );
    assert.ok(first !== undefined && second !== undefined);
    await send(agent, {
        type: "notice",
        text: "Removed h1.",
        held: { call: first, end: "ran" },
    });
    assert.deepEqual(tools((await read(agent)).items), [
        ["remove_hamlet", 0, ""],
        ["remove_hamlet", undefined, ""],
    ]);
    await send(agent, {
        type: "notice",
        text: "the chief refused it: keep it",
        held: { call: second, end: "blocked" },
    });
    assert.deepEqual(tools((await read(agent)).items), [
        ["remove_hamlet", 0, ""],
        [
            "remove_hamlet",
            1,
            "Tool call blocked: the chief refused it: keep it",
        ],
    ]);
});

test("an ask_chief's time runs from its call to the chief's answer, as today's call holds until then", async (t) => {
    const agent = await askedChief(t, "The chief answers: Blue.");

    const entries = await chiefEntries(agent);
    const result = entries.find((entry) => entry.kind === "pi.tool-result")
        ?.model?.[0] as ToolResultMessage | undefined;
    const answer = entries.find((entry) => entry.kind === "hamlet.notice");
    assert.ok(result?.durationMs !== undefined && answer !== undefined);
    const began = result.timestamp - result.durationMs;
    assert.deepEqual(ends((await read(agent)).items), [
        ["ask_chief", 0, timeOf(answer) - began, undefined],
    ]);
});

test("an objection stays open until the chief rules: go closes it at the ruling's time, withdraw aborts it, as today's", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { object: () => ({ type: "done" }) },
    });
    const agent = await wake(
        t,
        [
            toolCall("object", { reason: "no" }, "c1"),
            toolCall("object", { reason: "not this either" }, "c2"),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("Going on."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Do both." });
    assert.deepEqual(ends((await read(agent)).items), [
        ["object", undefined, undefined, undefined],
        ["object", undefined, undefined, undefined],
    ]);
    const [go, withdrawn] = sent
        .filter((one) => one.name === "object")
        .map((one) => (one.message as { readonly call: CallId }).call);
    assert.ok(go !== undefined && withdrawn !== undefined);
    await send(agent, {
        type: "notice",
        text: "The chief rules: go on with the instruction.",
        held: { call: go, end: "ran" },
    });
    await send(agent, {
        type: "notice",
        text: "The chief withdrew the instruction. Stop working on it.",
        held: { call: withdrawn, end: "aborted" },
    });

    const entries = await chiefEntries(agent);
    const result = entries.find((entry) => entry.kind === "pi.tool-result")
        ?.model?.[0] as ToolResultMessage | undefined;
    const ruling = entries.find((entry) => entry.kind === "hamlet.notice");
    assert.ok(result?.durationMs !== undefined && ruling !== undefined);
    const began = result.timestamp - result.durationMs;
    assert.deepEqual(ends((await read(agent)).items), [
        ["object", 0, timeOf(ruling) - began, undefined],
        ["object", 1, undefined, "Tool object was aborted"],
    ]);
});

test("a call that waits for a chief to allow it stays open, and its end closes the same item as today's", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            run: () => ({ type: "waits_for_chief", needs: ["command"] }),
            write_file: () => ({ type: "waits_for_chief", needs: ["edit"] }),
        },
    });
    const commands = [
        "echo ran",
        "echo oops; exit 3",
        "sleep 7",
        "echo keep",
        "echo bare",
        "echo stop",
    ];
    const agent = await wake(
        t,
        [
            ...commands.map((command, n) =>
                toolCall("bash", { command }, `c${n}`),
            ),
            toolCall("write", { path: "/work/a.md", content: "a\n" }, "cw"),
            fauxAssistantMessage("Waiting on the chief."),
            ...commands.map((_, n) => fauxAssistantMessage(`Heard ${n}.`)),
            fauxAssistantMessage("Written."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Run them and write." });
    assert.deepEqual(
        ends((await read(agent)).items),
        [...commands.map(() => "bash"), "write"].map((name) => [
            name,
            undefined,
            undefined,
            undefined,
        ]),
    );

    const ran = (code: number, output: string, took: number): Outcome => ({
        type: "run",
        end: {
            type: "ran",
            command: 1,
            exit: { type: "code", code },
            output,
            policy: "sandbox",
            took_ms: took,
        },
    });
    const notRun = (why: NotRunWhy, words: string): Outcome => ({
        type: "run",
        end: { type: "not_run", why, words },
    });
    const outcomes = [
        ran(0, "ran\n", 12),
        ran(3, "oops\n", 15),
        ran(0, "", 7019),
        notRun(
            { type: "chief_refused", reason: "keep it" },
            "a chief refused the call: keep it",
        ),
        notRun({ type: "chief_refused" }, "a chief refused the call"),
        notRun(
            { type: "lapsed", lapse: "stopped" },
            "the call's wait for a chief lapsed: the turn was stopped",
        ),
    ];
    const runs = sent
        .filter((one) => one.name === "run")
        .map((one) => (one.message as { call: CallId }).call);
    for (const [n, outcome] of outcomes.entries()) {
        const call = runs[n];
        assert.ok(call !== undefined);
        await agent.actions.finished(agent.by(from.workspace), {
            call,
            outcome,
        });
        await agent.idle();
    }
    await agent.actions.finished(agent.by(from.workspace), {
        call: callOf(sent, "write_file"),
        outcome: {
            type: "change",
            end: {
                type: "changed",
                path: "/work/a.md",
                created: true,
                hash: "1f",
            },
        },
    });
    await agent.idle();

    const own = await callTimes(agent);
    assert.ok(own.get("write") !== undefined);
    const items = (await read(agent)).items;
    assert.deepEqual(ends(items), [
        ["bash", 0, 12, undefined],
        ["bash", 3, 15, "oops"],
        ["bash", 0, 7019, undefined],
        [
            "bash",
            1,
            undefined,
            "Tool call blocked: the chief refused it: keep it",
        ],
        [
            "bash",
            1,
            undefined,
            "Tool call blocked: the chief refused it: no reason given",
        ],
        ["bash", 1, undefined, "Tool bash was aborted"],
        ["write", 0, own.get("write"), undefined],
    ]);
    assert.equal(
        items.find((item) => item.tool?.name === "write")?.tool?.change
            ?.created,
        true,
    );
    assert.deepEqual(
        items.filter((item) => item.kind === "user").map((item) => item.text),
        ["Run them and write."],
    );
    const closing = agent.events
        .filter(([name]) => name === "chat")
        .flatMap(([, event]) => (event as ChatEvent).ops)
        .flatMap((op) =>
            op.type === "add" && op.item.tool?.exit !== undefined ? [op] : [],
        );
    assert.deepEqual(
        closing.map((op) => op.replaces),
        items.flatMap((item) => (item.tool === undefined ? [] : [item.id])),
        "each end closes its call's own item, once",
    );
});

test("a command no chief holds closes with its own exit code, and one that runs on closes its own item when it ends, as today's", async (t) => {
    const ended = (exit: Exit, output: string): RunAnswer => ({
        type: "ended",
        end: {
            type: "ran",
            command: 1,
            exit,
            output,
            policy: "sandbox",
            took_ms: 15,
        },
    });
    const answers: Record<string, RunAnswer> = {
        "exit 3": ended({ type: "code", code: 3 }, "oops\n"),
        "sleep 200": ended({ type: "timed_out" }, ""),
        "sleep 7": { type: "running", command: 2 },
        "sleep 9": { type: "running", command: 3 },
    };
    const commands = Object.keys(answers);
    const { calls, sent } = fakeCalls({
        workspace: {
            run: (message: Run) =>
                answers[(message.program as { script: string }).script] ??
                assert.fail("an unknown command"),
        },
    });
    const agent = await wake(
        t,
        [
            ...commands.map((command, n) =>
                toolCall("bash", { command }, `c${n}`),
            ),
            fauxAssistantMessage("Running."),
            fauxAssistantMessage("Heard one."),
            fauxAssistantMessage("Heard two."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Run them." });

    const runs = sent
        .filter((one) => one.name === "run")
        .map((one) => (one.message as { call: CallId }).call);
    const later: Outcome[] = [
        {
            type: "run",
            end: {
                type: "ran",
                command: 2,
                exit: { type: "code", code: 0 },
                output: "slept\n",
                policy: "sandbox",
                took_ms: 7021,
            },
        },
        {
            type: "run",
            end: {
                type: "ran",
                command: 3,
                exit: { type: "cancelled" },
                output: "",
                policy: "sandbox",
                took_ms: 6004,
            },
        },
    ];
    for (const [n, outcome] of later.entries()) {
        const call = runs[n + 2];
        assert.ok(call !== undefined);
        await agent.actions.finished(agent.by(from.workspace), {
            call,
            outcome,
        });
        await agent.idle();
    }

    const items = (await read(agent)).items;
    const [failed, timedOut, ran, stopped] = ends(items);
    assert.deepEqual([failed?.[1], failed?.[3]], [3, "oops"]);
    assert.equal(timedOut?.[1], -1 >>> 0);
    assert.deepEqual(ran, ["bash", 0, 7021, undefined]);
    assert.deepEqual(stopped, ["bash", 1, undefined, "Tool bash was aborted"]);
    assert.deepEqual(
        items.filter((item) => item.kind === "user").map((item) => item.text),
        ["Run them."],
    );
});

test("a ruling closes its call's item in the watched chat after a restart, however long the chat grew", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { ask_chief: () => ({ type: "done" }) },
    });
    const agent = await wake(
        t,
        [
            toolCall("ask_chief", {
                questions: [{ question: "Which colour?" }],
            }),
            ...Array.from({ length: 31 }, (_, n) =>
                fauxAssistantMessage(`Answer ${n}.`),
            ),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Paint it." });
    for (let n = 0; n < 30; n += 1) {
        await send(agent, { type: "line", text: `Line ${n}.` });
    }
    await agent.stop();

    // The wake's watch sees the newest 50 entries, which leave the call out.
    const woken = await agent.again();
    t.after(() => woken.stop());
    await read(woken);
    await send(woken, {
        type: "notice",
        text: "The chief answers: Blue.",
        held: { call: callOf(sent, "ask_chief"), end: "ran" },
    });

    await until(() => answered(added(woken)).length > 0);
    assert.deepEqual(
        tools(added(woken).filter((item) => item.tool !== undefined)),
        [["ask_chief", 0, ""]],
    );
    assert.deepEqual(
        added(woken).filter((item) => item.kind === "notice"),
        [],
    );
});

test("a held edit a chief refused closes with no change, as today's", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            edit_file: () => ({ type: "waits_for_chief", needs: ["edit"] }),
        },
    });
    const agent = await wake(
        t,
        [
            toolCall(
                "edit",
                { path: "/work/a.md", edits: [{ oldText: "a", newText: "b" }] },
                "ce",
            ),
            fauxAssistantMessage("Waiting on the chief."),
            fauxAssistantMessage("Kept."),
        ],
        calls,
    );
    await send(agent, { type: "line", text: "Edit it." });
    const opened = (await read(agent)).items.find(
        (item) => item.tool?.name === "edit",
    );
    assert.ok(opened?.tool?.change !== undefined, "the add shows the edit");

    await agent.actions.finished(agent.by(from.workspace), {
        call: callOf(sent, "edit_file"),
        outcome: {
            type: "change",
            end: {
                type: "not_run",
                why: { type: "chief_refused", reason: "keep it" },
                words: "a chief refused the call: keep it",
            },
        },
    });
    await agent.idle();

    const closing = agent.events
        .filter(([name]) => name === "chat")
        .flatMap(([, event]) => (event as ChatEvent).ops)
        .flatMap((op) =>
            op.type === "add" && op.item.tool?.exit !== undefined ? [op] : [],
        );
    assert.deepEqual(
        closing.map((op) => [op.item.tool?.exit, op.item.tool?.change]),
        [[1, undefined]],
    );
});

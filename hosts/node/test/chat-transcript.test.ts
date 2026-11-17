import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { isDeepStrictEqual } from "node:util";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import {
    type AssistantMessage,
    createAssistantMessageEventStream,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
    type Models,
} from "@earendil-works/pi-ai";
import type { Harness } from "@earendil-works/pi-durable";
import { UserError } from "../src/actor/actor/mod.ts";
import type {
    ChatEvent,
    ChatItem,
    ChatOp,
    ChatPage,
    ChatRef,
    Input,
} from "../src/agent/messages.ts";
import { REFLECT } from "../src/agent/reflect.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";

const CHIEF: ChatRef = { with: 7 };
const LETTERS: ChatRef = { with: 7, from: 3, kind: "letters" };

function applied(
    items: readonly ChatItem[],
    ops: readonly ChatOp[],
): ChatItem[] {
    let kept = [...items];
    for (const op of ops) {
        switch (op.type) {
            case "add": {
                if (op.replaces !== undefined) {
                    kept = kept.filter((item) => item.id !== op.replaces);
                }
                const at = kept.findIndex((item) => item.id === op.item.id);
                if (at === -1) kept.push(op.item);
                else kept[at] = op.item;
                break;
            }
            case "append": {
                const at = kept.findIndex((item) => item.id === op.item);
                const item = kept[at];
                assert.ok(item !== undefined, `no item ${op.item}`);
                // `at` counts the UTF-8 bytes of the text before it.
                const before = Buffer.from(item.text).subarray(0, op.at);
                kept[at] = { ...item, text: `${before.toString()}${op.text}` };
                break;
            }
            case "reset":
                throw new Error("a reset: the client reads again");
            case "status":
            case "usage":
                break;
        }
    }
    return kept;
}

class Followed {
    readonly chat: ChatRef;
    items: ChatItem[];
    epoch: number;
    number: number;

    constructor(chat: ChatRef, page: ChatPage) {
        this.chat = chat;
        this.items = [...page.items];
        this.epoch = page.epoch;
        this.number = page.number;
    }

    /** False when the client must read again. */
    take(event: ChatEvent): boolean {
        if (event.epoch === this.epoch && event.number <= this.number) {
            return true;
        }
        if (event.epoch !== this.epoch || event.number !== this.number + 1) {
            return false;
        }
        this.number = event.number;
        if (!isDeepStrictEqual(event.chat, this.chat)) return true;
        if (event.ops.some((op) => op.type === "reset")) return false;
        this.items = applied(this.items, event.ops);
        return true;
    }
}

function chatEvents(agent: Woken): ChatEvent[] {
    return agent.events
        .filter(([name]) => name === "chat")
        .map(([, event]) => event as ChatEvent);
}

function read(agent: Woken, chat: ChatRef, before?: string): Promise<ChatPage> {
    return agent.actions.read_chat(agent.by(from.client), {
        id: "11111111-1111-4111-8111-111111111111",
        chat,
        ...(before === undefined ? {} : { before }),
    });
}

function shown(items: readonly ChatItem[]): string[] {
    return items.map((item) =>
        item.tool === undefined
            ? `${item.kind}: ${item.text}`
            : `tool ${item.tool.name}: exit ${item.tool.exit ?? "none"}`,
    );
}

/**
 * Answers the reflection a `/new` or a `/resume` asks "Kept.", and every
 * other request from `answers` in order, whichever conversation asks first.
 */
function reflecting(answers: string[]): FauxResponseStep[] {
    const step: FauxResponseStep = (request) => {
        const last = request.messages.at(-1);
        const text =
            typeof last?.content === "string"
                ? last.content
                : (last?.content ?? [])
                      .map((part) => (part.type === "text" ? part.text : ""))
                      .join("");
        return fauxAssistantMessage(
            text === REFLECT ? "Kept." : (answers.shift() ?? "Nothing left."),
        );
    };
    return Array.from({ length: answers.length + 2 }, () => step);
}

async function wake(
    t: TestContext,
    responses: FauxResponseStep[],
    models?: (models: Models) => Models,
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files: fakeFiles({}),
        responses,
        ...(models === undefined ? {} : { models }),
    });
    t.after(() => agent.stop());
    return agent;
}

/** pi-durable admits a request id once, over every wake. */
let inputs = 0;

function sender(agent: Woken) {
    return async (input: Input) => {
        inputs += 1;
        await agent.actions.submit(agent.by(from.realm), {
            id: `input-${inputs}`,
            input,
        });
        await agent.idle();
    };
}

const line = (text: string): Input => ({ type: "line", text });
const letter = (text: string): Input => ({
    type: "letter",
    from: { agent: 3, name: "Ada" },
    text,
});

test("a client reading after a gap misses nothing", async (t) => {
    const path = `${FAKE_ROOT}/memory/plan.md`;
    const agent = await wake(t, [
        fauxAssistantMessage("One."),
        toolCall("write", { path, content: "a\nb\n" }, "c1"),
        toolCall(
            "edit",
            { path, edits: [{ oldText: "b\n", newText: "B\n" }] },
            "c2",
        ),
        fauxAssistantMessage("Two."),
    ]);
    const send = sender(agent);
    await send(line("a"));
    const first = await read(agent, CHIEF);
    assert.deepEqual(shown(first.items), ["user: a", "assistant: One."]);

    // The client misses every event from here on.
    await send(line("b"));
    const again = await read(agent, CHIEF);
    assert.deepEqual(shown(again.items), [
        "user: a",
        "assistant: One.",
        "user: b",
        "tool write: exit 0",
        "tool edit: exit 0",
        "assistant: Two.",
    ]);
    assert.ok(again.number > first.number);
    assert.equal(again.epoch, first.epoch);
    assert.equal(again.session, 1);
    assert.equal(again.items[0]?.rewindable, true);
    // As `crates/hamlet_server/src/work/cut.rs` shows an edit.
    assert.deepEqual(again.items[4]?.tool?.change, {
        path,
        created: false,
        added: 1,
        removed: 1,
        total: 2,
        count: "1 line added, 1 removed",
        lines: [
            { mark: "removed", text: "b" },
            { mark: "added", text: "B" },
        ],
    });
    assert.equal(
        again.items[3]?.tool?.summary,
        JSON.stringify({ content: "a\nb\n", path }),
    );
    assert.equal(again.working, false);
    assert.equal(again.pending, 0);

    // What the client missed is what the events carried.
    const followed = new Followed(CHIEF, first);
    for (const event of chatEvents(agent)) {
        assert.ok(followed.take(event), `event ${event.number} applies`);
    }
    assert.deepEqual(followed.items, again.items);
    assert.equal(followed.number, again.number);
});

test("events number one more each over every chat, and a new wake starts a new epoch", async (t) => {
    const agent = await wake(t, [
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Dear Ada."),
        fauxAssistantMessage("Two."),
    ]);
    const send = sender(agent);
    const before = await read(agent, CHIEF);
    assert.deepEqual(before.items, []);
    await send(line("a"));
    await send(letter("Hello."));

    const events = chatEvents(agent);
    assert.deepEqual(
        events.map((event) => event.number),
        events.map((_, at) => before.number + at + 1),
    );
    assert.ok(events.every((event) => event.epoch === before.epoch));
    const chats = new Set(events.map((event) => JSON.stringify(event.chat)));
    assert.deepEqual(
        [...chats].sort(),
        [JSON.stringify(CHIEF), JSON.stringify(LETTERS)].sort(),
    );
    assert.deepEqual(shown((await read(agent, LETTERS)).items), [
        "user: A letter from Ada (`hire#3`):\n\nHello.",
        "assistant: Dear Ada.",
    ]);

    await agent.stop();
    const next = await agent.again();
    t.after(() => next.stop());
    const page = await read(next, CHIEF);
    assert.equal(page.epoch, before.epoch + 1);
    assert.equal(page.number, 0);
    assert.deepEqual(shown(page.items), ["user: a", "assistant: One."]);
    await sender(next)(line("b"));
    const later = chatEvents(next);
    assert.ok(later.length > 0);
    assert.deepEqual(
        later.map((event) => [event.epoch, event.number]),
        later.map((_, at) => [page.epoch, at + 1]),
    );
});

function held(gates: readonly Promise<void>[]): (models: Models) => Models {
    let first = true;
    return (models) =>
        new Proxy(models, {
            get(target, key, receiver) {
                if (key === "streamSimple") {
                    return (...args: Parameters<Models["streamSimple"]>) => {
                        const source = target.streamSimple(...args);
                        if (!first) return source;
                        first = false;
                        const out = createAssistantMessageEventStream();
                        void (async () => {
                            const done = await source.result();
                            const partial = (
                                text: string,
                            ): AssistantMessage => ({
                                ...done,
                                content: [{ type: "text", text }],
                            });
                            out.push({ type: "start", partial: partial("") });
                            out.push({
                                type: "text_start",
                                contentIndex: 0,
                                partial: partial(""),
                            });
                            let text = "";
                            for (const [at, delta] of [
                                "Héllo ",
                                "wörld",
                                "!",
                            ].entries()) {
                                if (at > 0) await gates[at - 1];
                                text += delta;
                                out.push({
                                    type: "text_delta",
                                    contentIndex: 0,
                                    delta,
                                    partial: partial(text),
                                });
                            }
                            out.push({
                                type: "text_end",
                                contentIndex: 0,
                                content: text,
                                partial: partial(text),
                            });
                            out.push({
                                type: "done",
                                reason: "stop",
                                message: done,
                            });
                            out.end(done);
                        })();
                        return out;
                    };
                }
                const value: unknown = Reflect.get(target, key, receiver);
                return typeof value === "function" ? value.bind(target) : value;
            },
        });
}

test("an append repeated, or crossing a read, applies once", async (t) => {
    const gates = [
        Promise.withResolvers<void>(),
        Promise.withResolvers<void>(),
    ];
    // A failure before the gates open would leave the run held at the stop.
    t.after(() => {
        for (const gate of gates) gate.resolve();
    });
    const agent = await wake(
        t,
        [fauxAssistantMessage("Héllo wörld!")],
        held(gates.map((gate) => gate.promise)),
    );
    await agent.actions.submit(agent.by(from.realm), {
        id: "held-1",
        input: line("a"),
    });
    const sent = (wanted: (op: ChatOp) => boolean) =>
        chatEvents(agent).some((event) => event.ops.some(wanted));
    // A partial is committed within 100 ms; `until` waits 5 s.
    await until(() =>
        sent((op) => op.type === "add" && op.item.text === "Héllo "),
    );
    const page = await read(agent, CHIEF);
    assert.equal(page.working, true);
    assert.deepEqual(shown(page.items), ["user: a", "assistant: Héllo "]);
    gates[0]?.resolve();
    await until(() => sent((op) => op.type === "append"));
    gates[1]?.resolve();
    await agent.idle();

    const events = chatEvents(agent);
    const once = new Followed(CHIEF, page);
    for (const event of events) assert.ok(once.take(event));
    // Every event, those the page already holds too, applied twice.
    let twice = [...page.items];
    for (const event of events) {
        if (isDeepStrictEqual(event.chat, CHIEF)) {
            twice = applied(applied(twice, event.ops), event.ops);
        }
    }
    const now = await read(agent, CHIEF);
    assert.deepEqual(shown(now.items), ["user: a", "assistant: Héllo wörld!"]);
    assert.equal(now.working, false);
    assert.deepEqual(once.items, now.items);
    assert.deepEqual(twice, now.items);
});

test("read_chat of a chat that does not exist is refused as today words it", async (t) => {
    const agent = await wake(t, [fauxAssistantMessage("One.")]);
    const refused = (words: string) => (error: unknown) =>
        error instanceof UserError && error.message === words;
    // As `crates/hamlet_server/src/society/refusal.rs` words them.
    await assert.rejects(
        read(agent, { with: 7, past: 9 }),
        refused("there is no past session 9 here"),
    );
    await assert.rejects(
        read(agent, { with: 7, from: 3 }),
        refused("no agent has the address `3's letters to 7`"),
    );
    await assert.rejects(
        read(agent, { with: 7, from: 3, kind: "questions" }),
        refused("no agent has the address `3's questions to 7`"),
    );
    await sender(agent)(line("a"));
    // As `crates/hamlet_server/src/clients/requests.rs` words it.
    await assert.rejects(
        read(agent, CHIEF, "nothing"),
        refused("There is no item `nothing` here."),
    );
});

test("a page before a cursor holds the items before it", async (t) => {
    // No reflection every ten lines, so the page holds lines and answers only.
    const every = process.env.HAMLET_REFLECT_EVERY;
    process.env.HAMLET_REFLECT_EVERY = "0";
    t.after(() => {
        if (every === undefined) delete process.env.HAMLET_REFLECT_EVERY;
        else process.env.HAMLET_REFLECT_EVERY = every;
    });
    const agent = await wake(
        t,
        Array.from({ length: 60 }, (_, at) => fauxAssistantMessage(`A${at}.`)),
    );
    const send = sender(agent);
    for (let at = 0; at < 60; at += 1) await send(line(`L${at}`));
    const last = await read(agent, CHIEF);
    assert.equal(last.items.length, 100);
    assert.equal(last.items[0]?.text, "L10");
    assert.equal(last.earlier, last.items[0]?.id);
    const earlier = await read(agent, CHIEF, last.earlier);
    assert.equal(earlier.items.length, 20);
    assert.deepEqual(shown(earlier.items).slice(0, 2), [
        "user: L0",
        "assistant: A0.",
    ]);
    assert.equal(earlier.earlier, undefined);
});

test("a harness reopened after a failed commit resets its chats and numbers on", async (t) => {
    const agent = await wake(t, [
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        fauxAssistantMessage("Three."),
    ]);
    const send = sender(agent);
    await send(line("a"));
    const harness = await agent.harness();
    const failed = agent.failNextCommit();
    await agent.actions
        .submit(agent.by(from.realm), { id: "input-x", input: line("lost") })
        .catch(() => undefined);
    await failed;
    await until(async () => (await agent.harness()) !== harness);
    await until(() =>
        chatEvents(agent).some(
            (event) =>
                isDeepStrictEqual(event.chat, CHIEF) &&
                event.ops.some((op) => op.type === "reset"),
        ),
    );
    const page = await read(agent, CHIEF);
    await send(line("b"));

    const events = chatEvents(agent);
    assert.deepEqual(
        events.map((event) => event.number),
        events.map((_, at) => at + 1),
    );
    const followed = new Followed(CHIEF, page);
    for (const event of events) assert.ok(followed.take(event));
    assert.deepEqual(followed.items, (await read(agent, CHIEF)).items);
    assert.ok(shown(followed.items).includes("user: b"));
});

test("a client following the chief's chat reads again after /new", async (t) => {
    const agent = await wake(t, reflecting(["One.", "Two."]));
    const send = sender(agent);
    await send(line("a"));
    const page = await read(agent, CHIEF);
    await agent.actions.new_session(agent.by(from.client), {
        id: "new-1",
        to: 7,
    });
    await send(line("b"));

    const followed = new Followed(CHIEF, page);
    const events = chatEvents(agent).filter(
        (event) => event.number > page.number,
    );
    const stopped = events.findIndex((event) => !followed.take(event));
    assert.notEqual(stopped, -1, "the client was told to read again");
    const again = await read(agent, CHIEF);
    assert.equal(again.session, 2);
    assert.deepEqual(shown(again.items), [
        "notice: Started a new session.",
        "user: b",
        "assistant: Two.",
    ]);
    await agent.idle();
    assert.deepEqual(shown((await read(agent, { with: 7, past: 1 })).items), [
        "user: a",
        "assistant: One.",
        "notice: reflection",
        "assistant: Kept.",
    ]);
});

test("a page read after the wake could not open the harness is followed", async (t) => {
    const first = await wake(t, [
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
    ]);
    await sender(first)(line("a"));
    await first.stop();
    const failed = first.failNextCommit();
    const agent = await first.again();
    t.after(() => agent.stop());
    await failed;
    assert.ok(
        agent.reported.some((line) => line.includes("could not open on wake")),
        "the wake's open failed",
    );

    const page = await read(agent, CHIEF);
    assert.deepEqual(shown(page.items), ["user: a", "assistant: One."]);
    await sender(agent)(line("b"));
    const followed = new Followed(CHIEF, page);
    for (const event of chatEvents(agent)) assert.ok(followed.take(event));
    assert.deepEqual(followed.items, (await read(agent, CHIEF)).items);
    assert.deepEqual(shown(followed.items).slice(-2), [
        "user: b",
        "assistant: Two.",
    ]);
});

test("an ask-again after a short cut-off shows only the summary's notice", async (t) => {
    const answers = [
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        fauxAssistantMessage("Three."),
        fauxAssistantMessage([], { stopReason: "length" }),
        fauxAssistantMessage("Four."),
    ];
    const step: FauxResponseStep = async (request) =>
        request.messages.some((message) =>
            wordsOf(message).startsWith("<conversation>"),
        )
            ? fauxAssistantMessage("Summary.")
            : (answers.shift() ?? fauxAssistantMessage("none"));
    const agent = await wake(
        t,
        Array.from({ length: 20 }, () => step),
    );
    for (const n of [1, 2, 3, 4]) {
        await agent.actions.submit(agent.by(from.realm), {
            id: `cut-${n}`,
            task: `task-${n}`,
            input: line(
                n < 4 ? `Line ${n}. ${"word ".repeat(7_000)}` : "Line 4.",
            ),
        });
        await agent.idle();
    }
    const page = await read(agent, CHIEF);
    assert.deepEqual(shown(page.items).slice(-3), [
        "user: Line 4.",
        "notice: The conversation grew longer than the model can hold, so the earlier part of it was summarised.",
        "assistant: Four.",
    ]);
});

function wordsOf(message: Message): string {
    return typeof message.content === "string"
        ? message.content
        : message.content
              .map((part) => (part.type === "text" ? part.text : ""))
              .join("");
}

test("a letter's, a question's and an answer's item names its author as today", async (t) => {
    const agent = await wakeAgent(t, {
        calls: fakeCalls({ realm: { reply: () => ({ type: "done" }) } }).calls,
        files: fakeFiles({}),
        responses: [
            fauxAssistantMessage("One."),
            fauxAssistantMessage("Dear Ada."),
            fauxAssistantMessage("Three."),
            fauxAssistantMessage("Thanks."),
        ],
    });
    t.after(() => agent.stop());
    const send = sender(agent);
    await send(line("a"));
    await send(letter("Hello."));
    await send({
        type: "question",
        letter: 5,
        from: { agent: 4 },
        text: "How many?",
    });
    const chief = await conversationOfChat(agent.by(from.client), CHIEF);
    await send({
        type: "answer",
        question: 6,
        from: { agent: 3, name: "Ada" },
        text: "Three.",
        conversation: String(chief),
    });
    // As `crates/hamlet_server/src/society/addresses.rs`'s `name_of` shows a sender.
    const authors = (items: readonly ChatItem[]) =>
        items.filter((item) => item.kind === "user").map((item) => item.author);
    assert.deepEqual(authors((await read(agent, LETTERS)).items), [
        "hire#3 (Ada)",
    ]);
    assert.deepEqual(
        authors(
            (await read(agent, { with: 7, from: 4, kind: "questions" })).items,
        ),
        ["hire#4"],
    );
    assert.deepEqual(authors((await read(agent, CHIEF)).items), [
        undefined,
        "hire#3 (Ada)",
    ]);
    const added = chatEvents(agent).flatMap((event) =>
        event.ops.flatMap((op) =>
            op.type === "add" && op.item.kind === "user" ? [op.item] : [],
        ),
    );
    assert.deepEqual(authors(added), [
        undefined,
        "hire#3 (Ada)",
        "hire#4",
        "hire#3 (Ada)",
    ]);
});

async function observed(
    t: TestContext,
    agent: Woken,
): Promise<{ count: number }> {
    const harness = await agent.harness();
    const proto = Object.getPrototypeOf(harness) as Harness;
    const original = proto.observeConversation;
    const seen = { count: 0 };
    proto.observeConversation = function (this: Harness, ...args) {
        seen.count += 1;
        return original.apply(this, args);
    };
    t.after(() => {
        proto.observeConversation = original;
    });
    return seen;
}

test("a wake follows the current session and the peers' chats, and resets a past session that changes", async (t) => {
    const first = await wake(
        t,
        Array.from({ length: 12 }, (_, at) => fauxAssistantMessage(`A${at}.`)),
    );
    const send = sender(first);
    await send(letter("Hello."));
    for (let at = 0; at < 4; at += 1) {
        await send(line(`L${at}`));
        await first.actions.new_session(first.by(from.client), {
            id: `00000000-0000-4000-8000-00000000000${at}`,
            to: 7,
        });
    }
    await send(line("L4"));
    const seen = await observed(t, first);
    await first.stop();
    const next = await first.again();
    t.after(() => next.stop());
    await read(next, CHIEF);
    await read(next, LETTERS);
    assert.equal(seen.count, 2, "the current session and the letters");

    const past: ChatRef = { with: 7, past: 1 };
    const conversation = await conversationOfChat(next.by(from.client), past);
    const answer = (text: string) =>
        sender(next)({
            type: "answer",
            question: 6,
            from: { agent: 3, name: "Ada" },
            text,
            conversation: String(conversation),
        });
    await answer("Late.");
    assert.equal(seen.count, 2, "a past session no client read is not folded");
    assert.ok(
        chatEvents(next).some(
            (event) =>
                isDeepStrictEqual(event.chat, past) &&
                event.ops.some((op) => op.type === "reset"),
        ),
        "a client that read it before reads again",
    );

    const page = await read(next, past);
    await answer("Later.");
    const followed = new Followed(past, page);
    for (const event of chatEvents(next)) assert.ok(followed.take(event));
    const now = await read(next, past);
    assert.deepEqual(followed.items, now.items);
    assert.equal(now.items.at(-2)?.text.endsWith("Later."), true);
});

test("a client following the chief's chat reads again after /resume to a session past at the wake", async (t) => {
    const first = await wake(t, reflecting(["One.", "Two.", "Three."]));
    await sender(first)(line("a"));
    await first.actions.new_session(first.by(from.client), {
        id: "new-1",
        to: 7,
    });
    await sender(first)(line("b"));
    await first.stop();
    const next = await first.again();
    t.after(() => next.stop());
    const page = await read(next, CHIEF);
    await next.actions.resume(next.by(from.client), {
        id: "back-1",
        number: 1,
        to: 7,
    });
    const followed = new Followed(CHIEF, page);
    await until(() => chatEvents(next).some((event) => !followed.take(event)));
    const again = await read(next, CHIEF);
    assert.equal(again.session, 1);
    await sender(next)(line("c"));
    const back = new Followed(CHIEF, again);
    for (const event of chatEvents(next)) assert.ok(back.take(event));
    assert.deepEqual(back.items, (await read(next, CHIEF)).items);
    assert.deepEqual(shown(back.items).slice(-2), [
        "user: c",
        "assistant: Three.",
    ]);
});

function heldWeakly(agent: Woken, text: string): WeakRef<ChatItem> {
    for (const event of chatEvents(agent)) {
        for (const op of event.ops) {
            if (op.type === "add" && op.item.text === text) {
                return new WeakRef(op.item);
            }
        }
    }
    throw new Error(`no item says ${text}`);
}

test("the watches keep no answer's text once it is sent", async (t) => {
    setFlagsFromString("--expose-gc");
    const gc = runInNewContext("gc") as () => void;
    const big = "x".repeat(50_000);
    const agent = await wake(t, [
        fauxAssistantMessage(big),
        fauxAssistantMessage("Two."),
    ]);
    await read(agent, CHIEF);
    await sender(agent)(line("a"));
    const answer = heldWeakly(agent, big);
    agent.events.length = 0;
    await until(() => {
        gc();
        return answer.deref() === undefined;
    });
    await sender(agent)(line("b"));
    assert.deepEqual(shown((await read(agent, CHIEF)).items).slice(-2), [
        "user: b",
        "assistant: Two.",
    ]);
});

test("a client following the chief's chat reads again after a rewind, then follows the fork", async (t) => {
    const agent = await wake(t, [
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
        fauxAssistantMessage("Three."),
    ]);
    const send = sender(agent);
    await send(line("a"));
    await send(line("b"));
    const page = await read(agent, CHIEF);
    const b = page.items.find((item) => item.text === "b");
    assert.ok(b !== undefined);
    const rewound = await agent.actions.rewind(agent.by(from.client), {
        id: "rewind-1",
        chat: CHIEF,
        item: b.id,
    });
    assert.equal(rewound.type, "rewound");
    const followed = new Followed(CHIEF, page);
    // Nothing is sent after the rewind: the reset comes of the move alone.
    await until(() => chatEvents(agent).some((event) => !followed.take(event)));
    const again = await read(agent, CHIEF);
    assert.deepEqual(shown(again.items), ["user: a", "assistant: One."]);
    await send(line("c"));
    const fork = new Followed(CHIEF, again);
    for (const event of chatEvents(agent)) {
        assert.ok(fork.take(event), `event ${event.number} applies`);
    }
    assert.deepEqual(fork.items, (await read(agent, CHIEF)).items);
    assert.deepEqual(shown(fork.items), [
        "user: a",
        "assistant: One.",
        "user: c",
        "assistant: Three.",
    ]);
});

test("a client following the chief's chat reads again after a rewind to the session's first line", async (t) => {
    const agent = await wake(t, [
        fauxAssistantMessage("One."),
        fauxAssistantMessage("Two."),
    ]);
    await sender(agent)(line("a"));
    const page = await read(agent, CHIEF);
    const a = page.items.find((item) => item.text === "a");
    assert.ok(a !== undefined);
    const rewound = await agent.actions.rewind(agent.by(from.client), {
        id: "rewind-1",
        chat: CHIEF,
        item: a.id,
    });
    assert.equal(rewound.type, "rewound");
    const followed = new Followed(CHIEF, page);
    // Nothing is sent after the rewind: the reset comes of the move alone.
    await until(() => chatEvents(agent).some((event) => !followed.take(event)));
    const again = await read(agent, CHIEF);
    assert.deepEqual(again.items, []);
    await sender(agent)(line("b"));
    const fork = new Followed(CHIEF, again);
    for (const event of chatEvents(agent)) assert.ok(fork.take(event));
    assert.deepEqual(shown(fork.items), ["user: b", "assistant: Two."]);
});

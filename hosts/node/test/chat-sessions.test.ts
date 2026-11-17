import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { ConversationId, EntryRecord } from "@earendil-works/pi-durable";
import type { Calls } from "../src/actor/actor/mod.ts";
import type { Input } from "../src/agent/messages.ts";
import {
    fakeCalls,
    from,
    named,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

interface Shown {
    readonly kind: string;
    readonly text: string;
}

function shown(entry: EntryRecord): Shown {
    const notice = entry.data as { text?: unknown } | undefined;
    const text =
        entry.kind === "hamlet.notice" && typeof notice?.text === "string"
            ? notice.text
            : (entry.model ?? [])
                  .flatMap((message) =>
                      typeof message.content === "string"
                          ? [message.content]
                          : message.content.flatMap((part) =>
                                part.type === "text" ? [part.text] : [],
                            ),
                  )
                  .join("");
    return { kind: entry.kind, text };
}

/** Every conversation of the agent with what it shows, and the root's id. */
async function chats(
    agent: Woken,
): Promise<{ root: ConversationId; all: Map<ConversationId, Shown[]> }> {
    const harness = await agent.harness();
    const root = (await harness.root(context)).id;
    const ids = await harness.commit(
        async (tx) =>
            (await tx.scanConversations({}, 100)).items.map((c) => c.id),
        context,
    );
    const all = new Map<ConversationId, Shown[]>();
    for (const id of ids) {
        const page = await (await harness.conversation(id, context))?.entries(
            { order: "ascending" },
            500,
            undefined,
            context,
        );
        all.set(id, (page?.items ?? []).map(shown));
    }
    return { root, all };
}

function holding(
    all: Map<ConversationId, Shown[]>,
    text: string,
): ConversationId[] {
    return [...all]
        .filter(([, entries]) => entries.some((entry) => entry.text === text))
        .map(([id]) => id);
}

/** What a conversation shows a person: its inputs, answers and notices. */
function said(all: Map<ConversationId, Shown[]>, id: ConversationId): string[] {
    return (all.get(id) ?? [])
        .filter((entry) => entry.text !== "")
        .map((entry) => entry.text);
}

async function wake(
    t: TestContext,
    calls: Calls,
    answers: readonly string[],
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        responses: answers.map((text) => fauxAssistantMessage(text)),
    });
    t.after(() => agent.stop());
    return agent;
}

function sender(agent: Woken) {
    let n = 0;
    return async (input: Input) => {
        n += 1;
        await agent.actions.submit(agent.by(from.realm), {
            id: `input-${n}`,
            input,
        });
        await agent.idle();
    };
}

const line = (text: string): Input => ({ type: "line", text });

test("a repeated /new answers the same number", async (t) => {
    const agent = await wake(t, fakeCalls({}).calls, ["One.", "Two."]);
    const send = sender(agent);
    await send(line("a"));
    const client = agent.by(from.client);

    assert.deepEqual(
        await agent.actions.new_session(client, { id: "new-1", to: 7 }),
        { number: 2 },
    );
    assert.deepEqual(
        await agent.actions.new_session(client, { id: "new-1", to: 7 }),
        { number: 2 },
    );
    await send(line("b"));

    const { root, all } = await chats(agent);
    const [second, ...more] = holding(all, "Started a new session.");
    assert.ok(second !== undefined && more.length === 0);
    assert.notEqual(second, root);
    assert.deepEqual(said(all, second), [
        "Started a new session.",
        "b",
        "Two.",
    ]);
    assert.deepEqual(said(all, root), ["a", "One."]);
    assert.deepEqual(
        await agent.actions.new_session(client, { id: "new-2", to: 7 }),
        { number: 3 },
    );
});

// `crates/hamlet_acceptance/tests/chats.rs`,
// `sessions_are_numbered_and_come_back`. The reflection it shows ("Kept
// it.") is `reflect.ts`'s; "Session 2 began." and "Back in session 1." are
// the front door's words for these answers.
test("A2: sessions are numbered and come back", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wake(t, calls, ["One.", "Two.", "Three."]);
    const send = sender(agent);
    const client = agent.by(from.client);
    await send(line("a"));
    assert.deepEqual(
        await agent.actions.new_session(client, { id: "new-1", to: 7 }),
        { number: 2 },
    );
    await send(line("b"));

    const { sessions } = await agent.actions.read_history(client);
    assert.deepEqual(
        sessions.map((s) => [s.number, s.current, s.title, s.messages]),
        [
            [2, true, "b", 2],
            [1, false, "a", 2],
        ],
    );
    assert.deepEqual(
        await agent.actions.resume(client, { id: "back-1", number: 1, to: 7 }),
        { type: "back", number: 1 },
    );
    await send(line("c"));
    assert.equal(
        (await agent.actions.resume(client, { id: "back-2", number: 9, to: 7 }))
            .type,
        "refused",
    );

    const { root, all } = await chats(agent);
    const [second] = holding(all, "Started a new session.");
    assert.ok(second !== undefined);
    assert.deepEqual(said(all, root), [
        "a",
        "One.",
        "Went back to an earlier session.",
        "c",
        "Three.",
    ]);
    assert.deepEqual(said(all, second), [
        "Started a new session.",
        "b",
        "Two.",
    ]);
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [
            { conversation: String(root), why: "ended" },
            { conversation: String(root), why: "resumed" },
        ],
    );
});

test("a letter and a question from one peer go to two chats of their own", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { reply: () => ({ type: "done" }) },
    });
    const agent = await wake(t, calls, ["Read it.", "Two of them.", "Again."]);
    const send = sender(agent);
    const ada = { agent: 12, name: "Ada" };
    await send({ type: "letter", from: ada, text: "Hello." });
    await send({ type: "question", letter: 5, from: ada, text: "How many?" });
    await send({ type: "letter", from: ada, text: "Once more." });

    const { root, all } = await chats(agent);
    const [letters, ...moreLetters] = holding(all, "Read it.");
    const [questions, ...moreQuestions] = holding(all, "Two of them.");
    assert.ok(letters !== undefined && moreLetters.length === 0);
    assert.ok(questions !== undefined && moreQuestions.length === 0);
    assert.equal(new Set([root, letters, questions]).size, 3);
    assert.deepEqual(said(all, root), []);
    assert.deepEqual(holding(all, "Again."), [letters]);
    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [{ question: 5, text: "Two of them." }],
    );
});

test("/resume of a session that does not exist is refused with today's words", async (t) => {
    const agent = await wake(t, fakeCalls({}).calls, ["One."]);
    const client = agent.by(from.client);
    const refusal = (number: number) => ({
        type: "refused",
        why: { type: "no_such_session", number },
        words: `there is no past session ${number} here`,
    });
    assert.deepEqual(
        await agent.actions.resume(client, { id: "r-1", number: 1, to: 7 }),
        refusal(1),
    );
    await sender(agent)(line("a"));
    assert.deepEqual(
        await agent.actions.resume(client, { id: "r-2", number: 2, to: 7 }),
        refusal(2),
    );
    assert.deepEqual(
        await agent.actions.resume(client, { id: "r-3", number: 1, to: 7 }),
        { type: "back", number: 1 },
    );
});

test("/new lapses the session it leaves", async (t) => {
    let fails = true;
    const { calls, sent } = fakeCalls({
        realm: {
            lapse: () => {
                if (fails) throw new Error("the realm is away");
                return null;
            },
        },
    });
    const agent = await wake(t, calls, ["One."]);
    const client = agent.by(from.client);
    await sender(agent)(line("a"));
    const { root } = await chats(agent);

    assert.deepEqual(
        await agent.actions.new_session(client, { id: "new-1", to: 7 }),
        { number: 2 },
    );
    assert.equal(agent.reported.length, 1);
    assert.match(agent.reported[0] ?? "", /the realm is away/);
    fails = false;
    assert.deepEqual(
        await agent.actions.new_session(client, { id: "new-2", to: 7 }),
        { number: 3 },
    );
    const { all } = await chats(agent);
    const [second] = holding(all, "Started a new session.");
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [
            { conversation: String(root), why: "ended" },
            { conversation: String(second), why: "ended" },
        ],
    );
});

test("the history titles a session by its line's first words, cut at 90, and leaves out one with no line", async (t) => {
    const agent = await wake(t, fakeCalls({}).calls, ["One."]);
    const client = agent.by(from.client);
    const long = "x".repeat(95);
    await sender(agent)(line(`\n  \n  ${long}  \nmore`));
    await agent.actions.new_session(client, { id: "new-1", to: 7 });

    const { sessions } = await agent.actions.read_history(client);
    assert.deepEqual(
        sessions.map((s) => [s.number, s.current, s.title, s.tokens]),
        [[1, false, `${"x".repeat(90)}...`, "0"]],
    );
});

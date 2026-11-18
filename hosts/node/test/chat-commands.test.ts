import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import {
    type AssistantMessage,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Message,
} from "@earendil-works/pi-ai";
import type {
    ConversationId,
    EntryRecord,
    HarnessSettings,
} from "@earendil-works/pi-durable";
import type { Calls } from "../src/actor/actor/mod.ts";
import type { Finished, Input, ReadAnswer } from "../src/agent/messages.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    fakeCalls,
    from,
    itemOfLine,
    named,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

function held(): {
    readonly step: FauxResponseStep;
    readonly asked: Promise<readonly Message[]>;
    answer(message: AssistantMessage): void;
} {
    const asked = Promise.withResolvers<readonly Message[]>();
    const answer = Promise.withResolvers<AssistantMessage>();
    return {
        step: (request, options) => {
            asked.resolve(request.messages);
            const aborted = Promise.withResolvers<AssistantMessage>();
            options?.signal?.addEventListener(
                "abort",
                () =>
                    aborted.resolve(
                        fauxAssistantMessage("", {
                            stopReason: "aborted",
                            errorMessage: "aborted",
                        }),
                    ),
                { once: true },
            );
            return Promise.race([answer.promise, aborted.promise]);
        },
        asked: asked.promise,
        answer: answer.resolve,
    };
}

function seen(asked: (readonly Message[])[], text: string): FauxResponseStep {
    return (request) => {
        asked.push(request.messages);
        return fauxAssistantMessage(text);
    };
}

function textOf(message: Message): string {
    return typeof message.content === "string"
        ? message.content
        : message.content
              .flatMap((part) => (part.type === "text" ? [part.text] : []))
              .join("");
}

function shown(entry: EntryRecord): string {
    const notice = entry.data as { text?: unknown } | undefined;
    return entry.kind === "hamlet.notice" && typeof notice?.text === "string"
        ? notice.text
        : (entry.model ?? []).map(textOf).join("");
}

async function entriesOf(
    agent: Woken,
    id: ConversationId,
): Promise<EntryRecord[]> {
    const harness = await agent.harness();
    const page = await (await harness.conversation(id, context))?.entries(
        { order: "ascending" },
        500,
        undefined,
        context,
    );
    return [...(page?.items ?? [])];
}

async function said(agent: Woken, id: ConversationId): Promise<string[]> {
    return (await entriesOf(agent, id))
        .map(shown)
        .filter((text) => text !== "");
}

async function now(agent: Woken): Promise<ConversationId> {
    const id = await conversationOfChat(agent.by(from.client), { with: 7 });
    assert.ok(id !== undefined, "the chief has a current session");
    return id;
}

async function lettersOf(agent: Woken): Promise<ConversationId> {
    const id = await conversationOfChat(agent.by(from.client), {
        with: 7,
        from: ADA.agent,
    });
    assert.ok(id !== undefined, "Ada's letters have a chat");
    return id;
}

async function wake(
    t: TestContext,
    calls: Calls,
    responses: FauxResponseStep[],
    settings?: HarnessSettings,
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        responses,
        ...(settings === undefined ? {} : { settings }),
    });
    t.after(() => agent.stop());
    return agent;
}

let inputs = 0;

function submitted(agent: Woken, input: Input): Promise<null> {
    inputs += 1;
    return agent.actions.submit(agent.by(from.realm), {
        id: `input-${inputs}`,
        input,
    });
}

async function told(agent: Woken, text: string): Promise<void> {
    await submitted(agent, line(text));
    await agent.idle();
}

const line = (text: string): Input => ({ type: "line", text });

const ADA = { agent: 12, name: "Ada" };

const NO_RETRIES: HarnessSettings = { retry: { maxRetries: 0 } };

/** Answers `cancel` and `open_task`, which `fakeCalls` does not. */
function fakes(answers: Parameters<typeof fakeCalls>[0] = {}) {
    return fakeCalls({
        workspace: {
            cancel: () => ({ commands: [] }),
            ...answers.workspace,
        },
        realm: {
            open_task: () => ({ type: "opened", task: "task-retry" }),
            ...answers.realm,
        },
    });
}

function failure(words: string): AssistantMessage {
    return fauxAssistantMessage("", {
        stopReason: "error",
        errorMessage: words,
    });
}

function slowRead() {
    const read = Promise.withResolvers<ReadAnswer>();
    const reading = Promise.withResolvers<void>();
    return {
        read_file: () => {
            reading.resolve();
            return read.promise;
        },
        reading: reading.promise,
        answer: () => read.resolve({ type: "text", text: "notes", notes: [] }),
    };
}

test("stop ends the run and drops the queue", async (t) => {
    const { calls, sent } = fakes();
    const chief = held();
    const peer = held();
    const asked: (readonly Message[])[] = [];
    const agent = await wake(t, calls, [
        chief.step,
        peer.step,
        seen(asked, "After."),
    ]);
    await submitted(agent, line("Take your time"));
    await chief.asked;
    await submitted(agent, { type: "letter", from: ADA, text: "Slowly." });
    await peer.asked;
    await submitted(agent, line("And this"));
    await submitted(agent, { type: "letter", from: ADA, text: "Queued." });
    const letters = await lettersOf(agent);

    assert.deepEqual(
        await agent.actions.stop(agent.by(from.client), { id: "stop-1" }),
        { turns: 2 },
    );
    assert.deepEqual(
        named(sent, "cancel").map((call) => call.message),
        [{ which: { type: "all" } }],
    );
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [{ why: "stopped" }],
    );

    await told(agent, "Now");
    assert.equal(agent.faux.state.callCount, 3);
    const request = (asked[0] ?? []).map(textOf);
    assert.equal(request.at(-1), "Now");
    assert.ok(!request.includes("And this"), "the steered line was dropped");
    assert.ok(!(await said(agent, letters)).some((t) => t.includes("Queued.")));
});

test("a repeated stop answers 0", async (t) => {
    const { calls, sent } = fakes();
    const slow = held();
    const agent = await wake(t, calls, [slow.step]);
    await submitted(agent, line("Take your time"));
    await slow.asked;
    const client = agent.by(from.client);

    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 1,
    });
    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 0,
    });
    // Sent again, so the repeat of a stop cut off before them sends them.
    assert.equal(named(sent, "cancel").length, 2);
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [{ why: "stopped" }, { why: "stopped" }],
    );
});

test("stop ends a turn whose call is out and cancels the call", async (t) => {
    const workspace = slowRead();
    const { calls, sent } = fakes({ workspace });
    const agent = await wake(t, calls, [
        toolCall("read", { path: "/work/project/notes.md" }),
        fauxAssistantMessage("Never asked."),
    ]);
    await submitted(agent, line("Read the notes"));
    await workspace.reading;

    assert.deepEqual(
        await agent.actions.stop(agent.by(from.client), { id: "stop-1" }),
        { turns: 1 },
    );
    assert.deepEqual(
        named(sent, "cancel").map((call) => call.message),
        [{ which: { type: "all" } }],
    );
    workspace.answer();
    await agent.idle();
    assert.equal(agent.faux.state.callCount, 1);
});

// `crates/hamlet_acceptance/tests/chats.rs`, `stop_ends_a_running_turn`.
// "Stopped 1 running turns." is the front door's wording of the answer.
test("A4: stop ends a running turn", async (t) => {
    const slow = held();
    const agent = await wake(t, fakes().calls, [
        slow.step,
        fauxAssistantMessage("Quick."),
    ]);
    await submitted(agent, line("Take your time"));
    await slow.asked;
    const client = agent.by(from.client);

    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 1,
    });
    await agent.idle();
    slow.answer(fauxAssistantMessage("Slow."));
    assert.ok(!(await said(agent, await now(agent))).includes("Slow."));
    assert.deepEqual(await agent.actions.stop(client, { id: "stop-2" }), {
        turns: 0,
    });
});

test("a question turn stopped by /stop replies once that its turn was stopped", async (t) => {
    const { calls, sent } = fakes({
        realm: { reply: () => ({ type: "done" }) },
    });
    const slow = held();
    const agent = await wake(t, calls, [slow.step]);
    await submitted(agent, {
        type: "question",
        letter: 1,
        from: ADA,
        text: "How?",
    });
    await slow.asked;
    const client = agent.by(from.client);

    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 1,
    });
    await until(() => named(sent, "reply").length > 0);
    assert.deepEqual(await agent.actions.stop(client, { id: "stop-2" }), {
        turns: 0,
    });
    await agent.idle();
    await delay(100);

    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [{ question: 1, text: "It could not answer: its turn was stopped" }],
    );
    assert.deepEqual(named(sent, "turn_failed"), []);
});

test("a chief's turn stopped by /stop tells the realm nothing", async (t) => {
    const { calls, sent } = fakes();
    const slow = held();
    const agent = await wake(t, calls, [slow.step]);
    await submitted(agent, line("Take your time"));
    await slow.asked;

    assert.deepEqual(
        await agent.actions.stop(agent.by(from.client), { id: "stop-1" }),
        { turns: 1 },
    );
    await agent.idle();
    await delay(100);

    assert.deepEqual(named(sent, "turn_failed"), []);
    assert.deepEqual(named(sent, "reply"), []);
});

test("retry is refused after an answer", async (t) => {
    const slow = held();
    const agent = await wake(t, fakes().calls, [
        fauxAssistantMessage("One."),
        slow.step,
    ]);
    const client = agent.by(from.client);
    const retry = (id: string) => agent.actions.retry(client, { id, to: 7 });

    await told(agent, "a");
    // As `crates/hamlet_server/src/society/refusal.rs` words them.
    assert.deepEqual(await retry("r-1"), {
        type: "refused",
        why: { type: "answered" },
        words: "the last message was answered, so there is nothing to carry on",
    });
    await submitted(agent, line("b"));
    await slow.asked;
    assert.deepEqual(await retry("r-2"), {
        type: "refused",
        why: { type: "running" },
        words: "a turn is running here; stop it, then retry",
    });
    slow.answer(fauxAssistantMessage("Two."));
    await agent.idle();
    assert.deepEqual(await said(agent, await now(agent)), [
        "a",
        "One.",
        "b",
        "Two.",
    ]);
});

test("a repeated retry runs once", async (t) => {
    const { calls, sent } = fakes();
    const agent = await wake(
        t,
        calls,
        [failure("the model fell over"), fauxAssistantMessage("Recovered.")],
        NO_RETRIES,
    );
    await told(agent, "Go");
    const client = agent.by(from.client);
    const retry = () => agent.actions.retry(client, { id: "r-1", to: 7 });

    assert.deepEqual(await Promise.all([retry(), retry()]), [
        { type: "done" },
        { type: "done" },
    ]);
    await agent.idle();
    assert.deepEqual(await retry(), { type: "done" });
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 2);
    assert.deepEqual(
        (await said(agent, await now(agent))).filter(
            (text) => text === "Carry on from where you stopped.",
        ),
        ["Carry on from where you stopped."],
    );
    assert.ok(named(sent, "open_task").length > 0);
    assert.ok(
        named(sent, "open_task").every(
            (call) =>
                JSON.stringify(call.message) ===
                JSON.stringify({ turn: "retry" }),
        ),
    );
    assert.deepEqual(named(sent, "may_spend").at(-1)?.message, {
        task: "task-retry",
    });
});

// `crates/hamlet_acceptance/tests/chats.rs`,
// `a_failed_turn_waits_for_a_chief_and_retry_carries_on`. Its fault wait
// is the realm's, and "Carrying on." the front door's wording of `done`.
test("A3: a failed turn carries on by retry, and the earlier answer stays", async (t) => {
    const agent = await wake(
        t,
        fakes().calls,
        [
            fauxAssistantMessage("Hello."),
            failure("the model fell over"),
            fauxAssistantMessage("Recovered."),
        ],
        NO_RETRIES,
    );
    await told(agent, "Hi");
    await told(agent, "Go");
    const client = agent.by(from.client);

    assert.deepEqual(await agent.actions.retry(client, { id: "r-1", to: 7 }), {
        type: "done",
    });
    await agent.idle();
    assert.deepEqual(await said(agent, await now(agent)), [
        "Hi",
        "Hello.",
        "Go",
        "Carry on from where you stopped.",
        "Recovered.",
    ]);
});

test("a stopped turn carries on by retry", async (t) => {
    const slow = held();
    const agent = await wake(t, fakes().calls, [
        slow.step,
        fauxAssistantMessage("Done."),
    ]);
    await submitted(agent, line("Go"));
    await slow.asked;
    const client = agent.by(from.client);
    await agent.actions.stop(client, { id: "stop-1" });

    assert.deepEqual(await agent.actions.retry(client, { id: "r-1", to: 7 }), {
        type: "done",
    });
    await agent.idle();
    assert.deepEqual((await said(agent, await now(agent))).slice(-2), [
        "Carry on from where you stopped.",
        "Done.",
    ]);
});

test("a stop that arrives while a retry opens its task wins: the retry admits nothing", async (t) => {
    const opening = Promise.withResolvers<{ type: "opened"; task: string }>();
    const asked = Promise.withResolvers<void>();
    const { calls } = fakes({
        realm: {
            open_task: () => {
                asked.resolve();
                return opening.promise;
            },
        },
    });
    const agent = await wake(
        t,
        calls,
        [failure("The model failed."), fauxAssistantMessage("Carried on.")],
        NO_RETRIES,
    );
    await told(agent, "Go");
    const client = agent.by(from.client);
    const retrying = agent.actions.retry(client, { id: "retry-1", to: 7 });
    await asked.promise;

    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 0,
    });
    opening.resolve({ type: "opened", task: "task-retry" });
    assert.deepEqual(await retrying, { type: "done" });
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 1);
    assert.ok(!(await said(agent, await now(agent))).includes("Carried on."));
});

test("a failed turn rewound away is not carried on", async (t) => {
    const agent = await wake(
        t,
        fakes().calls,
        [fauxAssistantMessage("First."), failure("the model fell over")],
        NO_RETRIES,
    );
    await told(agent, "One");
    await told(agent, "Two");
    const client = agent.by(from.client);
    await agent.actions.rewind(client, {
        id: "rw-1",
        chat: { with: 7 },
        item: await itemOfLine(agent, "Two"),
    });

    assert.equal(
        (await agent.actions.retry(client, { id: "r-1", to: 7 })).type,
        "refused",
    );
});

test("rewind forks before the line", async (t) => {
    const { calls, sent } = fakes();
    const asked: (readonly Message[])[] = [];
    const agent = await wake(t, calls, [
        fauxAssistantMessage("First."),
        fauxAssistantMessage("Second."),
        seen(asked, "Third."),
    ]);
    await told(agent, "One");
    await told(agent, "Two");
    const before = await now(agent);
    const client = agent.by(from.client);

    assert.deepEqual(
        await agent.actions.rewind(client, {
            id: "rw-1",
            chat: { with: 7 },
            item: await itemOfLine(agent, "Two"),
        }),
        { type: "rewound", text: "Two" },
    );
    const after = await now(agent);
    assert.notEqual(after, before);
    assert.deepEqual(await said(agent, after), ["One", "First."]);
    assert.deepEqual(await said(agent, before), [
        "One",
        "First.",
        "Two",
        "Second.",
    ]);
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [{ conversation: String(before), why: "rewound" }],
    );

    await told(agent, "Two, again");
    const chat = ["One", "First.", "Two", "Second.", "Two, again"];
    assert.deepEqual(
        (asked[0] ?? []).map(textOf).filter((text) => chat.includes(text)),
        ["One", "First.", "Two, again"],
    );
});

test("a rewind of a session's first line leaves it empty", async (t) => {
    const asked: (readonly Message[])[] = [];
    const agent = await wake(t, fakes().calls, [
        fauxAssistantMessage("First."),
        seen(asked, "Again."),
    ]);
    await told(agent, "One");

    assert.deepEqual(
        await agent.actions.rewind(agent.by(from.client), {
            id: "rw-1",
            chat: { with: 7 },
            item: await itemOfLine(agent, "One"),
        }),
        { type: "rewound", text: "One" },
    );
    assert.deepEqual(await said(agent, await now(agent)), []);
    await told(agent, "One more");
    const request = (asked[0] ?? []).map(textOf);
    assert.equal(request.at(-1), "One more");
    assert.ok(!request.includes("One") && !request.includes("First."));
});

test("a rewind repeated after it landed is refused no_such_item", async (t) => {
    const { calls, sent } = fakes();
    const agent = await wake(t, calls, [
        fauxAssistantMessage("First."),
        fauxAssistantMessage("Second."),
    ]);
    await told(agent, "One");
    await told(agent, "Two");
    const client = agent.by(from.client);
    const rewind = {
        id: "rw-1",
        chat: { with: 7 },
        item: await itemOfLine(agent, "Two"),
    };

    assert.equal((await agent.actions.rewind(client, rewind)).type, "rewound");
    const landed = await now(agent);
    assert.deepEqual(await agent.actions.rewind(client, rewind), {
        type: "refused",
        why: { type: "no_such_item", item: rewind.item },
        words: "That is not one of your lines in this conversation.",
    });
    assert.equal(await now(agent), landed);
    assert.equal(named(sent, "lapse").length, 1);
});

test("a rewind whose lapse fails moves nothing, and its repeat rewinds", async (t) => {
    let fails = true;
    const { calls, sent } = fakes({
        realm: {
            lapse: () => {
                if (fails) throw new Error("the realm is away");
                return null;
            },
        },
    });
    const agent = await wake(t, calls, [
        fauxAssistantMessage("First."),
        fauxAssistantMessage("Second."),
    ]);
    await told(agent, "One");
    await told(agent, "Two");
    const before = await now(agent);
    const client = agent.by(from.client);
    const rewind = {
        id: "rw-1",
        chat: { with: 7 },
        item: await itemOfLine(agent, "Two"),
    };

    await assert.rejects(
        agent.actions.rewind(client, rewind),
        /the realm is away/,
    );
    assert.equal(await now(agent), before);
    fails = false;
    assert.equal((await agent.actions.rewind(client, rewind)).type, "rewound");
    assert.notEqual(await now(agent), before);
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [
            { conversation: String(before), why: "rewound" },
            { conversation: String(before), why: "rewound" },
        ],
    );
});

// `crates/hamlet_acceptance/tests/chats.rs`,
// `rewind_takes_the_chat_back_to_before_a_line`.
test("A6: rewind takes the chat back to before a line", async (t) => {
    const slow = held();
    const agent = await wake(t, fakes().calls, [
        fauxAssistantMessage("First."),
        fauxAssistantMessage("Second."),
        fauxAssistantMessage("Third."),
        slow.step,
    ]);
    await told(agent, "One");
    await told(agent, "Two");
    const client = agent.by(from.client);
    const two = await itemOfLine(agent, "Two");
    const answer = (await entriesOf(agent, await now(agent))).find(
        (entry) => entry.kind === "pi.assistant",
    );
    assert.ok(answer !== undefined);
    const rewind = (item: string, chat: { past?: number } = {}) =>
        agent.actions.rewind(client, {
            id: `rw-${item}`,
            chat: { with: 7, ...chat },
            item,
        });
    // As `crates/hamlet_server/src/talk/chats.rs` and
    // `crates/hamlet_server/src/clients/requests.rs` word them.
    const notALine = "That is not one of your lines in this conversation.";

    assert.deepEqual(await rewind(`${answer.id}:0`), {
        type: "refused",
        why: { type: "not_a_chief_line" },
        words: notALine,
    });
    assert.deepEqual(await rewind("nowhere"), {
        type: "refused",
        why: { type: "no_such_item", item: "nowhere" },
        words: notALine,
    });
    assert.deepEqual(await rewind(two, { past: 1 }), {
        type: "refused",
        why: { type: "not_current" },
        words: "Only the current session of a chief's chat goes back.",
    });

    assert.deepEqual(await rewind(two), { type: "rewound", text: "Two" });
    assert.deepEqual(await said(agent, await now(agent)), ["One", "First."]);
    await told(agent, "Two, again");
    assert.deepEqual(await said(agent, await now(agent)), [
        "One",
        "First.",
        "Two, again",
        "Third.",
    ]);

    await submitted(agent, line("Three"));
    await slow.asked;
    assert.deepEqual(await rewind(await itemOfLine(agent, "Three")), {
        type: "refused",
        why: { type: "working" },
        words: "a turn is running here; stop it, then rewind",
    });
    slow.answer(fauxAssistantMessage("Fourth."));
    await agent.idle();
});

test("a chief's line joins the running turn", async (t) => {
    const workspace = slowRead();
    const asked: (readonly Message[])[] = [];
    const agent = await wake(t, fakes({ workspace }).calls, [
        toolCall("read", { path: "/work/project/notes.md" }),
        seen(asked, "Both done."),
        fauxAssistantMessage("Never asked."),
    ]);
    await submitted(agent, line("Read the notes"));
    await workspace.reading;
    await submitted(agent, line("And add a title"));
    workspace.answer();
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 2);
    assert.equal((asked[0] ?? []).map(textOf).at(-1), "And add a title");
});

test("a letter to a busy agent waits for the next run", async (t) => {
    const workspace = slowRead();
    const agent = await wake(
        t,
        fakes({ workspace, realm: { reply: () => ({ type: "done" }) } }).calls,
        [
            toolCall("read", { path: "/work/project/notes.md" }),
            fauxAssistantMessage("Read."),
            fauxAssistantMessage("Thanks."),
        ],
    );
    await submitted(agent, { type: "letter", from: ADA, text: "Read it" });
    await workspace.reading;
    await submitted(agent, { type: "letter", from: ADA, text: "Thanks" });
    workspace.answer();
    await agent.idle();

    assert.deepEqual((await said(agent, await lettersOf(agent))).slice(-3), [
        "Read.",
        "A letter from Ada (`hire#12`):\n\nThanks",
        "Thanks.",
    ]);
});

// As today, a retry of a session with nothing in it is admitted
// (`crates/hamlet_server/src/talk/chats.rs`, `retry`).
test("a retry of an empty session carries on", async (t) => {
    const agent = await wake(t, fakes().calls, [
        fauxAssistantMessage("Carried."),
    ]);

    assert.deepEqual(
        await agent.actions.retry(agent.by(from.client), { id: "r-1", to: 7 }),
        { type: "done" },
    );
    await agent.idle();
    assert.deepEqual(await said(agent, await now(agent)), [
        "Carry on from where you stopped.",
        "Carried.",
    ]);
});

test("of two retries at once, the second is refused running", async (t) => {
    const { calls, sent } = fakes();
    const slow = held();
    const agent = await wake(
        t,
        calls,
        [failure("the model fell over"), slow.step],
        NO_RETRIES,
    );
    await told(agent, "Go");
    const client = agent.by(from.client);

    const answers = await Promise.all([
        agent.actions.retry(client, { id: "r-1", to: 7 }),
        agent.actions.retry(client, { id: "r-2", to: 7 }),
    ]);
    assert.deepEqual(answers, [
        { type: "done" },
        {
            type: "refused",
            why: { type: "running" },
            words: "a turn is running here; stop it, then retry",
        },
    ]);
    slow.answer(fauxAssistantMessage("Recovered."));
    await agent.idle();
    assert.equal(named(sent, "open_task").length, 1);
    assert.deepEqual(
        (await said(agent, await now(agent))).filter(
            (text) => text === "Carry on from where you stopped.",
        ),
        ["Carry on from where you stopped."],
    );
});

test("a chief's line sent while a rewind is out lands in the session it goes to", async (t) => {
    const lapse = Promise.withResolvers<null>();
    const lapsing = Promise.withResolvers<void>();
    const { calls } = fakes({
        realm: {
            lapse: () => {
                lapsing.resolve();
                return lapse.promise;
            },
        },
    });
    const agent = await wake(t, calls, [
        fauxAssistantMessage("First."),
        fauxAssistantMessage("Second."),
        fauxAssistantMessage("Third."),
    ]);
    await told(agent, "One");
    await told(agent, "Two");
    const left = await now(agent);
    const rewinding = agent.actions.rewind(agent.by(from.client), {
        id: "rw-1",
        chat: { with: 7 },
        item: await itemOfLine(agent, "Two"),
    });
    await lapsing.promise;

    const sending = submitted(agent, line("Three"));
    // A line racing the rewind is admitted in milliseconds, so a second
    // decides only whether the race is caught, never whether it passes.
    await Promise.race([sending, delay(1_000)]);
    lapse.resolve(null);
    assert.deepEqual(await rewinding, { type: "rewound", text: "Two" });
    await sending;
    await agent.idle();
    assert.deepEqual(await said(agent, await now(agent)), [
        "One",
        "First.",
        "Three",
        "Third.",
    ]);
    assert.deepEqual(await said(agent, left), [
        "One",
        "First.",
        "Two",
        "Second.",
    ]);
});

const TESTS_FAILED: Finished["outcome"] = {
    type: "run",
    end: {
        type: "ran",
        command: 4,
        exit: { type: "code", code: 1 },
        output: "2 failed\n",
        policy: "3f2a",
        took_ms: 12_000,
    },
};

function callOfRun(sent: Parameters<typeof named>[0]): string {
    const [run] = named(sent, "run");
    assert.ok(run !== undefined, "the command was run");
    return (run.message as { call: string }).call;
}

// As today, where a turn waiting on a chief is busy (`talk/inputs.rs:273-280`,
// `talk/chats.rs:322-326`).
for (const [what, run] of [
    ["a command's end has not come", { type: "running", command: 4 }],
    [
        "a wait for a chief is open",
        { type: "waits_for_chief", needs: ["network"] },
    ],
] as const) {
    test(`a rewind while ${what} is refused working, and goes ahead once its end is admitted`, async (t) => {
        const { calls, sent } = fakes({ workspace: { run: () => run } });
        const agent = await wake(t, calls, [
            fauxAssistantMessage("First."),
            toolCall("bash", { command: "cargo test", timeout: 60 }),
            fauxAssistantMessage("It runs; I will hear its end."),
            fauxAssistantMessage("The tests failed."),
        ]);
        await told(agent, "One");
        await told(agent, "Run the tests");
        const item = await itemOfLine(agent, "Run the tests");
        const rewind = (id: string) =>
            agent.actions.rewind(agent.by(from.client), {
                id,
                chat: { with: 7 },
                item,
            });

        assert.deepEqual(await rewind("rw-1"), {
            type: "refused",
            why: { type: "working" },
            words: "a turn is running here; stop it, then rewind",
        });
        await agent.actions.finished(agent.by(from.workspace), {
            call: callOfRun(sent),
            outcome: TESTS_FAILED,
        });
        await agent.idle();

        assert.deepEqual(await rewind("rw-2"), {
            type: "rewound",
            text: "Run the tests",
        });
        assert.deepEqual(await said(agent, await now(agent)), [
            "One",
            "First.",
        ]);
    });
}

test("a command a restart cut off is told once, the model reads that it may have partially run, and a rewind goes ahead after", async (t) => {
    const asked: (readonly Message[])[] = [];
    const { calls, sent } = fakes({
        workspace: { run: () => ({ type: "running", command: 4 }) },
    });
    const agent = await wake(t, calls, [
        fauxAssistantMessage("First."),
        toolCall("bash", { command: "sleep 40" }),
        fauxAssistantMessage("It runs."),
        seen(asked, "It was cut off."),
    ]);
    await told(agent, "One");
    await told(agent, "Run it");
    const item = await itemOfLine(agent, "Run it");
    const rewind = (id: string) =>
        agent.actions.rewind(agent.by(from.client), {
            id,
            chat: { with: 7 },
            item,
        });
    const cutOff: Finished = {
        call: callOfRun(sent),
        outcome: {
            type: "run",
            end: {
                type: "ran",
                command: 4,
                exit: { type: "cut_off" },
                output: "",
                policy: "",
                took_ms: 0,
            },
        },
    };

    const before = await rewind("rw-1");
    await agent.actions.finished(agent.by(from.workspace), cutOff);
    await agent.actions.finished(agent.by(from.workspace), cutOff);
    await agent.idle();

    const words =
        "\n[Command 4: interrupted by a restart and may have partially run]";
    assert.equal(before.type, "refused");
    assert.deepEqual(
        (await entriesOf(agent, await now(agent)))
            .filter((entry) => entry.kind === "hamlet.finished")
            .map(shown),
        [words],
    );
    assert.equal(asked.length, 1);
    assert.equal(textOf(asked[0]?.at(-1) as Message), words);
    assert.deepEqual(await rewind("rw-2"), {
        type: "rewound",
        text: "Run it",
    });
});

test("a rewind while a question has no answer goes ahead, and the answer lands in the session it left", async (t) => {
    const agent = await wake(
        t,
        fakes({ realm: { ask: () => ({ type: "done" }) } }).calls,
        [
            toolCall("ask", { to: "hire#12", question: "Which file?" }),
            fauxAssistantMessage("Asked Ada."),
            fauxAssistantMessage("Thanks."),
        ],
    );
    await told(agent, "Ask Ada");
    const asking = await now(agent);

    assert.deepEqual(
        await agent.actions.rewind(agent.by(from.client), {
            id: "rw-1",
            chat: { with: 7 },
            item: await itemOfLine(agent, "Ask Ada"),
        }),
        { type: "rewound", text: "Ask Ada" },
    );
    await submitted(agent, {
        type: "answer",
        question: 3,
        from: ADA,
        text: "notes.md",
        conversation: String(asking),
    });
    await agent.idle();

    assert.deepEqual(await said(agent, await now(agent)), []);
    assert.equal((await said(agent, asking)).at(-1), "Thanks.");
});

test("a line steered into a running turn is charged to its own task", async (t) => {
    const workspace = slowRead();
    const { calls, sent } = fakes({ workspace });
    const agent = await wake(t, calls, [
        toolCall("read", { path: "/work/project/notes.md" }),
        fauxAssistantMessage("Both done."),
    ]);
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-a",
        task: "task-a",
        input: line("Read the notes"),
    });
    await workspace.reading;
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-b",
        task: "task-b",
        input: line("And add a title"),
    });
    workspace.answer();
    await agent.idle();

    const tasks = (name: string) =>
        named(sent, name).map(
            (call) => (call.message as { task: string }).task,
        );
    assert.deepEqual(tasks("may_spend"), ["task-a", "task-b"]);
    assert.deepEqual(tasks("charge"), ["task-a", "task-b"]);
});

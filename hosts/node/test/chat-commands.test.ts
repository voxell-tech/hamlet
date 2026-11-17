import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
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
import type { Input, ReadAnswer } from "../src/agent/messages.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    fakeCalls,
    from,
    named,
    toolCall,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

/** A model response held until the test answers it, or the run is aborted. */
function held(): {
    readonly step: FauxResponseStep;
    /** Resolves with what the request carried, once it is asked. */
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

/** Records what the request carried, then answers `text`. */
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

/** What a conversation shows a person, oldest first. */
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

/** The id a client reads for the chief's line `text` in the current session. */
async function itemOfLine(agent: Woken, text: string): Promise<string> {
    const entry = (await entriesOf(agent, await now(agent))).find(
        (each) => each.kind === "hamlet.line" && shown(each) === text,
    );
    assert.ok(entry !== undefined, `the line ${text} is shown`);
    return `${entry.id}:0`;
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

/** A workspace whose `read_file` answers only when the test says. */
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

const byText = (a: unknown, b: unknown) =>
    JSON.stringify(a).localeCompare(JSON.stringify(b));

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
    const session = await now(agent);
    const letters = await lettersOf(agent);

    assert.deepEqual(
        await agent.actions.stop(agent.by(from.client), { id: "stop-1" }),
        { turns: 2 },
    );
    assert.deepEqual(
        named(sent, "cancel")
            .map((call) => call.message)
            .sort(byText),
        [session, letters]
            .map((id) => ({
                which: { type: "conversation", conversation: String(id) },
            }))
            .sort(byText),
    );
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [{ conversation: String(session), why: "stopped" }],
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
    const session = await now(agent);

    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 1,
    });
    assert.deepEqual(await agent.actions.stop(client, { id: "stop-1" }), {
        turns: 0,
    });
    assert.equal(named(sent, "cancel").length, 1);
    // Told again, so the repeat of a stop cut off before the realm was told
    // tells it.
    assert.deepEqual(
        named(sent, "lapse").map((call) => call.message),
        [
            { conversation: String(session), why: "stopped" },
            { conversation: String(session), why: "stopped" },
        ],
    );
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

test("retry is refused after an answer", async (t) => {
    const slow = held();
    const agent = await wake(t, fakes().calls, [
        fauxAssistantMessage("One."),
        slow.step,
    ]);
    const client = agent.by(from.client);
    const retry = (id: string) => agent.actions.retry(client, { id, to: 7 });

    assert.equal((await retry("r-0")).type, "refused");
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

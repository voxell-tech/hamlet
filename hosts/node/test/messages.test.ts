import assert from "node:assert/strict";
import { test } from "node:test";
import { isDeepStrictEqual } from "node:util";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { ActorError } from "../src/actor/actor/mod.ts";
import {
    type ChatEvent,
    type Copies,
    readClient,
    type Submit,
} from "../src/agent/messages/agent.ts";
import { LIBRARY } from "../src/agent/messages/library.ts";
import { profileAt } from "../src/agent/messages/profile.ts";
import {
    type CredentialAnswer,
    type Kept,
    REALM,
    type SecretList,
    type TaskOpened,
} from "../src/agent/messages/realm.ts";
import { U32_MAX } from "../src/agent/messages/shared.ts";
import { WORKSPACE } from "../src/agent/messages/workspace.ts";
import {
    fakeCalls,
    from,
    named,
    priced,
    readPrompt,
    toolCall,
    toolResults,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import {
    ANSWERED_BY_TOOLS,
    actionOf,
    answer,
    answersTo,
    anthropic,
    CHIEF,
    COMMANDED,
    CREATED,
    chief,
    FAILED_WORDS,
    failed,
    field,
    fixtures,
    HAND,
    KEY,
    LAST_WORDS,
    listed,
    onAnthropic,
    ownForms,
    retry,
    SETTINGS,
    say,
    signedIn,
    submitted,
    TAKEN,
    TAKERS,
    threw,
    turn,
    uuid,
} from "./messages-testing.ts";
import { WRITTEN } from "./messages-written.ts";
import { fixture } from "./testing.ts";

for (const row of WRITTEN) {
    test(`the agent's ${row.fixture} equals its fixture`, async (t) => {
        const { calls, sent } = fakeCalls(row.answers ?? {});
        const agent =
            row.drive === undefined
                ? await turn(t, calls, {
                      submit: submitted(row.task, row.input),
                      responses:
                          row.tool === undefined ? [] : [toolCall(...row.tool)],
                      ...(row.models === undefined
                          ? {}
                          : { models: row.models }),
                      ...(row.offered === undefined
                          ? {}
                          : { offered: row.offered }),
                      ...(row.files === undefined ? {} : { files: row.files }),
                      ...(row.agent === undefined ? {} : { agent: row.agent }),
                  })
                : await row.drive(t, calls, sent);
        const { to, action } = actionOf(row.fixture);
        const of = () =>
            sent.filter((one) => one.to === to && one.name === action);
        // `turn_failed`, `read_copies` and a brief's end go out after the
        // work they follow has settled.
        await until(() => of().length > 0);
        assert.deepEqual(agent.reported, []);
        await agent.stop();

        const call = of().at(-1);
        assert.ok(call, `the agent sent no ${action}`);
        const expected = fixture(`${row.fixture}.args`) as unknown[];
        const args =
            call.message === undefined ? [] : [structuredClone(call.message)];
        for (const path of row.own ?? []) {
            const mine = field(args[0], path);
            const value = mine.holder[mine.key];
            assert.ok(
                ownForms(action)[mine.key]?.(value),
                `${path} has the agent's form: ${String(value)}`,
            );
            const theirs = field(expected[0], path);
            // A fixture that leaves the field out shows a call without it.
            if (theirs.key in theirs.holder) {
                mine.holder[mine.key] = theirs.holder[theirs.key];
            } else {
                delete mine.holder[mine.key];
            }
        }
        for (const [path, value] of Object.entries(row.constant ?? {})) {
            const mine = field(args[0], path);
            assert.equal(mine.holder[mine.key], value, path);
            const theirs = field(expected[0], path);
            mine.holder[mine.key] = theirs.holder[theirs.key];
        }
        assert.deepStrictEqual(args, expected);
    });
}

test("the agent's addresses of the workspace, the realm, the library and a profile equal their fixtures", () => {
    for (const [address, name] of [
        [WORKSPACE, "address/workspace"],
        [REALM, "address/realm"],
        [LIBRARY, "library/address"],
        [profileAt(2), "address/profile"],
    ] as const) {
        assert.deepStrictEqual(
            { kind: address.kind, key: address.key },
            fixture(`${name}.answer`),
        );
    }
});

test("a key signed in with settings reaches the model call with them after a restart, read from the realm", async (t) => {
    let kept: Kept | undefined;
    const { calls, sent } = fakeCalls({
        realm: {
            keep_credential: (message) => {
                kept = (message as { value: Kept }).value;
                return answer("realm/keep_credential");
            },
            credential: () =>
                kept ?? answer("realm/credential.refused.not_signed_in"),
        },
    });
    const keysSeen: (string | undefined)[] = [];
    const envsSeen: unknown[] = [];
    const first = await signedIn(
        t,
        calls,
        anthropic(keysSeen, { settings: SETTINGS, envsSeen }),
    );
    assert.deepStrictEqual(kept, answer("realm/credential.key.settings"));
    await first.stop();
    const asked = named(sent, "credential").length;

    const agent = await first.again();
    t.after(() => agent.stop());
    await agent.actions.copies(agent.by(from.realm), {
        model: { version: 1, value: { model: "anthropic::claude-opus" } },
    });
    await say(agent, "Go.");

    assert.ok(named(sent, "credential").length > asked, "the realm was asked");
    assert.deepEqual(keysSeen, [KEY]);
    assert.deepStrictEqual(envsSeen, [SETTINGS]);
});

test("realm/credential.refused.not_signed_in.answer fails the turn with its words, and no request is made", async (t) => {
    const given = answer("realm/credential.refused.not_signed_in") as Extract<
        CredentialAnswer,
        { type: "refused" }
    >;
    const { calls, sent } = fakeCalls({ realm: { credential: () => given } });
    const keysSeen: (string | undefined)[] = [];
    const agent = await onAnthropic(t, calls, keysSeen);
    await say(agent, "Go.");
    await until(() => named(sent, "turn_failed").length > 0);
    await agent.stop();

    assert.deepEqual(keysSeen, []);
    assert.deepEqual(
        named(sent, "turn_failed").map(
            (call) => (call.message as { words: string }).words,
        ),
        [given.words],
    );
});

test("realm/open_task.refused.you_left.answer refuses a retry in its words, and a reflection is asked on the standing task", async (t) => {
    const given = answer("realm/open_task.refused.you_left") as Extract<
        TaskOpened,
        { type: "refused" }
    >;
    const { calls, sent } = fakeCalls({
        realm: { open_task: () => given, lapse: () => answer("realm/lapse") },
    });
    const agent = await failed(t, calls, "task-2", FAILED_WORDS);
    await assert.rejects(
        retry(agent),
        (error: unknown) =>
            error instanceof ActorError &&
            error.code === "refused" &&
            error.public &&
            error.message === given.words,
    );
    assert.equal(await agent.actions.reflect(agent.by(from.client)), "asked");
    await agent.idle();
    const reported = [...agent.reported];
    await agent.stop();

    assert.deepEqual(reported, [
        `a reflection's task was refused: ${given.words}`,
    ]);
    const asked = named(sent, "may_spend").map(
        (call) => (call.message as { task: string }).task,
    );
    assert.deepEqual(asked, ["task-2", HAND.task]);
});

test("realm/read_copies.answer is read by the wake, and its charter reaches the prompt", async (t) => {
    const given = answer("realm/read_copies") as Copies;
    const { calls } = fakeCalls({ realm: { read_copies: () => given } });
    const agent = await wakeAgent(t, { calls, input: HAND });
    const prompt = async () => readPrompt(agent);
    await until(async () =>
        (await prompt()).includes(given.charter?.value ?? "no charter"),
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("realm/secrets.answer's values are taken out of what a client reads", async (t) => {
    const given = answer("realm/secrets") as SecretList;
    const [value] = given.values;
    assert.ok(value !== undefined);
    const { calls } = fakeCalls({ realm: { secrets: () => given } });
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        responses: [fauxAssistantMessage(`It is ${value}.`)],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "line-secret",
        input: { type: "line", text: "What is it?" },
    });
    await agent.idle();
    const read = async () =>
        JSON.stringify(
            await agent.actions.read_chat(agent.by(from.client), {
                id: "00000000-0000-4000-8000-0000000000cc",
                chat: { with: 7 },
            }),
        );
    await until(async () => (await read()).includes("It is [REDACTED]."));
    assert.ok(!(await read()).includes(value));
    await agent.stop();
});

test("a client's call is refused in its fixture's words", async (t) => {
    const agent = await chief(t, fakeCalls(COMMANDED).calls, ["One."]);
    await say(agent, "Go.");
    const by = agent.by(from.client);
    const answers = {
        "agent/retry.refused.answered": await retry(agent),
        "agent/resume.refused.no_such_session": await agent.actions.resume(by, {
            id: uuid(3),
            number: 9,
            to: 7,
        }),
        "agent/rewind.refused.not_current": await agent.actions.rewind(by, {
            id: uuid(4),
            chat: { with: 7, past: 1 },
            item: "i9",
        }),
        "agent/rewind.refused.no_such_item": await agent.actions.rewind(by, {
            id: uuid(5),
            chat: CHIEF,
            item: "i9",
        }),
    };
    await agent.stop();

    assert.deepStrictEqual(
        answers,
        Object.fromEntries(
            Object.keys(answers).map((name) => [name, answer(name)]),
        ),
    );
});

function formOf(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(formOf);
    if (value !== null && typeof value === "object") {
        return Object.fromEntries(
            Object.entries(value).map(([key, inner]) => [key, formOf(inner)]),
        );
    }
    return typeof value;
}

test("the agent's chat events equal their fixtures, and its items, status, usage and reset have their fixture's form", async (t) => {
    const { calls } = fakeCalls(COMMANDED);
    const agent = await wakeAgent(t, {
        calls,
        key: "12",
        input: HAND,
        responses: ["Hello back.", "One.", "Kept.", "Two.", "Three."].map(
            (text) => fauxAssistantMessage(text),
        ),
    });
    const send = async (id: string, input: Submit["input"]) => {
        await agent.actions.submit(agent.by(from.realm), { id, input });
        await agent.idle();
    };
    const parcel = (file: number, size: number) => ({
        parcel: 1,
        file,
        path: `kept-${file}`,
        size,
    });
    await send("input-1", { type: "letter", from: { agent: 3 }, text: "Hi." });
    await send("input-2", { type: "line", text: "Go." });
    await agent.actions.new_session(agent.by(from.client), {
        id: uuid(2),
        to: 12,
    });
    await send("input-3", {
        type: "line",
        text: "Again.",
        chief: "ada",
        images: [
            { image: { id: "ab".repeat(32), at: 6 }, file: parcel(0, 64) },
            { image: { id: "cd".repeat(32) }, file: parcel(1, 64) },
        ],
        files: [
            {
                attached: {
                    id: "12".repeat(32),
                    at: 0,
                    name: "notes/plan.md",
                    size: 2048,
                },
                file: parcel(2, 2048),
            },
            {
                attached: { id: "34".repeat(32), name: "log.txt", size: 0 },
                file: parcel(3, 0),
            },
        ],
    });
    await send("input-5", {
        type: "worker_answer",
        from: { agent: 9, name: "worker#9" },
        text: "Found.",
    });
    const harness = await agent.harness();
    // A harness reopened after a failed commit resets every chat.
    const lost = agent.failNextCommit();
    await agent.actions
        .submit(agent.by(from.realm), {
            id: "input-4",
            input: { type: "line", text: "Lost." },
        })
        .catch(() => undefined);
    await lost;
    await until(async () => (await agent.harness()) !== harness);
    const events = () =>
        agent.events
            .filter(([name]) => name === "chat")
            .map(([, event]) => event as ChatEvent);
    const resets = ["letters", "past"].map(
        (name) => fixture(`agent/chat.event.${name}.answer`) as ChatEvent,
    );
    const resetOf = (reset: ChatEvent) =>
        events().find(
            (event) =>
                isDeepStrictEqual(event.chat, reset.chat) &&
                isDeepStrictEqual(event.ops, reset.ops),
        );
    await until(() => resets.every((reset) => resetOf(reset) !== undefined));
    await agent.stop();

    for (const reset of resets) {
        const event = resetOf(reset);
        assert.ok(event);
        for (const counted of [event.epoch, event.number]) {
            assert.ok(Number.isInteger(counted) && counted > 0, `${counted}`);
        }
        assert.deepStrictEqual(
            { ...event, epoch: reset.epoch, number: reset.number },
            reset,
        );
    }
    const whole = fixture("agent/chat.event.answer") as ChatEvent;
    const ops = events()
        .filter((event) => isDeepStrictEqual(event.chat, whole.chat))
        .flatMap((event) => event.ops);
    // An append, and an answer that replaces the one being written, need a
    // stream held mid-answer, as `chat-transcript.test.ts` holds one.
    for (const op of whole.ops.filter(
        (one) => one.type === "add" && one.replaces === undefined,
    )) {
        assert.ok(
            ops.some((one) => isDeepStrictEqual(formOf(one), formOf(op))),
            `the agent added no item of the form of ${JSON.stringify(op)}`,
        );
    }
    for (const op of whole.ops.filter(
        (one) => one.type !== "add" && one.type !== "append",
    )) {
        const mine = ops.find((one) => one.type === op.type);
        assert.ok(mine, `the agent sent no ${op.type}`);
        assert.deepStrictEqual(formOf(mine), formOf(op), op.type);
    }
});

for (const name of fixtures(/^error\/.*\.run\./)) {
    test(`${name}, thrown by run, gives bash the interrupted result, never a throw`, async (t) => {
        const given = fixture(name) as {
            group: string;
            code: string;
            message: string;
            metadata: unknown;
        };
        const { calls } = fakeCalls({
            workspace: {
                run: () => {
                    throw new ActorError(
                        given.group,
                        given.code,
                        given.message,
                        {
                            metadata: given.metadata,
                        },
                    );
                },
            },
        });
        const agent = await turn(t, calls, {
            submit: submitted("task-1"),
            responses: [toolCall("bash", { command: "echo hi" })],
        });
        const [result] = await toolResults(agent);
        await agent.stop();

        assert.ok(result && !threw(result));
        // The words for a change that may have run.
        assert.ok(
            result.diagnostics.some(({ message }) =>
                message.includes(
                    "Tool bash was interrupted by a restart and may have partially run",
                ),
            ),
            JSON.stringify(result.diagnostics),
        );
    });
}

const CLIENT_CALLS = new RegExp(
    `^agent/(${[...Object.keys(readClient), "copies"].join("|")})(\\..+)?\\.(args|answer)$`,
);

for (const name of answersTo("workspace/list_tools")) {
    test(`${name} offers the tool it lists, however commands are confined`, async (t) => {
        const { calls } = fakeCalls({
            workspace: { list_tools: () => fixture(name) as never },
        });
        const agent = await turn(t, calls, {
            submit: submitted(undefined),
            offered: "word_count",
        });
        await agent.stop();
    });
}

test("every fixture is one the agent writes or takes, or is listed with its reason, never both", () => {
    const covered = new Set([
        ...WRITTEN.map((row) => `${row.fixture}.args`),
        "address/workspace.answer",
        "address/realm.answer",
        "address/profile.answer",
        "library/address.answer",
        ...TAKEN.map((call) => `${call}.args`),
        ...Object.values(TAKERS).map((taker) => taker.answer),
        // `test/branch.test.ts` compares it.
        "profile/remove_path.later.answer",
        // `test/inbox.test.ts` compares them.
        "agent/read_inbox.args",
        "agent/read_inbox.after.args",
        "agent/read_inbox.answer",
        "agent/read_inbox.last.answer",
        // `test/tasks.test.ts` compares them.
        "agent/on_task.args",
        "agent/on_task.answer",
        ...CREATED.map((input) => `${input}.input`),
        ...ANSWERED_BY_TOOLS.flatMap(answersTo).filter((name) => !listed(name)),
        ...answersTo("realm/may_spend"),
        ...answersTo("realm/reply"),
        ...answersTo("realm/read_place"),
        ...answersTo("realm/fetch_part"),
        ...[
            "realm/credential",
            "realm/keep_credential",
            "realm/lapse",
            "realm/open_task",
            "realm/read_copies",
            "realm/read_own_place",
            "realm/secrets",
            "realm/turn_failed",
            "realm/working",
            "realm/worker_done",
            "realm/errand_done",
            "workspace/cancel",
            "library/shelves",
            "library/offer",
            "profile/files",
            "profile/save",
            "profile/learned",
            "workspace/fit_image",
        ].flatMap(answersTo),
        "agent/chat.event.answer",
        "agent/chat.event.letters.answer",
        "agent/chat.event.past.answer",
        ...fixtures(CLIENT_CALLS).filter((name) => !listed(name)),
        "realm/charge.answer",
        ...answersTo("workspace/list_tools"),
        ...fixtures(/^error\/.*\.run\./),
        // `test/link/errors.test.ts` reads it across the link.
        "error/wrong_caller.tell.agent.answer",
    ]);
    const all = fixtures(/./);
    assert.deepEqual(
        all.filter((name) => !covered.has(name) && !listed(name)),
        [],
        "neither covered nor listed",
    );
    assert.deepEqual(
        all.filter((name) => covered.has(name) && listed(name)),
        [],
        "both covered and listed",
    );
});

test("a charge's micros, took_ms and tokens are held at 2^32 - 1", async (t) => {
    // Five thousand dollars is five billion micros.
    const { calls, sent } = fakeCalls({});
    const agent = await turn(t, calls, {
        submit: submitted(undefined),
        models: (models) =>
            priced(models, 5_000, { input: U32_MAX, output: 1 }),
    });
    await agent.stop();

    const charges = sent
        .filter((call) => call.name === "charge")
        .map(
            (call) =>
                call.message as {
                    micros: number;
                    took_ms: number;
                    tokens: number;
                },
        );
    assert.equal(charges.length, 1);
    for (const { micros, took_ms, tokens } of charges) {
        assert.equal(micros, U32_MAX);
        assert.equal(tokens, U32_MAX);
        assert.ok(Number.isInteger(took_ms) && took_ms <= U32_MAX);
    }
});

test("a reply's question is never above 2^32 - 1", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { reply: () => answer("realm/reply.done") },
    });
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        responses: [fauxAssistantMessage(LAST_WORDS)],
    });
    try {
        await agent.actions.submit(
            agent.by(from.realm),
            submitted(undefined, {
                type: "question",
                letter: 2 ** 32,
                from: { agent: 12 },
                text: "How many fail?",
            }),
        );
        await agent.idle();
    } catch (error) {
        // Refused before anything is sent.
        assert.ok(error instanceof ActorError);
    } finally {
        await agent.stop();
    }

    for (const call of named(sent, "reply")) {
        const { question } = call.message as { question: number };
        assert.ok(question <= U32_MAX, `question ${question}`);
    }
});

test("an agent's number in Named is never above 2^32 - 1", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { tell: () => answer("realm/tell.sent") },
    });
    const agent = await turn(t, calls, {
        submit: submitted(undefined),
        responses: [toolCall("tell", { to: `hire#${2 ** 32}`, text: "Hello" })],
    });
    await agent.stop();

    for (const call of named(sent, "tell")) {
        const { act } = call.message as { act: { to: { agent?: number } } };
        assert.ok((act.to.agent ?? 0) <= U32_MAX, `agent ${act.to.agent}`);
    }
});

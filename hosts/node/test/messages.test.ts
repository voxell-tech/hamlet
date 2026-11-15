import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { type TestContext, test } from "node:test";
import {
    type FauxResponseStep,
    fauxAssistantMessage,
    type Models,
} from "@earendil-works/pi-ai";
import { ActorError, type Calls } from "../src/actor/actor/mod.ts";
import { decodeCborCompat, decodeCborJsonCompat } from "../src/actor/serde.ts";
import {
    type AgentInput,
    REALM,
    type Submit,
    U32_MAX,
    WORKSPACE,
} from "../src/agent/messages.ts";
import {
    fakeCalls,
    from,
    named,
    priced,
    toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

const FOLDER = new URL(
    "../../../crates/hamlet_messages/fixtures/",
    import.meta.url,
);

/**
 * A fixture as the host decodes it: an answer as `Calls.call` does, and a
 * call's arguments or a create input as the host does for an action.
 */
function fixture(name: string): unknown {
    const bytes = readFileSync(new URL(`${name}.cbor`, FOLDER));
    return name.endsWith(".answer")
        ? decodeCborJsonCompat(bytes)
        : decodeCborCompat(bytes);
}

function answer(name: string): never {
    return fixture(`${name}.answer`) as never;
}

function fixtures(pattern: RegExp): string[] {
    return readdirSync(FOLDER)
        .flatMap((folder) =>
            readdirSync(new URL(`${folder}/`, FOLDER)).map(
                (file) => `${folder}/${file.replace(/\.cbor$/, "")}`,
            ),
        )
        .filter((name) => pattern.test(name))
        .sort();
}

type Answers = Parameters<typeof fakeCalls>[0];

const HAND = fixture("agent/hand.input") as AgentInput;

const LAST_WORDS = "Two of them";

/** What a field read from the wrong place looks like in the words the model is given. */
const NOT_WORDS = /undefined|NaN|\[object /;

function submitted(
    task: string | undefined,
    input: Submit["input"] = { type: "line", text: "Go." },
): Submit {
    return { id: "input-1", ...(task === undefined ? {} : { task }), input };
}

async function turn(
    t: TestContext,
    calls: Calls,
    given: {
        submit: Submit;
        responses?: FauxResponseStep[];
        models?: (models: Models) => Models;
        offered?: string;
    },
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        responses: [
            ...(given.responses ?? []),
            fauxAssistantMessage(LAST_WORDS),
        ],
        ...(given.models === undefined ? {} : { models: given.models }),
    });
    if (given.offered !== undefined) {
        const root = await (await agent.harness()).root(context);
        await until(async () =>
            (await root.agent(context)).tools.some(
                (tool) => tool.name === given.offered,
            ),
        );
    }
    await agent.actions.submit(agent.by(from.realm), given.submit);
    await agent.idle();
    return agent;
}

type ToolArgs = Parameters<typeof toolCall>[1];

interface Written {
    readonly fixture: string;
    readonly task?: string;
    readonly input?: Submit["input"];
    readonly tool?: readonly [name: string, args: ToolArgs];
    readonly answers?: Answers;
    readonly offered?: string;
    readonly models?: (models: Models) => Models;
    /**
     * Fields of the first argument the agent makes itself, which the test
     * cannot steer. Each is checked for the agent's form and then taken from
     * the fixture.
     */
    readonly own?: readonly string[];
    /** Fields the agent always writes the same, checked and then taken from the fixture. */
    readonly constant?: Readonly<Record<string, unknown>>;
}

function ownForms(
    action: string,
): Readonly<Record<string, (value: unknown) => boolean>> {
    const call =
        action === "charge"
            ? /^m[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/
            : /^t[1-9][0-9]*$/;
    return {
        call: (value) => typeof value === "string" && call.test(value),
        conversation: (value) =>
            typeof value === "string" && /^[1-9][0-9]*$/.test(value),
        took_ms: (value) =>
            Number.isInteger(value) &&
            (value as number) >= 0 &&
            (value as number) <= U32_MAX,
    };
}

const WRITTEN: readonly Written[] = [
    {
        fixture: "workspace/run",
        task: "task-1",
        tool: ["bash", { command: "echo hi", timeout: 30, network: false }],
        answers: { workspace: { run: () => answer("workspace/run.running") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/run.unset",
        tool: ["bash", { command: "echo hi", network: true }],
        answers: { workspace: { run: () => answer("workspace/run.running") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/run.tool",
        task: "task-1",
        tool: ["word_count", { path: "/project/a.md" }],
        answers: {
            workspace: {
                list_tools: () => answer("workspace/list_tools"),
                run: () => answer("workspace/run.running"),
            },
        },
        offered: "word_count",
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/write_file",
        tool: ["write", { path: "/work/project/notes.md", content: "hello\n" }],
        answers: {
            workspace: {
                write_file: () => answer("workspace/write_file.ended.changed"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/edit_file",
        task: "task-1",
        tool: [
            "edit",
            {
                path: "/work/project/notes.md",
                edits: [{ oldText: "hello", newText: "goodbye" }],
            },
        ],
        answers: {
            workspace: {
                edit_file: () => answer("workspace/edit_file.ended.changed"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "workspace/read_file",
        tool: [
            "read",
            { path: "/work/project/notes.md", offset: 10, limit: 20 },
        ],
        answers: {
            workspace: { read_file: () => answer("workspace/read_file.text") },
        },
    },
    {
        fixture: "workspace/read_file.whole",
        tool: ["read", { path: "/work/project/notes.md" }],
        answers: {
            workspace: { read_file: () => answer("workspace/read_file.text") },
        },
    },
    {
        fixture: "workspace/search",
        tool: [
            "search",
            {
                pattern: "fn main",
                path: "/work/project",
                name: "*.rs",
                ignore_case: true,
            },
        ],
        answers: {
            workspace: { search: () => answer("workspace/search.text") },
        },
    },
    {
        fixture: "workspace/find",
        tool: ["find", { path: "/work/project" }],
        answers: {
            workspace: { find: () => answer("workspace/search.text") },
        },
    },
    // Sent at each wake.
    { fixture: "workspace/list_tools" },
    // Sent before each model request; a turn whose input names no task asks for the standing one.
    { fixture: "realm/may_spend" },
    {
        fixture: "realm/charge",
        models: (models) => priced(models, 0.00125),
        own: ["call", "took_ms"],
    },
    {
        fixture: "realm/reply",
        task: "task-1",
        input: {
            type: "question",
            letter: 5,
            from: { agent: 12, name: "Ada" },
            text: "How many fail?",
        },
        answers: { realm: { reply: () => answer("realm/reply.done") } },
    },
    {
        fixture: "realm/tell",
        task: "task-1",
        tool: ["tell", { to: "mayor", text: "The tests pass" }],
        answers: { realm: { tell: () => answer("realm/tell.done") } },
        own: ["call"],
    },
    {
        fixture: "realm/tell.no_task",
        tool: ["tell", { to: "hand", text: "The tests pass" }],
        answers: { realm: { tell: () => answer("realm/tell.done") } },
        own: ["call"],
    },
    {
        fixture: "realm/ask",
        task: "task-1",
        tool: ["ask", { to: "hire#12", question: "Which tests fail?" }],
        answers: { realm: { ask: () => answer("realm/ask.done") } },
        own: ["call", "act.conversation"],
    },
    {
        fixture: "realm/request_road",
        task: "task-1",
        tool: [
            "request_road",
            { to: "hire#13", reason: "To ask about the tests" },
        ],
        answers: {
            realm: { request_road: () => answer("realm/request_road.done") },
        },
        own: ["call"],
    },
    {
        fixture: "realm/request_road.no_why",
        tool: ["request_road", { to: "mayor" }],
        answers: {
            realm: { request_road: () => answer("realm/request_road.done") },
        },
        own: ["call"],
    },
    {
        fixture: "realm/found",
        tool: ["found_hamlet", { name: "Harbor", purpose: "Run the release" }],
        answers: { realm: { found: () => answer("realm/found.founded") } },
        own: ["call"],
        // The tool has no tier to choose: a hamlet an agent founds has
        // today's default, 50. The fixture's 500 is a chief's choice.
        constant: { "act.tier": "fifty" },
    },
    {
        fixture: "realm/hire",
        tool: ["hire", { purpose: "Check the links", under: "hire#12" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call"],
    },
    {
        fixture: "realm/end",
        tool: ["end_hire", { hire: "hire#12" }],
        answers: { realm: { end: () => answer("realm/end.done") } },
        own: ["call"],
    },
    {
        fixture: "realm/seat",
        tool: ["seat_mayor", { hire: "hire#13" }],
        answers: { realm: { seat: () => answer("realm/seat.done") } },
        own: ["call"],
    },
    {
        fixture: "realm/lay_road",
        tool: ["lay_road", { a: "hire#12", b: "hire#13" }],
        answers: { realm: { lay_road: () => answer("realm/lay_road.done") } },
        own: ["call"],
    },
    {
        fixture: "realm/take_up_road",
        tool: ["take_up_road", { a: "hire#12", b: "mayor" }],
        answers: {
            realm: { take_up_road: () => answer("realm/take_up_road.done") },
        },
        own: ["call"],
    },
    {
        fixture: "realm/set_tools",
        tool: ["set_tools", { hire: "hire#12", off: ["tell"] }],
        answers: {
            realm: { set_tools: () => answer("realm/set_tools.done") },
        },
        own: ["call"],
    },
];

function field(
    message: unknown,
    path: string,
): { holder: Record<string, unknown>; key: string } {
    const steps = path.split(".");
    const key = steps.pop() as string;
    let holder = message as Record<string, unknown>;
    for (const step of steps) holder = holder[step] as Record<string, unknown>;
    return { holder, key };
}

function actionOf(name: string): { to: string; action: string } {
    const [folder = "", rest = ""] = name.split("/");
    const to = folder === "workspace" ? WORKSPACE : REALM;
    return { to: `${to.kind}/${to.key}`, action: rest.split(".")[0] ?? "" };
}

for (const row of WRITTEN) {
    test(`the agent's ${row.fixture} equals its fixture`, async (t) => {
        const { calls, sent } = fakeCalls(row.answers ?? {});
        const agent = await turn(t, calls, {
            submit: submitted(row.task, row.input),
            responses: row.tool === undefined ? [] : [toolCall(...row.tool)],
            ...(row.models === undefined ? {} : { models: row.models }),
            ...(row.offered === undefined ? {} : { offered: row.offered }),
        });
        await agent.stop();

        const { to, action } = actionOf(row.fixture);
        const call = sent.find((one) => one.to === to && one.name === action);
        assert.ok(call, `the agent sent no ${action}`);
        const expected = fixture(`${row.fixture}.args`) as unknown[];
        // Each action takes one message, but `list_tools` takes none.
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
            mine.holder[mine.key] = theirs.holder[theirs.key];
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

test("the agent's addresses of the workspace and the realm equal their fixtures", () => {
    for (const [address, name] of [
        [WORKSPACE, "address/workspace"],
        [REALM, "address/realm"],
    ] as const) {
        assert.deepStrictEqual(
            { kind: address.kind, key: address.key },
            fixture(`${name}.answer`),
        );
    }
});

const SENDER = {
    submit: from.realm,
    finished: from.workspace,
    place: from.realm,
    fill: from.realm,
} as const;

/** The calls of the actions this agent takes. */
const TAKEN = [
    "agent/fill",
    "agent/finished.change.changed",
    "agent/finished.change.failed",
    "agent/finished.change.not_run",
    "agent/finished.run.not_run",
    "agent/finished.run.ran",
    "agent/place",
    "agent/place.hand",
    "agent/submit.answer",
    "agent/submit.letter",
    "agent/submit.line",
    "agent/submit.line.attached",
    "agent/submit.question",
] as const;

/** The inputs this agent is created with. */
const CREATED = ["agent/hand", "agent/hire"] as const;

for (const call of TAKEN) {
    const name = `${call}.args`;
    test(`${name} is read and taken, and answered as its answer fixture holds`, async (t) => {
        const action = actionOf(name).action as keyof typeof SENDER;
        const { calls } = fakeCalls({
            realm: { reply: () => answer("realm/reply.done") },
        });
        const agent = await wakeAgent(t, {
            calls,
            input: HAND,
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        const [message, ...more] = fixture(name) as unknown[];
        assert.equal(more.length, 0);
        const take = agent.actions[action] as (
            c: ReturnType<Woken["by"]>,
            message: unknown,
        ) => Promise<unknown>;
        assert.deepStrictEqual(
            await take(agent.by(SENDER[action]), message),
            fixture(`agent/${action}.answer`),
        );
        await agent.idle();
        assert.deepEqual(agent.reported, []);
        const entries = await agent.entries();
        await agent.stop();

        if (action === "finished") {
            const ends = entries.filter(
                (entry) => entry.kind === "hamlet.finished",
            );
            assert.equal(ends.length, 1, name);
            assert.notEqual(ends[0]?.text, "", name);
        }
        for (const entry of entries) {
            assert.doesNotMatch(
                entry.text,
                NOT_WORDS,
                `${name}: ${entry.kind}`,
            );
        }
    });
}

for (const input of CREATED) {
    const name = `${input}.input`;
    test(`${name} is read and creates the agent`, async (t) => {
        const { calls } = fakeCalls({});
        const agent = await wakeAgent(t, {
            calls,
            input: fixture(name) as AgentInput,
        });
        assert.deepEqual(agent.reported, []);
        await agent.stop();
    });
}

test("agent/place.args keeps every tool's name and the folder, and turns off only the society tools named", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls, input: HAND });
    const [place] = fixture("agent/place.args") as unknown[];
    await agent.actions.place(agent.by(from.realm), place as never);
    const row = await agent.c.db.get<{ tools_off: string; place: string }>(
        "SELECT tools_off, place FROM agent",
    );
    const root = await (await agent.harness()).root(context);
    const offered = async () =>
        (await root.agent(context)).tools.map((tool) => tool.name);
    await until(async () => (await offered()).includes("bash"));
    const names = await offered();
    await agent.stop();

    assert.deepEqual(JSON.parse(row?.tools_off ?? ""), [
        { name: "found" },
        { name: "bash" },
    ]);
    assert.equal(
        (JSON.parse(row?.place ?? "") as { folder?: unknown }).folder,
        "/work/orchard",
    );
    assert.ok(!names.includes("found_hamlet"), names.join());
    assert.ok(names.includes("hire") && names.includes("bash"), names.join());
});

const ANSWERED_BY_TOOLS = [
    "workspace/run",
    "workspace/read_file",
    "workspace/search",
    "workspace/find",
    "workspace/write_file",
    "workspace/edit_file",
    "realm/tell",
    "realm/ask",
    "realm/request_road",
    "realm/found",
    "realm/hire",
    "realm/end",
    "realm/seat",
    "realm/lay_road",
    "realm/take_up_road",
    "realm/set_tools",
] as const;

function answersTo(action: string): string[] {
    return fixtures(new RegExp(`^${action}(\\..+)?\\.answer$`));
}

async function toolResults(agent: Woken) {
    return (await agent.entries()).filter(
        (entry) => entry.kind === "pi.tool-result",
    );
}

/**
 * pi-durable records a throw of a tool's `execute` as a result with no
 * output and a `tool_error` diagnostic.
 */
function threw(data: unknown): boolean {
    return JSON.stringify(data).includes('"tool_error"');
}

for (const action of ANSWERED_BY_TOOLS) {
    test(`each answer to ${action} gives its tool a result, never a throw`, async (t) => {
        const row = WRITTEN.find((written) => written.fixture === action);
        const tool = row?.tool;
        assert.ok(tool);
        const answers = answersTo(action);
        const queue = [...answers];
        const [folder = "", name = ""] = action.split("/");
        const { calls } = fakeCalls({
            [folder]: { [name]: () => fixture(queue.shift() as string) },
        });
        const agent = await turn(t, calls, {
            submit: submitted(row?.task),
            responses: answers.map((_, index) =>
                toolCall(tool[0], tool[1], `c${index}`),
            ),
        });
        const results = await toolResults(agent);
        await agent.stop();

        assert.equal(results.length, answers.length);
        answers.forEach((name, index) => {
            const result = results[index];
            assert.ok(result && !threw(result.data), `${name} threw`);
            const given = fixture(name) as { type: string; words?: string };
            if (given.type === "refused") {
                assert.equal(result.text, given.words, name);
            } else {
                assert.notEqual(result.text, "", name);
                assert.doesNotMatch(result.text, NOT_WORDS, name);
            }
        });
    });
}

for (const name of answersTo("realm/may_spend")) {
    test(`${name} is read by the model gate, and the charge's answer by its sender`, async (t) => {
        const given = fixture(name) as { type: string; words?: string };
        const { calls } = fakeCalls({
            realm: {
                may_spend: () => given as never,
                charge: () => answer("realm/charge"),
            },
        });
        const agent = await turn(t, calls, { submit: submitted(undefined) });
        const harness = await agent.harness();
        const root = await harness.root(context);
        const submission = await harness.commit(
            (tx) => tx.submissionByRequest(root.id, "input-1"),
            context,
        );
        await agent.stop();

        if (given.type === "refused") {
            assert.equal(agent.faux.state.callCount, 0);
            const detail = String(submission?.detail);
            assert.ok(detail.endsWith(`: ${given.words}`), detail);
        } else {
            assert.equal(agent.faux.state.callCount, 1);
        }
        assert.deepEqual(agent.reported, []);
    });
}

for (const name of answersTo("realm/reply")) {
    test(`${name} is read by the reply`, async (t) => {
        const given = fixture(name) as { type: string; words?: string };
        const { calls } = fakeCalls({ realm: { reply: () => given as never } });
        const row = WRITTEN.find(
            (written) => written.fixture === "realm/reply",
        );
        const agent = await turn(t, calls, {
            submit: submitted(row?.task, row?.input),
        });
        await agent.stop();

        assert.deepEqual(
            agent.reported,
            given.type === "refused"
                ? [`a reply was refused: ${given.words}`]
                : [],
        );
    });
}

for (const name of fixtures(/^error\/.*\.run\./)) {
    test(`${name}, thrown by run, gives bash today's interrupted result, never a throw`, async (t) => {
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

        assert.ok(result && !threw(result.data));
        // Today's words for a change that may have run (`calls.rs`).
        assert.ok(
            result.text.includes(
                "Tool bash was interrupted by a restart and may have partially run",
            ),
            result.text,
        );
    });
}

const NOT_THE_AGENTS: readonly (readonly [RegExp, string])[] = [
    [/^realm\/chief_/, "a chief's act, sent by a client"],
    [
        /^realm\/(say|answer|read_world|read_charter)\./,
        "a client's call of the realm",
    ],
    [
        /^realm\/(credential|keep_credential|lapse|open_task|read_copies|turn_failed|worker_done|errand_done)\./,
        "an agent's call of the realm, which this agent does not make",
    ],
    [
        /^realm\/(tell|ask)\.paths\.args$/,
        "an act that sends paths, which this agent does not write",
    ],
    [
        /^realm\/(seat|lay_road|take_up_road|set_tools)\.why\.args$/,
        "an act with its why, which this agent does not write",
    ],
    [
        /^realm\/charge\.tokens\.args$/,
        "a charge with its tokens, which this agent does not write",
    ],
    [
        /^realm\/society_tools\.answer$/,
        "the society tools as a refusal's gate names them: the agent reads only a refusal's words",
    ],
    [/^realm\/(needs_chief|secrets)\./, "the workspace's call of the realm"],
    [
        /^workspace\/(set_reach|cancel_agent|chief_answer)\./,
        "the realm's call of the workspace",
    ],
    [
        /^workspace\/cancel\./,
        "an agent's call, which this agent never makes: it has no `/stop`",
    ],
    [
        /^workspace\/command_status\./,
        "an agent's call, which this agent never makes: a command's end arrives as `finished`",
    ],
    [
        /^error\/wrong_caller\.set_reach\./,
        "the answer to the realm's set_reach",
    ],
    [/^address\/agent\./, "the agent's own address, which the host gives it"],
    [/^address\/profile\./, "a profile's address, which the agent never calls"],
    [
        /^agent\/(read_chat|read_history|read_prompt|login|new_session|resume|retry|rewind|stop|reflect|copies)\./,
        "an action this agent does not take",
    ],
    [/^agent\/chat\.event\./, "a chat's event, which this agent does not send"],
    [
        /^agent\/submit\.(notice|worker_answer)\.args$/,
        "an input this agent does not take: its submit is refused as unreadable",
    ],
    [
        /^agent\/submit\.line\.reviewed\.args$/,
        "a line a lead reviewed: the agent takes the line and drops the note",
    ],
    [
        /^agent\/(errand|worker)\.input$/,
        "the input of an errand or a worker, which this agent is never created with",
    ],
    [
        /^library\/(shelves|offer)\./,
        "an agent's call of the library, which this agent does not make",
    ],
    [
        /^library\/(read_shelves|verdict|unshelve)\./,
        "the realm's call of the library",
    ],
    [
        /^library\/(shelves_changed|drop_copy)\./,
        "the library's call of an agent, an action this agent does not take",
    ],
    [
        /^library\/address\./,
        "the library's address, which the agent never calls",
    ],
    [
        /^profile\/(files|save)\./,
        "an agent's call of its profile, which this agent does not make",
    ],
    [
        /^profile\/(moved|remove_path)\./,
        "a profile's call of an agent, an action this agent does not take",
    ],
    [
        /^profile\/(branch|tag|pin|revert|purge|edit|prepare_dream|end_dream|versions|papers|erase)\./,
        "the realm's call of a profile",
    ],
    [
        /^profile\/(character|fork)\.input$/,
        "the input a profile is created with",
    ],
    [
        /^stored\//,
        "a row the workspace, a profile, the library or the host keeps in its own file",
    ],
];

test("every fixture is one the agent writes or takes, or is listed with its reason", () => {
    const covered = new Set([
        ...WRITTEN.map((row) => `${row.fixture}.args`),
        "address/workspace.answer",
        "address/realm.answer",
        ...TAKEN.map((call) => `${call}.args`),
        ...Object.keys(SENDER).map((action) => `agent/${action}.answer`),
        ...CREATED.map((input) => `${input}.input`),
        ...ANSWERED_BY_TOOLS.flatMap(answersTo),
        ...answersTo("realm/may_spend"),
        ...answersTo("realm/reply"),
        "realm/charge.answer",
        // `workspace/run.tool` waits until the tool it lists is offered.
        "workspace/list_tools.answer",
        ...fixtures(/^error\/.*\.run\./),
    ]);
    const unaccounted = fixtures(/./).filter(
        (name) =>
            !covered.has(name) &&
            !NOT_THE_AGENTS.some(([pattern]) => pattern.test(name)),
    );
    assert.deepEqual(unaccounted, []);
});

test("a charge's micros and took_ms are held at 2^32 - 1", async (t) => {
    // Five thousand dollars is five billion micros.
    const { calls, sent } = fakeCalls({});
    const agent = await turn(t, calls, {
        submit: submitted(undefined),
        models: (models) => priced(models, 5_000),
    });
    await agent.stop();

    const charges = sent
        .filter((call) => call.name === "charge")
        .map((call) => call.message as { micros: number; took_ms: number });
    assert.equal(charges.length, 1);
    for (const { micros, took_ms } of charges) {
        assert.equal(micros, U32_MAX);
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
        realm: { tell: () => answer("realm/tell.done") },
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

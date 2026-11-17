import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { type TestContext, test } from "node:test";
import { isDeepStrictEqual } from "node:util";
import {
    type AssistantMessage,
    createFauxCore,
    createProvider,
    type FauxResponseFactory,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Models,
    type MutableModels,
} from "@earendil-works/pi-ai";
import { ActorError, type Calls } from "../src/actor/actor/mod.ts";
import { decodeCborCompat, decodeCborJsonCompat } from "../src/actor/serde.ts";
import { chatActions } from "../src/agent/chat.ts";
import { signIns } from "../src/agent/login.ts";
import {
    type AgentInput,
    type ChatEvent,
    type ChatRef,
    type Copies,
    type CredentialAnswer,
    type Done,
    type OauthLogin,
    REALM,
    readClient,
    type Submit,
    type TaskOpened,
    U32_MAX,
    WORKSPACE,
} from "../src/agent/messages.ts";
import {
    fakeCalls,
    from,
    named,
    priced,
    type Sent,
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
    readonly drive?: (
        t: TestContext,
        calls: Calls,
        sent: readonly Sent[],
    ) => Promise<Woken>;
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

const CHIEF: ChatRef = { with: 7 };

const COMMANDED: Answers = {
    realm: {
        lapse: () => answer("realm/lapse"),
        open_task: () => answer("realm/open_task.opened"),
    },
};

const KEY = "sk-test";

const FAILED_WORDS = "the model answered 529";

const STOPPING: Answers = {
    workspace: { cancel: () => answer("workspace/cancel.cancelled") },
    realm: { lapse: () => answer("realm/lapse") },
};

function uuid(n: number): string {
    return `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
}

async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(
        agent.by(from.realm),
        submitted(undefined, { type: "line", text }),
    );
    await agent.idle();
}

function chief(
    t: TestContext,
    calls: Calls,
    answers: readonly string[],
): Promise<Woken> {
    return wakeAgent(t, {
        calls,
        input: HAND,
        responses: answers.map((text) => fauxAssistantMessage(text)),
    });
}

function newSession(agent: Woken) {
    return agent.actions.new_session(agent.by(from.client), {
        id: uuid(2),
        to: 7,
    });
}

function retry(agent: Woken) {
    return agent.actions.retry(agent.by(from.client), { id: uuid(6), to: 7 });
}

function modelFailed(words: string): AssistantMessage {
    return fauxAssistantMessage([], {
        stopReason: "error",
        errorMessage: words,
    });
}

async function failed(
    t: TestContext,
    calls: Calls,
    task: string,
    words: string,
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        responses: [modelFailed(words), fauxAssistantMessage(LAST_WORDS)],
    });
    await agent.actions.submit(agent.by(from.realm), submitted(task));
    await agent.idle();
    return agent;
}

async function stopped(t: TestContext, calls: Calls): Promise<Woken> {
    const agent = await wakeAgent(t, { calls, input: HAND });
    assert.deepEqual(
        await agent.actions.stop(agent.by(from.client), { id: uuid(1) }),
        { turns: 0 },
    );
    return agent;
}

const LOGIN = (answer("realm/credential.oauth") as { login: OauthLogin }).login;

function anthropic(keysSeen: (string | undefined)[]) {
    const core = createFauxCore({
        provider: "anthropic",
        models: [{ id: "claude-opus", name: "Claude Opus" }],
    });
    const answered: FauxResponseFactory = (_transcript, options) => {
        keysSeen.push(options?.apiKey);
        return fauxAssistantMessage(LAST_WORDS);
    };
    core.setResponses([answered, answered]);
    const provider = createProvider({
        id: "anthropic",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            apiKey: {
                name: "anthropic key",
                login: async (interaction) => ({
                    type: "api_key",
                    key: await interaction.prompt({
                        type: "secret",
                        message: "Paste your key.",
                    }),
                }),
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
            oauth: {
                name: "anthropic subscription",
                login: async () => {
                    throw new Error(
                        "a subscription to anthropic is not offered",
                    );
                },
                refresh: async () => ({
                    ...JSON.parse(LOGIN.extra ?? "{}"),
                    type: "oauth",
                    access: LOGIN.access,
                    refresh: LOGIN.refresh,
                    expires: LOGIN.expires * 1000,
                }),
                toAuth: async (credential) => ({ apiKey: credential.access }),
            },
        },
    });
    return (models: Models): Models => {
        (models as MutableModels).setProvider(provider);
        return models;
    };
}

async function onAnthropic(
    t: TestContext,
    calls: Calls,
    keysSeen: (string | undefined)[] = [],
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        models: anthropic(keysSeen),
    });
    await agent.actions.copies(agent.by(from.realm), {
        model: { version: 1, value: { model: "anthropic::claude-opus" } },
    });
    return agent;
}

async function signedIn(t: TestContext, calls: Calls): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        models: anthropic([]),
    });
    const { login } = chatActions({
        ...agent.config,
        signIns: signIns(agent.config.models),
    });
    const by = agent.by(from.client);
    let answered = await login(by, {
        id: uuid(7),
        start: { provider: "anthropic", method: "key" },
    });
    for (let n = 8; answered.type === "step"; n += 1) {
        answered = await login(by, {
            id: uuid(n),
            reply: { login: answered.login, value: KEY },
        });
    }
    assert.deepEqual(answered, { type: "done", login: answered.login });
    return agent;
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
    { fixture: "realm/read_copies.none" },
    {
        fixture: "realm/read_copies",
        drive: async (t, calls, sent) => {
            const first = await wakeAgent(t, {
                calls,
                input: HAND,
                models: anthropic([]),
            });
            const [copies] = fixture("agent/copies.args") as [Copies];
            await first.actions.copies(first.by(from.realm), copies);
            await first.stop();
            const again = await first.again();
            await until(() => named(sent, "read_copies").length === 2);
            return again;
        },
        answers: {
            realm: { credential: () => answer("realm/credential.key") },
        },
    },
    {
        fixture: "realm/turn_failed.model",
        drive: (t, calls) => failed(t, calls, "task-2", FAILED_WORDS),
        answers: { realm: { turn_failed: () => answer("realm/turn_failed") } },
        own: ["conversation"],
    },
    {
        fixture: "realm/turn_failed.budget",
        task: "task-2",
        answers: {
            realm: {
                may_spend: () => ({
                    ...(fixture(
                        "realm/may_spend.refused.over_cost.answer",
                    ) as Done),
                    words: FAILED_WORDS,
                }),
                turn_failed: () => answer("realm/turn_failed"),
            },
        },
        own: ["conversation"],
    },
    {
        fixture: "workspace/cancel.all",
        drive: stopped,
        answers: STOPPING,
    },
    {
        fixture: "realm/lapse.all",
        drive: stopped,
        answers: STOPPING,
        // As today, a stop lapses every wait the agent raised
        // (`crates/hamlet_server/src/talk/chats.rs:443-447`).
        constant: { why: "stopped" },
    },
    {
        fixture: "realm/lapse.ended",
        drive: async (t, calls) => {
            const agent = await chief(t, calls, ["One."]);
            await say(agent, "Go.");
            assert.deepEqual(await newSession(agent), { number: 2 });
            return agent;
        },
        answers: COMMANDED,
        own: ["conversation"],
    },
    {
        fixture: "realm/lapse.resumed",
        drive: async (t, calls) => {
            const agent = await chief(t, calls, ["One."]);
            await say(agent, "Go.");
            await newSession(agent);
            assert.deepEqual(
                await agent.actions.resume(agent.by(from.client), {
                    id: uuid(3),
                    number: 1,
                    to: 7,
                }),
                { type: "back", number: 1 },
            );
            return agent;
        },
        answers: COMMANDED,
        own: ["conversation"],
    },
    {
        fixture: "realm/lapse.rewound",
        drive: async (t, calls) => {
            const agent = await chief(t, calls, ["One."]);
            await say(agent, "Go.");
            const page = await agent.actions.read_chat(agent.by(from.client), {
                id: uuid(4),
                chat: CHIEF,
            });
            const line = page.items.find((item) => item.kind === "user");
            assert.ok(line);
            assert.deepEqual(
                await agent.actions.rewind(agent.by(from.client), {
                    id: uuid(5),
                    chat: CHIEF,
                    item: line.id,
                }),
                { type: "rewound", text: "Go." },
            );
            return agent;
        },
        answers: COMMANDED,
        own: ["conversation"],
    },
    {
        fixture: "realm/open_task.retry",
        drive: async (t, calls, sent) => {
            const agent = await failed(t, calls, "task-2", FAILED_WORDS);
            assert.deepEqual(await retry(agent), { type: "done" });
            await agent.idle();
            const { task } = answer("realm/open_task.opened") as {
                task: string;
            };
            assert.deepEqual(named(sent, "may_spend").at(-1)?.message, {
                task,
            });
            return agent;
        },
        answers: COMMANDED,
    },
    {
        fixture: "realm/open_task.reflection",
        drive: async (t, calls) => {
            const agent = await chief(t, calls, ["One.", "Kept."]);
            await say(agent, "Go.");
            assert.equal(
                await agent.actions.reflect(agent.by(from.client)),
                "asked",
            );
            await agent.idle();
            return agent;
        },
        answers: COMMANDED,
    },
    {
        fixture: "realm/credential",
        drive: async (t, calls) => {
            const keysSeen: (string | undefined)[] = [];
            const agent = await onAnthropic(t, calls, keysSeen);
            await say(agent, "Go.");
            assert.deepEqual(keysSeen, [KEY]);
            return agent;
        },
        answers: {
            realm: { credential: () => answer("realm/credential.key") },
        },
    },
    {
        fixture: "realm/keep_credential.key",
        drive: signedIn,
        answers: {
            realm: { keep_credential: () => answer("realm/keep_credential") },
        },
    },
    {
        fixture: "realm/keep_credential.oauth",
        drive: async (t, calls) => {
            const keysSeen: (string | undefined)[] = [];
            const agent = await onAnthropic(t, calls, keysSeen);
            await say(agent, "Go.");
            assert.deepEqual(keysSeen, [LOGIN.access]);
            return agent;
        },
        answers: {
            realm: {
                credential: () => answer("realm/credential.oauth"),
                keep_credential: () => answer("realm/keep_credential"),
            },
        },
    },
    ...(["worker", "errand"] as const).flatMap((kind): Written[] => {
        const done = { [`${kind}_done`]: () => answer(`realm/${kind}_done`) };
        return [
            {
                fixture: `realm/${kind}_done.answered`,
                drive: (t, calls) =>
                    wakeAgent(t, {
                        calls,
                        input: fixture(`agent/${kind}.input`) as AgentInput,
                        responses: [
                            fauxAssistantMessage("Three links are broken"),
                        ],
                    }),
                answers: { realm: done },
            },
            {
                fixture: `realm/${kind}_done.failed`,
                drive: (t, calls) =>
                    wakeAgent(t, {
                        calls,
                        input: fixture(`agent/${kind}.input`) as AgentInput,
                        responses: [modelFailed("the model failed")],
                    }),
                answers: { realm: done },
            },
        ];
    }),
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
                  })
                : await row.drive(t, calls, sent);
        const { to, action } = actionOf(row.fixture);
        const of = () =>
            sent.filter((one) => one.to === to && one.name === action);
        await until(() => of().length > 0);
        assert.deepEqual(agent.reported, []);
        await agent.stop();

        const call = of().at(-1);
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
    "agent/submit.line.reviewed",
    "agent/submit.notice",
    "agent/submit.question",
    "agent/submit.worker_answer",
] as const;

const CREATED = [
    "agent/errand",
    "agent/hand",
    "agent/hire",
    "agent/worker",
] as const;

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
        const { calls, sent } = fakeCalls({
            realm: {
                worker_done: () => answer("realm/worker_done"),
                errand_done: () => answer("realm/errand_done"),
            },
        });
        const created = fixture(name) as AgentInput;
        const agent = await wakeAgent(t, {
            calls,
            input: created,
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        if (created.kind === "worker" || created.kind === "errand") {
            const done = `${created.kind}_done`;
            await until(() => named(sent, done).length === 1);
        }
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
        const given = fixture(name) as {
            type: string;
            why?: { type: string };
            words?: string;
        };
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
            assert.ok(
                given.why?.type === "objection"
                    ? detail === given.words
                    : detail.endsWith(`: ${given.words}`),
                detail,
            );
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

test("realm/open_task.refused.you_left.answer refuses a retry with its words, and a reflection is asked on the standing task", async (t) => {
    const given = answer("realm/open_task.refused.you_left") as Extract<
        TaskOpened,
        { type: "refused" }
    >;
    const { calls, sent } = fakeCalls({
        realm: { open_task: () => given, lapse: () => answer("realm/lapse") },
    });
    const agent = await failed(t, calls, "task-2", FAILED_WORDS);
    await assert.rejects(retry(agent), new Error(given.words));
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
    const prompt = async () =>
        (await agent.actions.read_prompt(agent.by(from.client))).text;
    await until(async () =>
        (await prompt()).includes(given.charter?.value ?? "no charter"),
    );
    assert.deepEqual(agent.reported, []);
    await agent.stop();
});

test("a client's call is refused in its fixture's words", {
    todo: "the agent words a refusal as today's server does, and `hamlet_messages` words it otherwise",
}, async (t) => {
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

test("the agent's chat events equal their fixtures, and its status, usage and reset have their fixture's form", async (t) => {
    const { calls } = fakeCalls(COMMANDED);
    const agent = await wakeAgent(t, {
        calls,
        key: "12",
        input: HAND,
        responses: ["Hello back.", "One.", "Kept.", "Two."].map((text) =>
            fauxAssistantMessage(text),
        ),
    });
    const send = async (id: string, input: Submit["input"]) => {
        await agent.actions.submit(agent.by(from.realm), { id, input });
        await agent.idle();
    };
    await send("input-1", { type: "letter", from: { agent: 3 }, text: "Hi." });
    await send("input-2", { type: "line", text: "Go." });
    await agent.actions.new_session(agent.by(from.client), {
        id: uuid(2),
        to: 12,
    });
    await send("input-3", { type: "line", text: "Again." });
    const harness = await agent.harness();
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
    for (const op of whole.ops.filter(
        (one) => one.type !== "add" && one.type !== "append",
    )) {
        const mine = ops.find((one) => one.type === op.type);
        assert.ok(mine, `the agent sent no ${op.type}`);
        assert.deepStrictEqual(formOf(mine), formOf(op), op.type);
    }
});

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
    [
        /^realm\/lapse\.stopped\.args$/,
        "a stop's lapse of one conversation: as today, `/stop` lapses every wait the agent raised",
    ],
    [/^realm\/(needs_chief|secrets)\./, "the workspace's call of the realm"],
    [
        /^workspace\/(set_reach|cancel_agent|chief_answer)\./,
        "the realm's call of the workspace",
    ],
    [
        /^workspace\/cancel\.(conversation|one)\.args$/,
        "a cancel of one conversation or one call: `/stop` cancels every command the agent has out",
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
        /^agent\/(read_chat|rewind\.rewound)\.answer$/,
        "an item with files and images, which no item of the agent's carries",
    ],
    [
        /^agent\/retry\.refused\.no_turn\.answer$/,
        "a retry refused for want of a turn: as today, a session with nothing in it is carried on",
    ],
    [
        /^agent\/chat\.event\.answer$/,
        "an item with both an author and a rewind, which no item of the agent's has: a chief's line names no author",
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
    [
        /^realm\/(agree_road|ask_chief|object|start_worker|read_budget|read_record|set_allowance|set_model|set_nickname|fetch_part)\./,
        "an agent's call of the realm, which this agent does not make",
    ],
    [
        /^realm\/hire\.in_a_turn\.args$/,
        "a hire with the task and conversation of the turn that asks, which this agent does not write: it sends the call and the act only",
    ],
    [
        /^agent\/submit\.(letter|question)\.files\.args$/,
        "a letter or a question with a parcel: the agent takes it and drops the parcel, so the model is never told of its files",
    ],
    [/^workspace\/parcel(_part)?\./, "the realm's call of the workspace"],
];

const CLIENT_CALLS = new RegExp(
    `^agent/(${[...Object.keys(readClient), "copies"].join("|")})(\\..+)?\\.(args|answer)$`,
);

function listed(name: string): boolean {
    return NOT_THE_AGENTS.some(([pattern]) => pattern.test(name));
}

test("every fixture is one the agent writes or takes, or is listed with its reason, never both", () => {
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
        ...[
            "realm/credential",
            "realm/keep_credential",
            "realm/lapse",
            "realm/open_task",
            "realm/read_copies",
            "realm/turn_failed",
            "realm/worker_done",
            "realm/errand_done",
            "workspace/cancel",
        ].flatMap(answersTo),
        "agent/chat.event.letters.answer",
        "agent/chat.event.past.answer",
        ...fixtures(CLIENT_CALLS).filter((name) => !listed(name)),
        "realm/charge.answer",
        // `workspace/run.tool` waits until the tool it lists is offered.
        "workspace/list_tools.answer",
        ...fixtures(/^error\/.*\.run\./),
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

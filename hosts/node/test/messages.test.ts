import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { type TestContext, test } from "node:test";
import { isDeepStrictEqual } from "node:util";
import {
    createFauxCore,
    createProvider,
    type FauxResponseFactory,
    type FauxResponseStep,
    fauxAssistantMessage,
    type Models,
    type MutableModels,
    type ProviderEnv,
} from "@earendil-works/pi-ai";
import { ActorError, type Calls } from "../src/actor/actor/mod.ts";
import { chatActions } from "../src/agent/chat.ts";
import { signIns } from "../src/agent/login.ts";
import {
    type AgentInput,
    type ChatEvent,
    type ChatRef,
    type Copies,
    readClient,
    type Submit,
} from "../src/agent/messages/agent.ts";
import { LIBRARY } from "../src/agent/messages/library.ts";
import { profileAt } from "../src/agent/messages/profile.ts";
import {
    type CredentialAnswer,
    type Done,
    type Kept,
    type OauthLogin,
    type Part,
    type PlaceText,
    REALM,
    type SecretList,
    type TaskOpened,
} from "../src/agent/messages/realm.ts";
import { U32_MAX } from "../src/agent/messages/shared.ts";
import { WORKSPACE } from "../src/agent/messages/workspace.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    modelFailed,
    named,
    priced,
    type Sent,
    type ToolResult,
    toolCall,
    toolResults,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context, FIXTURES, fixture } from "./testing.ts";

function answer(name: string): never {
    return fixture(`${name}.answer`) as never;
}

function fixtures(pattern: RegExp): string[] {
    return readdirSync(FIXTURES)
        .flatMap((folder) =>
            readdirSync(new URL(`${folder}/`, FIXTURES)).map(
                (file) => `${folder}/${file.replace(/\.cbor$/, "")}`,
            ),
        )
        .filter((name) => pattern.test(name))
        .sort();
}

type Answers = Parameters<typeof fakeCalls>[0];

const HAND = fixture("agent/hand.input") as AgentInput;

/** A hire with no project, so its prompt reads no file of the workspace. */
const HIRE: AgentInput = (() => {
    const hire = fixture("agent/hire.input") as AgentInput;
    const { folder: _, ...place } = hire.place;
    return { ...hire, place, copies: {} };
})();

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
        files?: Readonly<Record<string, string>>;
        agent?: AgentInput;
    },
): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        input: given.agent ?? HAND,
        ...(given.files === undefined
            ? {}
            : { files: fakeFiles({ ...given.files }) }),
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
    /** The agent's filesystem, by path. */
    readonly files?: Readonly<Record<string, string>>;
    readonly offered?: string;
    readonly models?: (models: Models) => Models;
    /** The agent woken; the Hand when left out. */
    readonly agent?: AgentInput;
    /**
     * Fields of the first argument the agent makes itself, which the test
     * cannot steer. Each is checked for the agent's form and then taken from
     * the fixture.
     */
    readonly own?: readonly string[];
    /** Fields the agent always writes the same, checked and then taken from the fixture. */
    readonly constant?: Readonly<Record<string, unknown>>;
    /**
     * Drives a feature other than a turn of the Hand's and gives back the
     * agent it woke; its last call of the fixture's action is compared.
     */
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
        {
            charge: /^m[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
            save: /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
        }[action] ?? /^t[1-9][0-9]*$/;
    return {
        call: (value) => typeof value === "string" && call.test(value),
        part: (value) => Number.isInteger(value) && (value as number) >= 0,
        last: (value) => typeof value === "boolean",
        conversation: (value) =>
            typeof value === "string" && /^[1-9][0-9]*$/.test(value),
        took_ms: (value) =>
            Number.isInteger(value) &&
            (value as number) >= 0 &&
            (value as number) <= U32_MAX,
        request: (value) =>
            typeof value === "string" &&
            /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(value),
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

const REWINDING: Answers = { ...STOPPING, ...COMMANDED };

function uuid(n: number): string {
    return `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
}

/** A chief's line, then its rewind. */
async function rewound(t: TestContext, calls: Calls): Promise<Woken> {
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

/** The realm's login in the fixtures, which expired long ago. */
const LOGIN = (answer("realm/credential.oauth") as { login: OauthLogin }).login;

/** The settings of the key in the fixtures with settings. */
const SETTINGS = (
    answer("realm/credential.key.settings") as { env: ProviderEnv }
).env;

/**
 * A stand-in for the provider the credential fixtures name, so no request
 * leaves the host. Its key sign-in gives `settings` with the key.
 */
function anthropic(
    keysSeen: (string | undefined)[],
    {
        settings,
        envsSeen,
    }: { settings?: ProviderEnv; envsSeen?: unknown[] } = {},
) {
    const core = createFauxCore({
        provider: "anthropic",
        models: [{ id: "claude-opus", name: "Claude Opus" }],
    });
    const answered: FauxResponseFactory = (_transcript, options) => {
        keysSeen.push(options?.apiKey);
        envsSeen?.push(options?.env);
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
                    ...(settings === undefined ? {} : { env: settings }),
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

async function signedIn(
    t: TestContext,
    calls: Calls,
    models = anthropic([]),
): Promise<Woken> {
    const agent = await wakeAgent(t, { calls, input: HAND, models });
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
    assert.ok(answered.type === "done", JSON.stringify(answered));
    return agent;
}

/** Answers each call with the next of `answers`. */
function answering<T>(...answers: T[]): () => T {
    return () => {
        const next = answers.shift();
        assert.ok(next !== undefined, "asked more than was answered");
        return next;
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
        fixture: "realm/charge.tokens",
        models: (models) =>
            priced(models, 0.0025, { input: 1200, output: 280 }),
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
        answers: { realm: { tell: () => answer("realm/tell.sent") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/tell.no_task",
        tool: ["tell", { to: "hand", text: "The tests pass" }],
        answers: { realm: { tell: () => answer("realm/tell.sent") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/tell.paths",
        task: "task-1",
        tool: [
            "tell",
            {
                to: "mayor",
                text: "The report",
                paths: ["/work/report.md", "/work/figures"],
            },
        ],
        answers: { realm: { tell: () => answer("realm/tell.sent") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/ask.paths",
        task: "task-1",
        tool: [
            "ask",
            {
                to: "hire#12",
                question: "Is this plan right?",
                paths: ["/work/plan.md"],
            },
        ],
        answers: { realm: { ask: () => answer("realm/ask.waiting") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/ask",
        task: "task-1",
        tool: ["ask", { to: "hire#12", question: "Which tests fail?" }],
        answers: { realm: { ask: () => answer("realm/ask.waiting") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/request_road",
        agent: HIRE,
        task: "task-1",
        tool: [
            "request_road",
            { to: "hire#13", reason: "To ask about the tests" },
        ],
        answers: {
            realm: { request_road: () => answer("realm/request_road.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/request_road.no_why",
        agent: HIRE,
        tool: ["request_road", { to: "mayor" }],
        answers: {
            realm: { request_road: () => answer("realm/request_road.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/found",
        tool: ["found_hamlet", { name: "Harbor", purpose: "Run the release" }],
        answers: { realm: { found: () => answer("realm/found.founded") } },
        own: ["call", "conversation"],
        // The tool has no tier to choose: a hamlet an agent founds has
        // today's default, 50. The fixture's 500 is a chief's choice.
        constant: { "act.tier": "fifty" },
    },
    {
        fixture: "realm/hire",
        tool: ["hire", { purpose: "Check the links", under: "hire#12" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/hire.in_a_turn",
        task: "standing-12",
        tool: ["hire", { purpose: "Check the links" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/hire.at_a_version",
        tool: ["hire", { scholar: "scholar#4@v1", purpose: "Check the links" }],
        answers: { realm: { hire: () => answer("realm/hire.hired") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/create_profile",
        tool: [
            "create_scholar",
            { name: "Ada", character: "You write careful tests." },
        ],
        answers: {
            realm: {
                create_profile: () => answer("realm/create_profile.made"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/create_profile.realms_character",
        tool: ["create_scholar", { name: "Ada" }],
        answers: {
            realm: {
                create_profile: () => answer("realm/create_profile.made"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/read_profile",
        tool: ["read_scholar", { scholar: "scholar#4" }],
        answers: {
            realm: { read_profile: () => answer("realm/read_profile.listed") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/read_profile.file",
        tool: [
            "read_scholar",
            { scholar: "scholar#4", path: "memory/tests.md", version: "v1" },
        ],
        answers: {
            realm: { read_profile: () => answer("realm/read_profile.text") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/edit_profile",
        tool: [
            "edit_scholar",
            {
                scholar: "scholar#4",
                path: "CHARACTER.md",
                content: "You write careful tests.",
                message: "Sharper",
            },
        ],
        answers: {
            realm: {
                edit_profile: () =>
                    answer("realm/edit_profile.refused.reserved"),
            },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/tag",
        tool: [
            "tag",
            { scholar: "scholar#4", tag: "v1", note: "Before the refactor" },
        ],
        answers: { realm: { tag: () => answer("realm/tag.refused.reserved") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/revert",
        tool: [
            "revert",
            {
                scholar: "scholar#4",
                commit: "ab".repeat(20),
                reason: "It broke the tests",
            },
        ],
        answers: {
            realm: { revert: () => answer("realm/revert.refused.reserved") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/ask_librarian",
        tool: ["ask_librarian", { question: "How do I lint the docs?" }],
        answers: {
            realm: { ask_librarian: () => answer("realm/ask_librarian.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/submit_skill",
        tool: [
            "submit_skill",
            { skill: "lint-docs", reason: "It checks every link" },
        ],
        files: { "skills/lint-docs/SKILL.md": "---\nname: lint-docs\n---\n" },
        answers: {
            library: { offer: () => ({ type: "kept", offer: 3 }) },
            realm: { submit_skill: () => answer("realm/submit_skill.done") },
        },
        own: ["call", "conversation"],
    },
    // The numbering of an offer's or a save's parts is `test/branch.test.ts`'s.
    ...["library/offer", "library/offer.last"].map(
        (fixture): Written => ({
            fixture,
            tool: [
                "submit_skill",
                { skill: "fixtures", reason: "Every hire writes fixtures" },
            ],
            files: { "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n" },
            answers: {
                library: { offer: () => answer("library/offer.kept") },
                realm: {
                    submit_skill: () => answer("realm/submit_skill.done"),
                },
            },
            own: ["call", "part", "last"],
        }),
    ),
    // Asked at each wake of a hand or a hire.
    {
        fixture: "library/shelves",
        answers: { library: { shelves: () => answer("library/shelves") } },
    },
    {
        fixture: "library/shelves.seen",
        drive: async (t, calls) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.shelves_changed(agent.by(from.library), {
                list: { version: 4, skills: [] },
            });
            await agent.stop();
            return agent.again();
        },
    },
    {
        fixture: "profile/files.tip",
        drive: async (t, calls) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.fill(agent.by(from.realm), { profile: 2 });
            return agent;
        },
        answers: {
            profile: { files: () => answer("profile/files.part.last") },
        },
    },
    {
        fixture: "profile/files.since",
        drive: async (t, calls, sent) => {
            const agent = await wakeAgent(t, { calls, input: HAND });
            await agent.actions.fill(agent.by(from.realm), { profile: 2 });
            await agent.actions.moved(agent.by(from.profile(2)), {
                tip: "cd".repeat(20),
            });
            await until(() => named(sent, "files").length === 3);
            return agent;
        },
        answers: {
            profile: {
                files: answering(
                    {
                        type: "part",
                        tip: "12".repeat(20),
                        files: [],
                        removed: [],
                    },
                    answer("profile/files.part"),
                    answer("profile/files.part.last"),
                ),
            },
        },
    },
    ...["profile/save", "profile/save.last"].map(
        (fixture): Written => ({
            fixture,
            drive: async (t, calls) => {
                const files: Record<string, string> = {};
                const agent = await wakeAgent(t, {
                    calls,
                    input: HAND,
                    files: fakeFiles(files),
                    responses: [fauxAssistantMessage(LAST_WORDS)],
                });
                await agent.actions.fill(agent.by(from.realm), { profile: 2 });
                files["memory/tests.md"] = "# Notes\n";
                delete files["memory/old.md"];
                await agent.actions.submit(
                    agent.by(from.realm),
                    submitted(undefined),
                );
                await agent.idle();
                return agent;
            },
            answers: {
                profile: {
                    files: answering(
                        {
                            type: "part",
                            tip: "ab".repeat(20),
                            files: ["memory/tests.md", "memory/old.md"].map(
                                (path) => ({
                                    path,
                                    offset: 0,
                                    bytes: Buffer.from("old\n").toString(
                                        "base64",
                                    ),
                                    last: true,
                                }),
                            ),
                            removed: [],
                        },
                        answer("profile/files.part.last"),
                    ),
                    save: () => answer("profile/save.saved"),
                },
            },
            own: ["call", "part", "last"],
        }),
    ),
    {
        fixture: "realm/end",
        tool: ["end_hire", { hire: "hire#12" }],
        answers: { realm: { end: () => answer("realm/end.done") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/end.worker",
        tool: ["end_hire", { hire: "worker#15" }],
        answers: { realm: { end: () => answer("realm/end.done") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/seat",
        tool: ["seat_mayor", { hire: "hire#13" }],
        answers: { realm: { seat: () => answer("realm/seat.done") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/lay_road",
        tool: ["lay_road", { a: "hire#12", b: "hire#13" }],
        answers: { realm: { lay_road: () => answer("realm/lay_road.done") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/take_up_road",
        tool: ["take_up_road", { a: "hire#12", b: "mayor" }],
        answers: {
            realm: { take_up_road: () => answer("realm/take_up_road.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/set_tools",
        tool: ["set_tools", { hire: "hire#12", off: ["tell"] }],
        answers: {
            realm: { set_tools: () => answer("realm/set_tools.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/seat.why",
        tool: [
            "seat_mayor",
            { hire: "hire#12", reason: "To keep the tests apart" },
        ],
        answers: { realm: { seat: () => answer("realm/seat.done") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/lay_road.why",
        tool: [
            "lay_road",
            { a: "hire#12", b: "hire#13", reason: "To keep the tests apart" },
        ],
        answers: { realm: { lay_road: () => answer("realm/lay_road.done") } },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/take_up_road.why",
        tool: [
            "take_up_road",
            { a: "hire#12", b: "mayor", reason: "To keep the tests apart" },
        ],
        answers: {
            realm: { take_up_road: () => answer("realm/take_up_road.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/set_tools.why",
        tool: [
            "set_tools",
            {
                hire: "hire#12",
                on: ["tell"],
                reason: "To keep the tests apart",
            },
        ],
        answers: {
            realm: { set_tools: () => answer("realm/set_tools.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/ask_chief",
        tool: [
            "ask_chief",
            {
                message: "Before planting",
                questions: [
                    {
                        question: "Which seed?",
                        description: "The field is wet",
                        options: [
                            "Oats",
                            { title: "Barley", description: "Slower" },
                        ],
                        text_input: false,
                    },
                    { question: "When?" },
                ],
            },
        ],
        answers: { realm: { ask_chief: () => answer("realm/ask_chief.done") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/ask_chief.no_message",
        tool: ["ask_chief", { questions: [{ question: "Go on?" }] }],
        answers: { realm: { ask_chief: () => answer("realm/ask_chief.done") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/object",
        tool: ["object", { reason: "The field is better left alone" }],
        answers: { realm: { object: () => answer("realm/object.done") } },
        own: ["call", "conversation", "act.conversation"],
    },
    {
        fixture: "realm/start_worker",
        tool: [
            "start_worker",
            { task: "Check the links in the docs", tools: ["read"] },
        ],
        answers: {
            realm: { start_worker: () => answer("realm/start_worker.started") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/set_allowance",
        tool: [
            "set_allowance",
            {
                hire: "hire#12",
                dollars: 5000,
                reason: "To keep the tests apart",
            },
        ],
        answers: {
            realm: { set_allowance: () => answer("realm/set_allowance.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/read_budget.own",
        tool: ["read_budget", {}],
        answers: {
            realm: { read_budget: () => answer("realm/read_budget.budget") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/agree_road",
        agent: HIRE,
        tool: [
            "agree_road",
            {
                mine: "hire#12",
                theirs: "hire#31",
                reason: "To keep the tests apart",
            },
        ],
        answers: {
            realm: { agree_road: () => answer("realm/agree_road.waiting") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/agree_road.mayor_of",
        agent: HIRE,
        tool: [
            "agree_road",
            {
                mine: "hire#12",
                theirs: "mayor@5f0d2c1b-7a3e-4d9f-8b6a-1c2d3e4f5a6b",
            },
        ],
        answers: {
            realm: { agree_road: () => answer("realm/agree_road.waiting") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/set_nickname",
        tool: ["nickname", { hire: "hire#12", nickname: "Spade" }],
        answers: {
            realm: { set_nickname: () => answer("realm/set_nickname.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/read_record",
        tool: [
            "read_record",
            { hamlet: "5f0d2c1b-7a3e-4d9f-8b6a-1c2d3e4f5a6b" },
        ],
        answers: {
            realm: { read_record: () => answer("realm/read_record.read") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/fetch_part",
        input: {
            type: "letter",
            from: { agent: 12, name: "Ada" },
            text: "The report",
            files: [
                {
                    parcel: 8,
                    file: 1,
                    path: "from-hire-12/report.md",
                    size: 2048,
                },
            ],
        },
        answers: {
            realm: {
                fetch_part: ({ part }) =>
                    answer(
                        part < 3
                            ? "realm/fetch_part.bytes"
                            : "realm/fetch_part.bytes.last",
                    ),
            },
        },
    },
    { fixture: "realm/read_place" },
    {
        fixture: "realm/read_place.letters",
        input: {
            type: "letter",
            from: { agent: 12, name: "hire#12 (Ada)" },
            text: "The tests pass",
        },
    },
    {
        fixture: "realm/read_place.questions",
        task: "task-1",
        input: {
            type: "question",
            letter: 5,
            from: { agent: 12, name: "hire#12 (Ada)" },
            text: "How many fail?",
        },
        answers: { realm: { reply: () => answer("realm/reply.done") } },
    },
    {
        // A worker's brief is its one conversation, which is no chat.
        fixture: "realm/read_place.other",
        drive: async (t, calls, sent) => {
            const agent = await wakeAgent(t, {
                calls,
                input: fixture("agent/worker.input") as AgentInput,
                responses: [fauxAssistantMessage(LAST_WORDS)],
            });
            await until(() => named(sent, "worker_done").length === 1);
            return agent;
        },
        answers: { realm: { worker_done: () => null } },
    },
    // Sent at the first wake, which holds no copy yet.
    { fixture: "realm/read_copies.none" },
    {
        fixture: "realm/secrets",
        answers: { realm: { secrets: () => answer("realm/secrets") } },
    },
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
        fixture: "workspace/cancel.conversation",
        drive: rewound,
        answers: REWINDING,
        own: ["which.conversation"],
    },
    {
        fixture: "realm/lapse.rewound",
        drive: rewound,
        answers: REWINDING,
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
        own: ["request"],
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
        own: ["request"],
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
        drive: (t, calls) => signedIn(t, calls),
        answers: {
            realm: { keep_credential: () => answer("realm/keep_credential") },
        },
    },
    {
        fixture: "realm/keep_credential.key.settings",
        drive: (t, calls) =>
            signedIn(t, calls, anthropic([], { settings: SETTINGS })),
        answers: {
            realm: { keep_credential: () => answer("realm/keep_credential") },
        },
    },
    {
        // The realm's expired login is rotated. A rotation runs once in the
        // host, so only this test reads that answer.
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

/** The fixtures' profile is the one the agent was filled from. */
const ADDRESSES: Readonly<Record<string, { kind: string; key: string }>> = {
    workspace: WORKSPACE,
    library: LIBRARY,
    profile: profileAt(2),
};

function actionOf(name: string): { to: string; action: string } {
    const [folder = "", rest = ""] = name.split("/");
    const to = ADDRESSES[folder] ?? REALM;
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
        // Each action takes one message, but `list_tools` and `secrets` take
        // none.
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

/** Each action the agent takes, by whom it is sent and the fixture of its answer. */
const TAKERS = {
    submit: { by: from.realm, answer: "agent/submit.answer" },
    finished: { by: from.workspace, answer: "agent/finished.answer" },
    place: { by: from.realm, answer: "agent/place.answer" },
    fill: { by: from.realm, answer: "agent/fill.answer" },
    moved: { by: from.profile(2), answer: "profile/moved.answer" },
    remove_path: {
        by: from.profile(2),
        answer: "profile/remove_path.removed.answer",
    },
    shelves_changed: {
        by: from.library,
        answer: "library/shelves_changed.answer",
    },
} as const;

const TAKEN = [
    "agent/fill",
    "library/shelves_changed",
    "profile/moved",
    "profile/moved.tip",
    "profile/remove_path",
    "agent/finished.change.changed",
    "agent/finished.change.failed",
    "agent/finished.change.not_run",
    "agent/finished.run.not_run",
    "agent/finished.run.ran",
    "agent/place",
    "agent/place.hand",
    "agent/submit.answer",
    "agent/submit.letter",
    "agent/submit.letter.files",
    "agent/submit.line",
    "agent/submit.line.attached",
    "agent/submit.line.reviewed",
    "agent/submit.notice",
    "agent/submit.question",
    "agent/submit.question.files",
    "agent/submit.worker_answer",
] as const;

/** What a taken call reports: a file held whole is not fetched past 32 MiB. */
const REPORTED: Partial<Record<(typeof TAKEN)[number], RegExp>> = {
    "agent/submit.letter.files":
        /^from-hire-12\/figures\/plot\.png was not saved in the inbox: it holds more than 32\.0 MiB$/,
};

const CREATED = [
    "agent/errand",
    "agent/hand",
    "agent/hire",
    "agent/worker",
] as const;

for (const call of TAKEN) {
    const name = `${call}.args`;
    test(`${name} is read and taken, and answered as its answer fixture holds`, async (t) => {
        const action = actionOf(name).action as keyof typeof TAKERS;
        const { calls, sent } = fakeCalls({
            realm: {
                reply: () => answer("realm/reply.done"),
                fetch_part: () => answer("realm/fetch_part.bytes.last"),
            },
            profile: { files: () => answer("profile/files.part.last") },
        });
        const agent = await wakeAgent(t, {
            calls,
            input: HAND,
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        // A profile's message comes from the profile the agent was filled from.
        if (name.startsWith("profile/")) {
            await agent.actions.fill(agent.by(from.realm), { profile: 2 });
        }
        const [message, ...more] = fixture(name) as unknown[];
        assert.equal(more.length, 0);
        const take = agent.actions[action] as (
            c: ReturnType<Woken["by"]>,
            message: unknown,
        ) => Promise<unknown>;
        assert.deepStrictEqual(
            await take(agent.by(TAKERS[action].by), message),
            fixture(TAKERS[action].answer),
        );
        // A move's pull follows its answer.
        if (action === "moved") {
            await until(() => named(sent, "files").length === 2);
        }
        await agent.idle();
        const report = REPORTED[call];
        if (report === undefined) {
            assert.deepEqual(agent.reported, []);
        } else {
            assert.equal(agent.reported.length, 1);
            assert.match(agent.reported[0] ?? "", report);
        }
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
        // A worker's or an errand's brief is its one turn, which ends with a call.
        if (created.kind === "worker" || created.kind === "errand") {
            const done = `${created.kind}_done`;
            await until(() => named(sent, done).length === 1);
        }
        assert.deepEqual(agent.reported, []);
        await agent.stop();
    });
}

test("agent/place.args keeps every tool's name and the folder, and turns off only the built-in tools named", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls, input: HAND });
    const [place] = fixture("agent/place.args") as unknown[];
    await agent.actions.place(agent.by(from.realm), place as never);
    const row = await agent.c.db.get<{ tools_off: string; place: string }>(
        "SELECT tools_off, place FROM agent",
    );
    const root = await (await agent.harness()).root(context);
    const names = (await root.agent(context)).tools.map((tool) => tool.name);
    await agent.stop();

    assert.deepEqual(JSON.parse(row?.tools_off ?? ""), [
        { name: "found" },
        { name: "bash" },
    ]);
    assert.equal(
        (JSON.parse(row?.place ?? "") as { folder?: unknown }).folder,
        "/work/orchard",
    );
    assert.ok(!names.includes("bash"), names.join());
    assert.ok(
        names.includes("found_hamlet") && names.includes("read"),
        names.join(),
    );
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
    "realm/ask_chief",
    "realm/object",
    "realm/start_worker",
    "realm/set_allowance",
    "realm/read_budget",
    "realm/agree_road",
    "realm/set_nickname",
    "realm/set_model",
    "realm/read_record",
    "realm/create_profile",
    "realm/read_profile",
    "realm/edit_profile",
    "realm/tag",
    "realm/revert",
    "realm/ask_librarian",
    "realm/submit_skill",
] as const;

function answersTo(action: string): string[] {
    return fixtures(new RegExp(`^${action}(\\..+)?\\.answer$`));
}

/**
 * pi-durable records a throw of a tool's `execute` as a result with no
 * output and a `tool_error` diagnostic.
 */
function threw(result: ToolResult): boolean {
    return result.diagnostics.some(({ code }) => code === "tool_error");
}

/** A tool whose call has no fixture this agent writes. */
const CALLED: Readonly<Record<string, readonly [string, ToolArgs]>> = {
    "realm/set_model": [
        "set_model",
        { hire: "hire#12", model: "anthropic::claude-opus" },
    ],
};

/**
 * A refusal the model is told in today's words of a result, not of an error:
 * a call a chief must allow, and the chief that cannot be asked.
 */
function toldPlainly(given: {
    why?: { type: string; most?: number };
}): string | undefined {
    switch (given.why?.type) {
        case "reserved":
            return "This call waits for a chief's answer.";
        case "no_chief":
            return "No chief oversees this turn; go on with what you know.";
        case "too_many":
            return `You have asked the chief ${given.why.most === 1 ? "once" : `${given.why.most} times`} this turn; go on with what you know.`;
        default:
            return undefined;
    }
}

for (const action of ANSWERED_BY_TOOLS) {
    test(`each answer to ${action} gives its tool a result, never a throw`, async (t) => {
        const row = WRITTEN.find(
            (written) =>
                written.fixture === action ||
                written.fixture.startsWith(`${action}.`),
        );
        const tool = row?.tool ?? CALLED[action];
        assert.ok(tool);
        const answers = answersTo(action).filter((name) => !listed(name));
        const queue = [...answers];
        const [folder = "", name = ""] = action.split("/");
        const { calls } = fakeCalls({
            ...row?.answers,
            [folder]: {
                ...row?.answers?.[folder as keyof Answers],
                [name]: () => fixture(queue.shift() as string),
            },
        });
        const agent = await turn(t, calls, {
            submit: submitted(row?.task),
            responses: answers.map((_, index) =>
                toolCall(tool[0], tool[1], `c${index}`),
            ),
            ...(row?.files === undefined ? {} : { files: row.files }),
            ...(row?.agent === undefined ? {} : { agent: row.agent }),
        });
        const results = await toolResults(agent);
        await agent.stop();

        assert.equal(results.length, answers.length);
        answers.forEach((name, index) => {
            const result = results[index];
            assert.ok(result && !threw(result), `${name} threw`);
            const given = fixture(name) as {
                type: string;
                why?: { type: string; most?: number };
                words?: string;
            };
            const plainly = toldPlainly(given);
            const told = [result.output, result.isError];
            if (plainly !== undefined) {
                assert.deepEqual(told, [plainly, false], name);
            } else if (given.type === "refused") {
                assert.deepEqual(told, [given.words, true], name);
            } else {
                const shown = [result.output]
                    .concat(result.diagnostics.map(({ message }) => message))
                    .join("");
                assert.notEqual(shown, "", name);
                assert.doesNotMatch(shown, NOT_WORDS, name);
            }
        });
    });
}

for (const name of answersTo("profile/files")) {
    test(`${name} is read by a pull: a part is written, and a gone commit is pulled whole`, async (t) => {
        const given = fixture(name) as { type: string };
        const whole = answer("profile/files.part.last");
        const { calls, sent } = fakeCalls({
            profile: {
                files: answering(whole, given as never, whole, whole),
            },
        });
        const agent = await wakeAgent(t, { calls, input: HAND });
        await agent.actions.fill(agent.by(from.realm), { profile: 2 });
        await agent.actions.moved(agent.by(from.profile(2)), {
            tip: "cd".repeat(20),
        });
        const since = { since: "ab".repeat(20) };
        const expected =
            given.type === "refused"
                ? [{}, since, {}]
                : [
                      {},
                      since,
                      ...("next" in given
                          ? [
                                {
                                    ...since,
                                    after: (given as { next: unknown }).next,
                                },
                            ]
                          : []),
                  ];
        await until(() => named(sent, "files").length === expected.length);
        await agent.stop();

        assert.deepEqual(
            named(sent, "files").map((call) => call.message),
            expected,
        );
        assert.deepEqual(agent.reported, []);
    });
}

for (const name of answersTo("profile/save")) {
    test(`${name} is read by a save: a gone commit is filled again and saved once more on its tip, and another refusal is reported in its words`, async (t) => {
        const given = fixture(name) as {
            type: string;
            why?: { type: string };
            words?: string;
        };
        const gone = given.why?.type === "commit_gone";
        const files: Record<string, string> = {};
        const { calls, sent } = fakeCalls({
            profile: {
                files: () => answer("profile/files.part.last"),
                save: () => given as never,
            },
        });
        const agent = await wakeAgent(t, {
            calls,
            input: HAND,
            files: fakeFiles(files),
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        await agent.actions.fill(agent.by(from.realm), { profile: 2 });
        files["memory/tests.md"] = "# Notes\n";
        await agent.actions.submit(agent.by(from.realm), submitted(undefined));
        await agent.idle();
        await agent.stop();

        assert.deepEqual(
            agent.reported,
            given.type === "refused"
                ? [`the filesystem was not saved: ${given.words}`]
                : [],
        );
        assert.equal(named(sent, "save").length, gone ? 2 : 1);
        assert.deepEqual(
            named(sent, "files").map((call) => call.message),
            given.type !== "refused"
                ? [{}, { since: "ab".repeat(20) }]
                : gone
                  ? [{}, {}]
                  : [{}],
        );
    });
}

for (const name of answersTo("library/offer")) {
    test(`${name} is read by submit_skill, as the answer to the first of its parts`, async (t) => {
        const given = fixture(name) as { type: string; words?: string };
        let offers = 0;
        const { calls, sent } = fakeCalls({
            library: {
                offer: () =>
                    offers++ === 0
                        ? (given as never)
                        : answer("library/offer.kept"),
            },
            realm: { submit_skill: () => answer("realm/submit_skill.done") },
        });
        const agent = await turn(t, calls, {
            submit: submitted(undefined),
            responses: [toolCall("submit_skill", { skill: "fixtures" })],
            files: {
                "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n",
                "skills/fixtures/large.md": "a".repeat(800 * 1024),
            },
        });
        const [result] = await toolResults(agent);
        await agent.stop();
        assert.ok(result && !threw(result), `${name} threw`);
        if (given.type === "refused") {
            assert.equal(result.output, given.words);
            assert.equal(named(sent, "offer").length, 1);
            assert.deepEqual(named(sent, "submit_skill"), []);
        } else {
            assert.equal(result.output, "The librarian is judging it.");
            assert.ok(named(sent, "offer").length > 1);
            assert.equal(named(sent, "submit_skill").length, 1);
        }
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

for (const name of answersTo("realm/read_place")) {
    test(`${name}'s sections end the prompt, each that it holds`, async (t) => {
        const given = fixture(name) as PlaceText;
        const { calls } = fakeCalls({
            realm: { read_place: () => given },
        });
        const agent = await turn(t, calls, { submit: submitted(undefined) });
        const prompt = (await agent.actions.read_prompt(agent.by(from.client)))
            .text;
        await agent.stop();

        const sections = [given.place, given.realm, given.scholars].filter(
            (text) => text !== undefined,
        );
        assert.ok(prompt.endsWith(`\n\n${sections.join("\n\n")}`), prompt);
    });
}

for (const name of answersTo("realm/fetch_part")) {
    test(`${name} is read by the parcel's saving`, async (t) => {
        const given = fixture(name) as Part;
        const last = fixture("realm/fetch_part.bytes.last.answer") as Part;
        const held: Record<string, string> = {};
        const { calls } = fakeCalls({
            realm: { fetch_part: ({ part }) => (part === 0 ? given : last) },
        });
        const agent = await wakeAgent(t, {
            calls,
            files: fakeFiles(held),
            responses: [fauxAssistantMessage(LAST_WORDS)],
        });
        await agent.actions.submit(agent.by(from.realm), {
            id: "input-1",
            input: {
                type: "letter",
                from: { agent: 12, name: "Ada" },
                text: "The report",
                files: [
                    {
                        parcel: 8,
                        file: 0,
                        path: "from-hire-12/report.md",
                        size: 2048,
                    },
                ],
            },
        });
        await agent.idle();
        await agent.stop();

        const text = (part: Part) =>
            part.type === "bytes"
                ? Buffer.from(part.bytes, "base64").toString()
                : "";
        if (given.type === "bytes") {
            assert.equal(
                held["inbox/from-hire-12/report.md"],
                text(given) + (given.last ? "" : text(last)),
            );
            assert.deepEqual(agent.reported, []);
        } else {
            assert.deepEqual(held, {});
            assert.deepEqual(agent.reported, [
                `from-hire-12/report.md was not saved in the inbox: ${given.words}`,
            ]);
        }
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
    const prompt = async () =>
        (await agent.actions.read_prompt(agent.by(from.client))).text;
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
    await send("input-1", { type: "letter", from: { agent: 3 }, text: "Hi." });
    await send("input-2", { type: "line", text: "Go." });
    await agent.actions.new_session(agent.by(from.client), {
        id: uuid(2),
        to: 12,
    });
    await send("input-3", {
        type: "line",
        text: "Again.",
        images: [{ id: "ab".repeat(32), at: 6 }, { id: "cd".repeat(32) }],
        files: [
            { id: "12".repeat(32), at: 0, name: "notes/plan.md", size: 2048 },
            { id: "34".repeat(32), name: "log.txt", size: 0 },
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

        assert.ok(result && !threw(result));
        // Today's words for a change that may have run (`calls.rs`).
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

const NOT_THE_AGENTS: readonly (readonly [RegExp, string])[] = [
    [
        /^realm\/(edit_profile\.done|tag\.tagged|revert\.(reverted|unchanged))\.answer$/,
        "the realm keeps an agent's edit, tag and revert for a chief: the tool's answer is always a refusal, and what the chief rules arrives as a notice",
    ],
    [/^realm\/chief_/, "a chief's act, sent by a client"],
    [
        /^realm\/(say|answer|read_world|read_charter)\./,
        "a client's call of the realm",
    ],
    [
        /^realm\/charge\.args$/,
        "a charge without its tokens: the agent counts the tokens of every model call",
    ],
    [
        /^realm\/society_tools\.answer$/,
        "the society tools as a refusal's gate names them: the agent reads only a refusal's words",
    ],
    [
        /^realm\/lapse\.stopped\.args$/,
        "a stop's lapse of one conversation: as today, `/stop` lapses every wait the agent raised",
    ],
    [/^realm\/needs_chief\./, "the workspace's call of the realm"],
    [
        /^workspace\/(set_reach|cancel_agent|chief_answer)\./,
        "the realm's call of the workspace",
    ],
    [
        /^workspace\/cancel\.one\.args$/,
        "a cancel of one call: `/stop` cancels every command the agent has out, and a rewind those of the session it leaves",
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
    [
        /^library\/(read_shelves|read_offer|verdict|unshelve|drop_offer_parts)\./,
        "the realm's call of the library",
    ],
    [
        /^library\/drop_copy\./,
        "the library's call of an agent, an action this agent does not take",
    ],
    [
        /^profile\/(branch|tag|pin|revert|purge|edit|prepare_dream|end_dream|versions|papers|read|erase|drop_save_parts)\./,
        "the realm's call of a profile",
    ],
    [
        /^profile\/(character|fork)\.input$/,
        "the input a profile is created with",
    ],
    [
        /^stored\/(end\.change\.changed|end\.change\.changed\.diff|end\.change\.failed|end\.change\.not_run|end\.change_not_run|end\.change_not_run\.chief_refused|end\.change_not_run\.chief_refused\.no_reason|end\.change_not_run\.lapsed|end\.change_not_run\.lapsed\.stopped|end\.change_not_run\.reach_needs|end\.change_not_run\.reach_refuses\.agent_filesystem|end\.change_not_run\.reach_refuses\.no_reach|end\.change_not_run\.reach_refuses\.outside_reach|end\.change_not_run\.reach_refuses\.project_in_realm|end\.command|end\.command\.cancelled|end\.command\.cut|end\.command\.cut_off|end\.command\.failed|end\.command\.timed_out|end\.run_not_run|end\.run_not_run\.chief_refused|end\.run_not_run\.chief_refused\.no_reason|end\.run_not_run\.lapsed|end\.run_not_run\.lapsed\.stopped|end\.run_not_run\.reach_needs|end\.run_not_run\.reach_refuses\.no_reach|end\.run_not_run\.reach_refuses\.project_in_realm|end\.run_not_run\.tool_arguments|end\.run_not_run\.tool_not_found|hashed\.absent|hashed\.sha256|needs|needs\.edit|reach\.agent|reach\.agent\.auto|reach\.ended|reach\.worker|reach\.worker\.started|request\.edit_file|request\.run|request\.run\.tool|request\.write_file)$/,
        "a row the workspace keeps in its own file",
    ],
    [
        /^stored\/(profile\.edit\.done|profile\.edit\.refused\.character_too_long|profile\.edit\.refused\.not_a_path|profile\.edit\.refused\.withheld|profile\.end_dream\.dreamt|profile\.end_dream\.dreamt\.main_unchanged|profile\.end_dream\.nothing|profile\.erase\.done|profile\.erase\.refused\.dreaming|profile\.input\.character|profile\.input\.fork|profile\.pin\.follows_main|profile\.pin\.pinned|profile\.pin\.refused\.no_hire|profile\.pin\.refused\.no_version|profile\.purge\.purged|profile\.purge\.refused\.dreaming|profile\.purge\.refused\.not_a_path|profile\.revert\.refused\.no_commit|profile\.revert\.refused\.no_version|profile\.revert\.refused\.not_on_main|profile\.revert\.reverted|profile\.revert\.unchanged|profile\.save\.kept|profile\.save\.refused\.commit_gone|profile\.save\.refused\.not_a_path|profile\.save\.refused\.part_too_large|profile\.save\.refused\.withheld|profile\.save\.saved|profile\.tag\.refused\.bad_name|profile\.tag\.refused\.no_such_commit|profile\.tag\.refused\.taken|profile\.tag\.tagged)$/,
        "a row a profile keeps in its own file",
    ],
    [
        /^stored\/(library\.offer\.kept|library\.offer\.more|library\.offer\.refused\.already_shelved|library\.offer\.refused\.being_judged|library\.offer\.refused\.no_front_matter|library\.offer\.refused\.not_a_path|library\.offer\.refused\.not_a_skill_name|library\.offer\.refused\.part_too_large|library\.offer\.refused\.skill_too_large|library\.unshelve\.done|library\.unshelve\.refused\.no_such_skill|library\.verdict\.done|library\.verdict\.refused\.judged|library\.verdict\.refused\.no_such_offer)$/,
        "a row the library keeps in its own file",
    ],
    [
        /^realm\/lapse\.ended\.args$/,
        "a /new, which leaves the waits of the session it left, as today's does",
    ],
    [
        /^realm\/read_budget\.args$/,
        "a budget read for a task's payer: the agent reads its own, as today's read_budget does",
    ],
    [
        /^realm\/set_model\.args$/,
        "a model change with an effort and a speed, which today's set_model does not take",
    ],
    [/^workspace\/parcel(_part)?\./, "the realm's call of the workspace"],
];

const CLIENT_CALLS = new RegExp(
    `^agent/(${[...Object.keys(readClient), "copies"].join("|")})(\\..+)?\\.(args|answer)$`,
);

function listed(name: string): boolean {
    return NOT_THE_AGENTS.some(([pattern]) => pattern.test(name));
}

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
            "realm/secrets",
            "realm/turn_failed",
            "realm/worker_done",
            "realm/errand_done",
            "workspace/cancel",
            "library/shelves",
            "library/offer",
            "profile/files",
            "profile/save",
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

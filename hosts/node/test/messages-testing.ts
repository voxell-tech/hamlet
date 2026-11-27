import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import type { TestContext } from "node:test";
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
import type { Calls } from "../src/actor/actor/mod.ts";
import { chatActions } from "../src/agent/chat.ts";
import { signIns } from "../src/agent/login.ts";
import type {
    AgentInput,
    ChatRef,
    Submit,
} from "../src/agent/messages/agent.ts";
import { LIBRARY } from "../src/agent/messages/library.ts";
import { profileAt } from "../src/agent/messages/profile.ts";
import { type OauthLogin, REALM } from "../src/agent/messages/realm.ts";
import { U32_MAX } from "../src/agent/messages/shared.ts";
import { WORKSPACE } from "../src/agent/messages/workspace.ts";
import {
    type fakeCalls,
    fakeFiles,
    from,
    modelFailed,
    type Sent,
    type ToolResult,
    type toolCall,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context, FIXTURES, fixture } from "./testing.ts";

export function answer(name: string): never {
    return fixture(`${name}.answer`) as never;
}

export function fixtures(pattern: RegExp): string[] {
    return readdirSync(FIXTURES)
        .flatMap((folder) =>
            readdirSync(new URL(`${folder}/`, FIXTURES)).map(
                (file) => `${folder}/${file.replace(/\.cbor$/, "")}`,
            ),
        )
        .filter((name) => pattern.test(name))
        .sort();
}

export type Answers = Parameters<typeof fakeCalls>[0];

export const HAND = fixture("agent/hand.input") as AgentInput;

/** A hire with no project, so its prompt reads no file of the workspace. */
export const HIRE: AgentInput = (() => {
    const hire = fixture("agent/hire.input") as AgentInput;
    const { folder: _, ...place } = hire.place;
    return { ...hire, place, copies: {} };
})();

/** A mayor while another hamlet lives, offered `agree_road`. */
export const MAYOR: AgentInput = (() => {
    const { lead: _, ...place } = HIRE.place;
    return { ...HIRE, place: { ...place, agrees_roads: true } };
})();

export const LAST_WORDS = "Two of them";

/** What a field read from the wrong place looks like in the words the model is given. */
export const NOT_WORDS = /undefined|NaN|\[object /;

export function submitted(
    task: string | undefined,
    input: Submit["input"] = { type: "line", text: "Go." },
): Submit {
    return { id: "input-1", ...(task === undefined ? {} : { task }), input };
}

export async function turn(
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

export interface Written {
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

export function ownForms(
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

export const CHIEF: ChatRef = { with: 7 };

export const COMMANDED: Answers = {
    realm: {
        lapse: () => answer("realm/lapse"),
        open_task: () => answer("realm/open_task.opened"),
    },
};

export const KEY = "sk-test";

export const FAILED_WORDS = "the model answered 529";

export const STOPPING: Answers = {
    workspace: { cancel: () => answer("workspace/cancel.cancelled") },
};

export const REWINDING: Answers = { ...STOPPING, ...COMMANDED };

export function uuid(n: number): string {
    return `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
}

/** A chief's line, then its rewind. */
export async function rewound(t: TestContext, calls: Calls): Promise<Woken> {
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

export async function say(agent: Woken, text: string): Promise<void> {
    await agent.actions.submit(
        agent.by(from.realm),
        submitted(undefined, { type: "line", text }),
    );
    await agent.idle();
}

export function chief(
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

export function newSession(agent: Woken) {
    return agent.actions.new_session(agent.by(from.client), {
        id: uuid(2),
        to: 7,
    });
}

export function retry(agent: Woken) {
    return agent.actions.retry(agent.by(from.client), { id: uuid(6), to: 7 });
}

export async function failed(
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

export async function stopped(t: TestContext, calls: Calls): Promise<Woken> {
    const agent = await wakeAgent(t, { calls, input: HAND });
    assert.deepEqual(
        await agent.actions.stop(agent.by(from.realm), { number: 1 }),
        { turns: 0 },
    );
    return agent;
}

/** A chief's line whose turn is still running when the agent stops. */
export async function working(t: TestContext, calls: Calls): Promise<Woken> {
    const agent = await wakeAgent(t, {
        calls,
        input: HAND,
        responses: [() => new Promise<never>(() => {})],
    });
    await agent.actions.submit(agent.by(from.realm), submitted(undefined));
    return agent;
}

/** The realm's login in the fixtures, which expired long ago. */
export const LOGIN = (answer("realm/credential.oauth") as { login: OauthLogin })
    .login;

/** The settings of the key in the fixtures with settings. */
export const SETTINGS = (
    answer("realm/credential.key.settings") as { env: ProviderEnv }
).env;

/**
 * A stand-in for the provider the credential fixtures name, so no request
 * leaves the host. Its key sign-in gives `settings` with the key.
 */
export function anthropic(
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

export async function onAnthropic(
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

export async function signedIn(
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
export function answering<T>(...answers: T[]): () => T {
    return () => {
        const next = answers.shift();
        assert.ok(next !== undefined, "asked more than was answered");
        return next;
    };
}

export function field(
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

export function actionOf(name: string): { to: string; action: string } {
    const [folder = "", rest = ""] = name.split("/");
    const to = ADDRESSES[folder] ?? REALM;
    return { to: `${to.kind}/${to.key}`, action: rest.split(".")[0] ?? "" };
}

/** Each action the agent takes, by whom it is sent and the fixture of its answer. */
export const TAKERS = {
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
    stop: { by: from.realm, answer: "agent/stop.answer" },
    retire: { by: from.realm, answer: "agent/retire.answer" },
    erase: { by: from.realm, answer: "agent/erase.answer" },
    take_inbox: { by: from.realm, answer: "agent/take_inbox.answer" },
    drop_inbox: { by: from.realm, answer: "agent/drop_inbox.answer" },
} as const;

export const TAKEN = [
    "agent/drop_inbox",
    "agent/erase",
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
    "agent/retire",
    "agent/stop",
    "agent/submit.answer",
    "agent/submit.letter",
    "agent/submit.letter.files",
    "agent/submit.line",
    "agent/submit.line.attached",
    "agent/submit.line.reviewed",
    "agent/submit.notice",
    "agent/submit.notice.aborted",
    "agent/submit.notice.blocked",
    "agent/submit.notice.withdrawn",
    "agent/submit.question",
    "agent/submit.question.files",
    "agent/submit.worker_answer",
    "agent/take_inbox",
] as const;

/** The report a taken call makes: a file held whole is not fetched past the workspace's 256 MiB. */
export const REPORTED: Partial<Record<(typeof TAKEN)[number], RegExp>> = {
    "agent/submit.letter.files":
        /^from-hire-12\/figures\/plot\.png was not saved in the inbox: it holds more than 256\.0 MiB$/,
};

export const CREATED = [
    "agent/dream",
    "agent/errand",
    "agent/hand",
    "agent/hire",
    "agent/worker",
] as const;

export const ANSWERED_BY_TOOLS = [
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
    "realm/merge_hires",
    "realm/remove_hamlet",
    "realm/create_profile",
    "realm/read_profile",
    "realm/edit_profile",
    "realm/tag",
    "realm/revert",
    "realm/ask_librarian",
    "realm/submit_skill",
] as const;

export function answersTo(action: string): string[] {
    return fixtures(new RegExp(`^${action}(\\..+)?\\.answer$`));
}

/**
 * pi-durable records a throw of a tool's `execute` as a result with no
 * output and a `tool_error` diagnostic.
 */
export function threw(result: ToolResult): boolean {
    return result.diagnostics.some(({ code }) => code === "tool_error");
}

/** A tool whose call has no fixture this agent writes. */
export const CALLED: Readonly<Record<string, readonly [string, ToolArgs]>> = {
    "realm/set_model": [
        "set_model",
        { hire: "hire#12", model: "anthropic::claude-opus" },
    ],
};

const NOT_THE_AGENTS: readonly (readonly [RegExp, string])[] = [
    [
        /^realm\/(edit_profile\.done|tag\.tagged|revert\.(reverted|unchanged)|remove_hamlet\.done)\.answer$/,
        "the realm keeps an agent's edit, tag, revert and hamlet removal for a chief: the tool's answer is always a refusal, and what the chief rules arrives as a notice",
    ],
    [/^realm\/chief_/, "a chief's act, sent by a client"],
    [
        /^(realm\/(read_state\.(args|answer)|world\.answer)|library\/shelved\.answer)$/,
        "the client's read of the world, and the realm's and the library's events",
    ],
    [
        /^realm\/(say|answer|read_world|read_charter|read_catalogue|read_line|stop_agents|remember_secret)\./,
        "a client's call of the realm",
    ],
    [
        /^realm\/lapse\.all\.args$/,
        "the lapse of every wait an agent raised, which the realm's stop makes itself",
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
        "a stop's lapse of one conversation: `/stop` lapses every wait the agent raised",
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
        /^profile\/(branch|tag|pin|revert|purge|edit|prepare_dream|end_dream|versions|papers|read|erase|drop_save_parts|merge_branch|bundle|unmerge|drop_branch)\./,
        "the realm's call of a profile",
    ],
    [
        /^profile\/(character|fork)\.input$/,
        "the input a profile is created with",
    ],
    [
        /^stored\/(end\.change\.changed|end\.change\.changed\.diff|end\.change\.failed|end\.change\.not_run|end\.change_not_run|end\.change_not_run\.chief_refused|end\.change_not_run\.chief_refused\.no_reason|end\.change_not_run\.lapsed|end\.change_not_run\.lapsed\.stopped|end\.change_not_run\.reach_needs|end\.change_not_run\.reach_refuses\.agent_filesystem|end\.change_not_run\.reach_refuses\.no_reach|end\.change_not_run\.reach_refuses\.outside_reach|end\.change_not_run\.reach_refuses\.project_in_realm|end\.command|end\.command\.cancelled|end\.command\.cut|end\.command\.cut_off|end\.command\.failed|end\.command\.timed_out|end\.run_not_run|end\.run_not_run\.chief_refused|end\.run_not_run\.chief_refused\.no_reason|end\.run_not_run\.lapsed|end\.run_not_run\.lapsed\.stopped|end\.run_not_run\.reach_needs|end\.run_not_run\.reach_refuses\.no_reach|end\.run_not_run\.reach_refuses\.project_in_realm|end\.run_not_run\.tool_arguments|end\.run_not_run\.tool_not_found|hashed\.absent|hashed\.sha256|needs|needs\.edit|reach\.agent|reach\.agent\.auto|reach\.dream|reach\.ended|reach\.worker|reach\.worker\.started|request\.edit_file|request\.run|request\.run\.tool|request\.write_file)$/,
        "a row the workspace keeps in its own file",
    ],
    [
        /^stored\/(profile\.edit\.done|profile\.edit\.refused\.character_too_long|profile\.edit\.refused\.not_a_path|profile\.edit\.refused\.withheld|profile\.end_dream\.dreamt|profile\.end_dream\.dreamt\.main_unchanged|profile\.end_dream\.nothing|profile\.erase\.done|profile\.erase\.refused\.dreaming|profile\.input\.character|profile\.input\.fork|profile\.merge_branch\.kept|profile\.merge_branch\.merged|profile\.merge_branch\.refused\.commit_gone|profile\.merge_branch\.refused\.not_a_path|profile\.merge_branch\.refused\.part_too_large|profile\.merge_branch\.refused\.withheld|profile\.pin\.follows_main|profile\.pin\.pinned|profile\.pin\.refused\.no_hire|profile\.pin\.refused\.no_version|profile\.purge\.purged|profile\.purge\.refused\.dreaming|profile\.purge\.refused\.not_a_path|profile\.revert\.refused\.no_commit|profile\.revert\.refused\.no_version|profile\.revert\.refused\.not_on_main|profile\.revert\.reverted|profile\.revert\.unchanged|profile\.save\.kept|profile\.save\.refused\.commit_gone|profile\.save\.refused\.not_a_path|profile\.save\.refused\.part_too_large|profile\.save\.refused\.withheld|profile\.save\.saved|profile\.tag\.refused\.bad_name|profile\.tag\.refused\.no_such_commit|profile\.tag\.refused\.no_version|profile\.tag\.refused\.taken|profile\.tag\.tagged|profile\.unmerge\.refused\.commit_gone|profile\.unmerge\.unmerged)$/,
        "a row a profile keeps in its own file",
    ],
    [
        /^stored\/(library\.offer\.kept|library\.offer\.more|library\.offer\.refused\.already_shelved|library\.offer\.refused\.being_judged|library\.offer\.refused\.no_front_matter|library\.offer\.refused\.not_a_path|library\.offer\.refused\.not_a_skill_name|library\.offer\.refused\.part_too_large|library\.offer\.refused\.skill_too_large|library\.unshelve\.done|library\.unshelve\.refused\.no_such_skill|library\.verdict\.done|library\.verdict\.refused\.judged|library\.verdict\.refused\.no_such_offer)$/,
        "a row the library keeps in its own file",
    ],
    [
        /^realm\/lapse\.ended\.args$/,
        "a /new, which leaves the waits of the session it left",
    ],
    [
        /^realm\/read_budget\.args$/,
        "a budget read for a task's payer: the agent reads its own",
    ],
    [/^realm\/set_model\.args$/, "a model change with an effort and a speed"],
    [/^workspace\/parcel(_part)?\./, "the realm's call of the workspace"],
];

export function listed(name: string): boolean {
    return NOT_THE_AGENTS.some(([pattern]) => pattern.test(name));
}

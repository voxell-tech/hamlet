import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type Context,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai";
import type { SocietyActs, SocietyTool } from "../src/agent/messages.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    interrupted,
    named,
    timedOut,
    toolCall,
    toolResults,
    until,
    type Woken,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

const line = (id: string, text: string, task?: string) => ({
    id,
    ...(task === undefined ? {} : { task }),
    input: { type: "line" as const, text },
});

async function schemas(
    agent: Woken,
): Promise<{ name: string; description: string; parameters: unknown }[]> {
    const harness = await agent.harness();
    const root = await harness.root(context);
    return (await root.agent(context)).tools.map((tool) => ({
        name: tool.name,
        description: tool.description,
        parameters: JSON.parse(JSON.stringify(tool.parameters)),
    }));
}

async function offered(agent: Woken): Promise<string[]> {
    return (await schemas(agent)).map((tool) => tool.name);
}

function acts(sent: Parameters<typeof named>[0], name: string): unknown[] {
    return named(sent, name).map(
        (call) => (call.message as { act: unknown }).act,
    );
}

const ADDRESS =
    "An agent's address: `hand`, `mayor`, `mayor@<hamlet id>`, `hire#<id>` or `worker#<id>`.";
const REASON = {
    type: "string",
    description: "Why, in a sentence, if it helps; the record keeps it.",
};
const PATHS = {
    type: "array",
    items: { type: "string" },
    description:
        "Files or folders to send with it, in your lodge or your hamlet's folder. They are copied into the receiver's inbox, and the message says where. Links are not followed. A chief approves a send of more than 32 MiB or 1000 files.",
};
const address = { type: "string", description: ADDRESS };
const text = (description: string) => ({ type: "string", description });
const PROFILE_ADDRESS = text("The scholar's address, `scholar#<id>`.");

/** Today's society tools as `crates/hamlet_server/src/calls/society.rs`'s `specs` offers them, typed out by hand. */
const TODAY = [
    {
        name: "tell",
        description:
            "Send a letter to an agent you have a road to. It is queued for them; you do not wait for an answer.",
        parameters: {
            type: "object",
            properties: {
                to: address,
                text: text("The letter."),
                paths: PATHS,
            },
            required: ["to", "text"],
        },
    },
    {
        name: "ask",
        description:
            "Ask an agent you have a road to a question and wait for its answer. Ask only what you need; a question to someone waiting on you is refused.",
        parameters: {
            type: "object",
            properties: {
                to: address,
                question: text("The question."),
                paths: PATHS,
            },
            required: ["to", "question"],
        },
    },
    {
        name: "request_road",
        description:
            "Ask your lead for a road to an agent of your hamlet you need to reach.",
        parameters: {
            type: "object",
            properties: { to: address, reason: REASON },
            required: ["to"],
        },
    },
    {
        name: "found_hamlet",
        description:
            "Found a hamlet with a scholar as its mayor, or a blank agent when `scholar` is left out.",
        parameters: {
            type: "object",
            properties: {
                name: text(
                    "The name it is shown by: one line, up to 40 characters, no colon. Its id is made for it.",
                ),
                scholar: text(
                    "The address, `scholar#<id>`, of the scholar its mayor is a hire of.",
                ),
                purpose: text("What the hamlet is for."),
            },
            required: ["name", "purpose"],
        },
    },
    {
        name: "seat_mayor",
        description: "Seat a hire as its hamlet's mayor.",
        parameters: {
            type: "object",
            properties: { hire: address, reason: REASON },
            required: ["hire"],
        },
    },
    {
        name: "hire",
        description:
            "Hire a scholar into your hamlet under you, or under a hire of your tree. Leave out `scholar` to hire a blank agent instead, which starts from the realm's character and the library's skills and is not a scholar of the city.",
        parameters: {
            type: "object",
            properties: {
                scholar: text(
                    "The scholar's address, `scholar#<id>`, with `@<version>` to pin a version.",
                ),
                name: text("A blank agent's name, in place of `scholar`."),
                purpose: text("What the hire is for."),
                under: text("The hire it answers to; you when left out."),
                hamlet: text("The hamlet, for the Hand."),
            },
            required: ["purpose"],
        },
    },
    {
        name: "end_hire",
        description: "End a hire under you. Its work stays in its scholar.",
        parameters: {
            type: "object",
            properties: { hire: address, reason: REASON },
            required: ["hire"],
        },
    },
    {
        name: "lay_road",
        description:
            "Lay a road between two hires under you, so they can write and ask each other.",
        parameters: {
            type: "object",
            properties: { a: address, b: address, reason: REASON },
            required: ["a", "b"],
        },
    },
    {
        name: "take_up_road",
        description: "Take up a road you laid between two hires under you.",
        parameters: {
            type: "object",
            properties: { a: address, b: address, reason: REASON },
            required: ["a", "b"],
        },
    },
    {
        name: "set_tools",
        description: "Turn built-in tools off or back on for a hire under you.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                off: { type: "array", items: { type: "string" } },
                on: { type: "array", items: { type: "string" } },
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "submit_skill",
        description:
            "Offer a skill from your skills/ folder to the library, where every agent can read it. The librarian judges it.",
        parameters: {
            type: "object",
            properties: {
                skill: text("The skill's folder name under skills/."),
                reason: REASON,
            },
            required: ["skill"],
        },
    },
    {
        name: "ask_librarian",
        description:
            "Ask the librarian which skills in the library help with something.",
        parameters: {
            type: "object",
            properties: { question: text("What you need a skill for.") },
            required: ["question"],
        },
    },
    {
        name: "create_scholar",
        description:
            "Make a new scholar of the city, to hire next. Without a character it starts from the realm's own. A scholar you make may wait at the harbor until it is let into the city; its hires work all the same.",
        parameters: {
            type: "object",
            properties: {
                name: text(
                    "A short name, one line with no colon. It is a label and may repeat.",
                ),
                character: text("Who the scholar is and how it works."),
            },
            required: ["name"],
        },
    },
    {
        name: "read_scholar",
        description:
            "List a scholar's files, or read one, at its `main` or a version.",
        parameters: {
            type: "object",
            properties: {
                scholar: PROFILE_ADDRESS,
                path: text("A file to read."),
                version: text("A version or commit."),
            },
            required: ["scholar"],
        },
    },
    {
        name: "edit_scholar",
        description:
            "Write a file of a scholar's `main`; a chief approves it first.",
        parameters: {
            type: "object",
            properties: {
                scholar: PROFILE_ADDRESS,
                path: text("The file."),
                content: text("Its new text; empty removes it."),
                message: text("What the change does."),
            },
            required: ["scholar", "path", "content", "message"],
        },
    },
    {
        name: "tag",
        description:
            "Name the scholar's `main` as a version; a chief approves it first.",
        parameters: {
            type: "object",
            properties: {
                scholar: PROFILE_ADDRESS,
                tag: text("The version's name."),
                note: text("What it holds."),
            },
            required: ["scholar", "tag"],
        },
    },
    {
        name: "revert",
        description:
            "Undo a commit of a scholar's `main`; a chief approves it first.",
        parameters: {
            type: "object",
            properties: {
                scholar: PROFILE_ADDRESS,
                commit: text("The commit or version to undo."),
                reason: REASON,
            },
            required: ["scholar", "commit"],
        },
    },
];

/**
 * `npm run check` fails here when an act gains a field, so a parameter that
 * `DROPPED` lists is mapped to it.
 */
const ACT_FIELDS = {
    found: ["name", "mayor", "tier", "purpose"],
    hire: ["hamlet", "under", "profile", "version", "purpose"],
    end: ["hire", "why"],
    seat: ["hire"],
    tell: ["to", "text"],
    ask: ["to", "text", "conversation"],
    lay_road: ["a", "b"],
    take_up_road: ["a", "b"],
    request_road: ["to", "why"],
    set_tools: ["hire", "off", "on"],
    create_profile: ["name", "character"],
    read_profile: ["profile", "at", "path"],
    edit_profile: ["profile", "path", "text", "message"],
    tag: ["profile", "version", "note"],
    revert: ["profile", "what", "reason"],
    ask_librarian: ["question"],
    submit_skill: ["offer", "skill", "reason"],
} as const satisfies {
    readonly [N in SocietyTool]: readonly (keyof SocietyActs[N]["act"])[];
};
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
const _everyField: {
    readonly [N in SocietyTool]: Same<
        keyof SocietyActs[N]["act"],
        (typeof ACT_FIELDS)[N][number]
    >;
} = {
    found: true,
    hire: true,
    end: true,
    seat: true,
    tell: true,
    ask: true,
    lay_road: true,
    take_up_road: true,
    request_road: true,
    set_tools: true,
    create_profile: true,
    read_profile: true,
    edit_profile: true,
    tag: true,
    revert: true,
    ask_librarian: true,
    submit_skill: true,
};

const DROPPED: Record<string, readonly string[]> = {
    tell: ["paths"],
    ask: ["paths"],
    hire: ["name"],
    seat_mayor: ["reason"],
    lay_road: ["reason"],
    take_up_road: ["reason"],
    set_tools: ["reason"],
};

test("tell carries t<task id>, the turn's task and its conversation, and a refusal's words are the tool's result", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            tell: (message) =>
                message.act.to.type === "mayor"
                    ? { type: "done" }
                    : {
                          type: "refused",
                          why: { type: "no_road" },
                          words: "you have no road to `hire#12`; ask your lead for one with request_road",
                      },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("tell", { to: "mayor", text: "The tests pass" }, "c1"),
            toolCall("tell", { to: "hire#12", text: "The tests pass" }, "c2"),
            toolCall("tell", { to: "nobody", text: "The tests pass" }, "c3"),
            fauxAssistantMessage("Told."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Tell them.", "task-1"),
    );
    await agent.idle();

    const tells = named(sent, "tell");
    // An address that names no one is refused here, and nothing is sent for it.
    assert.equal(tells.length, 2);
    const [first] = tells;
    assert.ok(first);
    const { call, conversation } = first.message as {
        call: string;
        conversation: string;
    };
    assert.match(call, /^t[1-9][0-9]*$/);
    assert.match(conversation, /^[1-9][0-9]*$/);
    assert.deepEqual(first, {
        to: "realm/local",
        name: "tell",
        message: {
            call,
            task: "task-1",
            conversation,
            act: { to: { type: "mayor" }, text: "The tests pass" },
        },
        deadlineMs: 10_000,
    });
    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "pi.tool-result")
            .map((entry) => entry.text),
        [
            "Sent.",
            "you have no road to `hire#12`; ask your lead for one with request_road",
            "`nobody` is not an address",
        ],
    );
    await agent.stop();
});

/**
 * How long the first `tell` waits for the second to reach the realm before
 * the test reads them as run in turn. A passing run waits on the second
 * itself, about 70 ms.
 */
const OVERLAP_MS = 5_000;

test("two society tools in one answer run at once, each answered under its own call id, and the next request holds both results", async (t) => {
    const second = Promise.withResolvers<void>();
    let met: string | undefined;
    const { calls, sent } = fakeCalls({
        realm: {
            tell: async (message) => {
                if (message.act.to.type !== "mayor") {
                    second.resolve();
                    return {
                        type: "refused",
                        why: { type: "no_road", to: 12 },
                        words: "you have no road to `hire#12`; ask your lead for one with request_road",
                    };
                }
                met = await within(
                    second.promise.then(() => "met"),
                    OVERLAP_MS,
                    "alone",
                );
                return { type: "done" };
            },
        },
    });
    const requested: Context[] = [];
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage(
                [
                    fauxToolCall(
                        "tell",
                        { to: "mayor", text: "The tests pass" },
                        { id: "c1" },
                    ),
                    fauxToolCall(
                        "tell",
                        { to: "hire#12", text: "The tests pass" },
                        { id: "c2" },
                    ),
                ],
                { stopReason: "toolUse" },
            ),
            (context) => {
                requested.push(context);
                return fauxAssistantMessage("Told.");
            },
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Tell them.", "task-1"),
    );
    await agent.idle();
    await agent.stop();

    assert.equal(met, "met");
    const tells = named(sent, "tell").map(
        (call) =>
            call.message as {
                call: string;
                task: string;
                conversation: string;
                act: { to: unknown };
            },
    );
    assert.deepEqual(
        tells
            .map(({ task, act }) => ({ task, to: act.to }))
            .sort((a, b) =>
                JSON.stringify(a.to).localeCompare(JSON.stringify(b.to)),
            ),
        [
            { task: "task-1", to: { type: "agent", agent: 12 } },
            { task: "task-1", to: { type: "mayor" } },
        ],
    );
    const [first, other] = tells;
    assert.ok(first && other);
    assert.notEqual(first.call, other.call);
    assert.equal(first.conversation, other.conversation);
    assert.match(first.conversation, /^[1-9][0-9]*$/);

    assert.equal(requested.length, 1);
    assert.deepEqual(
        requested[0]?.messages.flatMap((message) =>
            message.role === "toolResult"
                ? [
                      {
                          id: message.toolCallId,
                          text: message.content
                              .map((part) =>
                                  part.type === "text" ? part.text : "",
                              )
                              .join(""),
                      },
                  ]
                : [],
        ),
        [
            { id: "c1", text: "Sent." },
            {
                id: "c2",
                text: "you have no road to `hire#12`; ask your lead for one with request_road",
            },
        ],
    );
});

test("ask carries its conversation and no task when its input named none, and the turn on a question replies with its answer", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            ask: () => ({ type: "done" }),
            reply: () => ({ type: "done" }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("ask", { to: "hand", question: "Which tests fail?" }),
            fauxAssistantMessage("Two of them."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: {
            type: "question",
            letter: 5,
            from: { agent: 12, name: "Ada" },
            text: "How many fail?",
        },
    });
    await agent.idle();

    const [ask] = named(sent, "ask");
    assert.ok(ask);
    const act = (ask.message as { act: { conversation: string } }).act;
    // The question named no task, so the act carries none.
    assert.deepEqual(act, {
        to: { type: "hand" },
        text: "Which tests fail?",
        conversation: act.conversation,
    });
    assert.match(act.conversation, /^[1-9][0-9]*$/);
    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [{ question: 5, text: "Two of them." }],
    );
    await agent.stop();
});

test("each society tool is offered under today's name, and set_tools sends a society tool by its act and any other name as given", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            found: () => ({ type: "founded", hamlet: "h1", mayor: 21 }),
            set_tools: () => ({ type: "done" }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("found_hamlet", { name: "Mill", purpose: "Grind" }, "c1"),
            toolCall(
                "set_tools",
                { hire: "hire#21", off: ["found_hamlet", "tell", "bash"] },
                "c2",
            ),
            toolCall("set_tools", { hire: "hire#21", on: ["shell"] }, "c3"),
            fauxAssistantMessage("Done."),
        ],
    });
    const names = [
        "tell",
        "ask",
        "request_road",
        "found_hamlet",
        "seat_mayor",
        "hire",
        "end_hire",
        "lay_road",
        "take_up_road",
        "set_tools",
    ];
    assert.deepEqual(
        (await offered(agent)).filter((name) => names.includes(name)),
        names,
    );
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Set it up."),
    );
    await agent.idle();

    // A hamlet an agent founds has today's default tier, 50
    // (`hamlet_store/src/schema.sql`).
    assert.deepEqual(acts(sent, "found"), [
        { name: "Mill", tier: "fifty", purpose: "Grind" },
    ]);
    // As today (`society/hires.rs`, `set_tools`), no name is refused.
    assert.deepEqual(acts(sent, "set_tools"), [
        {
            hire: { type: "agent", agent: 21 },
            off: [{ name: "found" }, { name: "tell" }, { name: "bash" }],
        },
        { hire: { type: "agent", agent: 21 }, on: [{ name: "shell" }] },
    ]);
    await agent.stop();
});

test("a hire's place turns its built-in and hand tools off and back on, never a society tool, and an older one changes nothing", async (t) => {
    const { calls } = fakeCalls({
        workspace: {
            list_tools: () => ({
                tools: [
                    {
                        name: "deploy",
                        description: "Runs deploy.",
                        parameters: JSON.stringify({
                            type: "object",
                            properties: {},
                        }),
                    },
                ],
                faults: [],
            }),
        },
    });
    const place = (version: number, off: string[]) => ({
        version,
        lead: 1,
        hires: [],
        roads: [],
        tools_off: off.map((name) => ({ name })),
    });
    const first = await wakeAgent(t, {
        calls,
        input: {
            kind: "hire",
            task: "standing-7",
            place: place(1, []),
            copies: {},
        },
    });
    await until(async () => (await offered(first)).includes("deploy"));
    const all = await offered(first);
    const without = all.filter((name) => name !== "bash" && name !== "deploy");

    assert.equal(
        await first.actions.place(
            first.by(from.realm),
            place(3, ["bash", "deploy", "tell", "found", "a_newer_tool"]),
        ),
        null,
    );
    assert.deepEqual(await offered(first), without);

    await first.actions.place(first.by(from.realm), place(2, []));
    assert.deepEqual(await offered(first), without);
    await first.stop();

    const second = await first.again();
    await until(async () => (await schemas(second)).length > 0);
    assert.deepEqual(await offered(second), without);
    await second.actions.place(second.by(from.realm), place(4, []));
    await until(async () => (await offered(second)).includes("deploy"));
    assert.deepEqual((await offered(second)).sort(), [...all].sort());
    await second.stop();
});

test("an input that arrives while the agent is busy waits, then has its own turn, its own task and its own reply", async (t) => {
    const held = Promise.withResolvers<void>();
    const release = Promise.withResolvers<void>();
    let tells = 0;
    const { calls, sent } = fakeCalls({
        realm: {
            tell: async () => {
                tells += 1;
                if (tells === 1) {
                    held.resolve();
                    await release.promise;
                }
                return { type: "done" as const };
            },
            reply: () => ({ type: "done" }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("tell", { to: "mayor", text: "Starting." }, "c1"),
            fauxAssistantMessage("Told."),
            toolCall(
                "tell",
                { to: "mayor", text: "Asked how many fail." },
                "c2",
            ),
            fauxAssistantMessage("Two of them."),
        ],
    });
    // Both are questions from one peer, so they share its chat and the
    // second waits for the first.
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: {
            type: "question",
            letter: 4,
            from: { agent: 12 },
            text: "Start.",
        },
    });
    await held.promise;
    // The agent is in a tool call. The question is admitted at once, and answered `null`.
    assert.equal(
        await agent.actions.submit(agent.by(from.realm), {
            id: "input-2",
            task: "task-2",
            input: {
                type: "question",
                letter: 5,
                from: { agent: 12 },
                text: "How many fail?",
            },
        }),
        null,
    );
    release.resolve();
    await agent.idle();

    assert.deepEqual(
        named(sent, "tell").map(
            (call) => (call.message as { task?: string }).task,
        ),
        ["task-1", "task-2"],
    );
    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [
            { question: 4, text: "Told." },
            { question: 5, text: "Two of them." },
        ],
    );
    await agent.stop();
});

test("each society tool is offered with today's name, description and parameters, word for word", async (t) => {
    const agent = await wakeAgent(t, { calls: fakeCalls({}).calls });
    const names = TODAY.map((tool) => tool.name);
    const byName = (a: { name: string }, b: { name: string }) =>
        a.name.localeCompare(b.name);
    assert.deepEqual(
        (await schemas(agent))
            .filter((tool) => names.includes(tool.name))
            .sort(byName),
        [...TODAY].sort(byName),
    );
    await agent.stop();
});

test("every one of today's parameters leaves as a field of its act, but those this agent does not send", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            tell: () => ({ type: "done" }),
            ask: () => ({ type: "done" }),
            request_road: () => ({ type: "done" }),
            found: () => ({ type: "founded", hamlet: "h1", mayor: 21 }),
            seat: () => ({ type: "done" }),
            hire: () => ({ type: "hired", agent: 22 }),
            end: () => ({ type: "done" }),
            lay_road: () => ({ type: "done" }),
            take_up_road: () => ({ type: "done" }),
            set_tools: () => ({ type: "done" }),
        },
    });
    const every: Record<string, Parameters<typeof toolCall>[1]> = {
        tell: { to: "mayor", text: "Done.", paths: ["notes.md"] },
        ask: { to: "hand", question: "Why?", paths: ["notes.md"] },
        request_road: { to: "hire#4", reason: "We share a file." },
        found_hamlet: { name: "Mill", scholar: "scholar#2", purpose: "Grind" },
        seat_mayor: { hire: "hire#4", reason: "Cy leads now." },
        // `scholar` and `name` are refused together, as today; the scholar
        // form is the scholar address test's.
        hire: {
            name: "Cy",
            purpose: "Dig",
            under: "hire#4",
            hamlet: "h1",
        },
        end_hire: { hire: "hire#4", reason: "Done." },
        lay_road: { a: "hire#4", b: "mayor@h1", reason: "They share a file." },
        take_up_road: { a: "hire#4", b: "mayor@h1", reason: "Done." },
        set_tools: {
            hire: "hire#4",
            off: ["tell"],
            on: ["found_hamlet"],
            reason: "Quiet.",
        },
    };
    const without = (tool: string) =>
        Object.fromEntries(
            Object.entries(every[tool] ?? {}).filter(
                ([key]) => !DROPPED[tool]?.includes(key),
            ),
        );
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            ...Object.entries(every).map(([tool, args], n) =>
                toolCall(tool, args, `all${n}`),
            ),
            ...Object.keys(DROPPED).map((tool, n) =>
                toolCall(tool, without(tool), `kept${n}`),
            ),
            toolCall("request_road", { to: "hire#4" }, "no-reason"),
            fauxAssistantMessage("Done."),
        ],
    });
    for (const [tool, dropped] of Object.entries(DROPPED)) {
        const today =
            TODAY.find((one) => one.name === tool)?.parameters.properties ?? {};
        for (const parameter of dropped)
            assert.ok(
                parameter in today,
                `${tool}.${parameter} is offered today`,
            );
    }
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Do it all.", "task-1"),
    );
    await agent.idle();

    const four = { type: "agent", agent: 4 };
    const [ask] = acts(sent, "ask") as { conversation: string }[];
    assert.deepEqual(
        ["tell", "ask", "request_road"].flatMap((name) =>
            named(sent, name).map(
                (call) => (call.message as { task?: string }).task,
            ),
        ),
        Array(6).fill("task-1"),
        "a letter's call carries the turn's task",
    );
    assert.deepEqual(
        {
            tell: acts(sent, "tell"),
            ask: acts(sent, "ask"),
            request_road: acts(sent, "request_road"),
            found: acts(sent, "found"),
            seat: acts(sent, "seat"),
            hire: acts(sent, "hire"),
            end: acts(sent, "end"),
            lay_road: acts(sent, "lay_road"),
            take_up_road: acts(sent, "take_up_road"),
            set_tools: acts(sent, "set_tools"),
        },
        {
            tell: Array(2).fill({ to: { type: "mayor" }, text: "Done." }),
            ask: Array(2).fill({
                to: { type: "hand" },
                text: "Why?",
                conversation: ask?.conversation,
            }),
            request_road: [{ to: four, why: "We share a file." }, { to: four }],
            found: [
                { name: "Mill", tier: "fifty", purpose: "Grind", mayor: 2 },
            ],
            seat: Array(2).fill({ hire: four }),
            hire: Array(2).fill({
                purpose: "Dig",
                hamlet: "h1",
                under: four,
            }),
            end: [{ hire: four, why: "Done." }],
            lay_road: Array(2).fill({
                a: four,
                b: { type: "mayor_of", hamlet: "h1" },
            }),
            take_up_road: Array(2).fill({
                a: four,
                b: { type: "mayor_of", hamlet: "h1" },
            }),
            set_tools: Array(2).fill({
                hire: four,
                off: [{ name: "tell" }],
                on: [{ name: "found" }],
            }),
        },
    );
    await agent.stop();
});

test("a scholar's address is read as today's server reads it, a pinned version is hired at it, and one it cannot read is refused with its words", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            hire: () => ({ type: "hired", agent: 22 }),
            found: () => ({ type: "founded", hamlet: "h1", mayor: 21 }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("hire", { scholar: " scholar#3 ", purpose: "Dig" }, "c1"),
            toolCall(
                "hire",
                { scholar: "  ", under: "", hamlet: "", purpose: "Dig" },
                "c2",
            ),
            toolCall("hire", { scholar: "Ada", purpose: "Dig" }, "c3"),
            toolCall("hire", { scholar: "scholar#3@v1", purpose: "Dig" }, "c4"),
            toolCall(
                "found_hamlet",
                { name: "Mill", scholar: "hire#3", purpose: "Grind" },
                "c5",
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Hire."));
    await agent.idle();

    // An empty or blank parameter is one left out, as today's `text` trims it.
    assert.deepEqual(acts(sent, "hire"), [
        { purpose: "Dig", profile: 3 },
        { purpose: "Dig" },
        { purpose: "Dig", profile: 3, version: "v1" },
    ]);
    assert.deepEqual(acts(sent, "found"), []);
    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "pi.tool-result")
            .map((entry) => entry.text)
            .slice(2),
        [
            "`Ada` is not an address",
            "Hired `hire#22`. It is still being hired.",
            "`hire#3` is not an address",
        ],
    );
    await agent.stop();
});

test("a society tool whose call fails says it was interrupted, under the tool's name", async (t) => {
    const { calls } = fakeCalls({
        realm: {
            found: () => {
                throw timedOut();
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("found_hamlet", { name: "Mill", purpose: "Grind" }),
            fauxAssistantMessage("I will look first."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Found."));
    await agent.idle();

    assert.deepEqual(await toolResults(agent), [interrupted("found_hamlet")]);
    await agent.stop();
});

test("hire with both `scholar` and `name` is refused in today's words, and nothing is sent", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { hire: () => ({ type: "hired", agent: 22 }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall(
                "hire",
                { scholar: "scholar#3", name: "Ada", purpose: "Dig" },
                "c1",
            ),
            toolCall(
                "hire",
                { scholar: "scholar#3", name: " ", purpose: "Dig" },
                "c2",
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Hire."));
    await agent.idle();

    assert.deepEqual(acts(sent, "hire"), [{ purpose: "Dig", profile: 3 }]);
    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "pi.tool-result")
            .map((entry) => entry.text)[0],
        "Give `scholar` to hire one of the city, or `name` for a blank agent, not both.",
    );
    await agent.stop();
});

async function seatAndHire(
    t: Parameters<typeof wakeAgent>[0],
    hires: string[],
    scholars: string[] = [],
): Promise<{ seat: unknown[]; hire: unknown[]; results: string[] }> {
    const { calls, sent } = fakeCalls({
        realm: {
            seat: () => ({ type: "done" }),
            hire: () => ({ type: "hired", agent: 22 }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            ...hires.map((hire, n) =>
                toolCall("seat_mayor", { hire }, `seat${n}`),
            ),
            ...scholars.map((scholar, n) =>
                toolCall("hire", { scholar, purpose: "Dig" }, `hire${n}`),
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Seat."));
    await agent.idle();
    const results = (await agent.entries())
        .filter((entry) => entry.kind === "pi.tool-result")
        .map((entry) => entry.text);
    await agent.stop();
    return { seat: acts(sent, "seat"), hire: acts(sent, "hire"), results };
}

const seated = (agent: number) => ({ hire: { type: "agent", agent } });

test("an address is trimmed, as today's server trims it", async (t) => {
    const { seat } = await seatAndHire(t, [
        " hire#4 ",
        " mayor ",
        "mayor@ h1 ",
    ]);
    assert.deepEqual(seat, [
        seated(4),
        { hire: { type: "mayor" } },
        { hire: { type: "mayor_of", hamlet: "h1" } },
    ]);
});

test("an address's number is read as today's server reads it, where the act can carry it", async (t) => {
    const read = [
        "hire#007",
        "hire#+3",
        "hire#0",
        "hire#-0",
        "hire#4294967295",
        "worker#3",
    ];
    const refused = [
        "mayor@",
        "hire#",
        "hire#-1",
        "hire#4294967296",
        "hire#0x10",
        "hire#1e3",
        "worker#",
        "helper#3",
    ];
    const { seat, hire, results } = await seatAndHire(
        t,
        [...read, ...refused],
        ["scholar#007", "scholar#0", "scholar#+3", "scholar#-3"],
    );

    assert.deepEqual(seat, [
        seated(7),
        seated(3),
        seated(0),
        seated(0),
        seated(4294967295),
        seated(3),
    ]);
    assert.deepEqual(hire, [
        { purpose: "Dig", profile: 7 },
        { purpose: "Dig", profile: 0 },
        { purpose: "Dig", profile: 3 },
    ]);
    assert.deepEqual(
        results.slice(read.length, read.length + refused.length),
        refused.map((address) => `\`${address}\` is not an address`),
    );
    assert.equal(results.at(-1), "`scholar#-3` is not an address");
});

test("a reply the realm refuses, or one that fails, is reported and leaves the run and the next turn as they were", async (t) => {
    let replies = 0;
    const { calls, sent } = fakeCalls({
        realm: {
            reply: () => {
                replies += 1;
                if (replies > 1) throw timedOut();
                return {
                    type: "refused",
                    why: { type: "no_question" },
                    words: "the question is answered",
                };
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage("Two of them."),
            fauxAssistantMessage("Three of them."),
            fauxAssistantMessage("Done."),
        ],
    });
    const question = (id: string, letter: number) => ({
        id,
        input: {
            type: "question" as const,
            letter,
            from: { agent: 12, name: "Ada" },
            text: "How many fail?",
        },
    });
    await agent.actions.submit(agent.by(from.realm), question("input-1", 5));
    await agent.idle();
    await agent.actions.submit(agent.by(from.realm), question("input-2", 6));
    await agent.idle();
    await agent.actions.submit(agent.by(from.realm), line("input-3", "On."));
    await agent.idle();
    const answers = (await agent.entries())
        .filter((entry) => entry.kind === "pi.assistant")
        .map((entry) => entry.text);
    const harness = await agent.harness();
    const settled = await harness.commit(async (tx) => {
        const conversations = (await tx.scanConversations({}, 100)).items;
        return Promise.all(
            ["input-1", "input-2", "input-3"].map(async (id) => {
                for (const { id: conversation } of conversations) {
                    const found = await tx.submissionByRequest(
                        conversation,
                        id,
                    );
                    if (found !== undefined) return found.status;
                }
                return undefined;
            }),
        );
    }, context);
    await agent.stop();

    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [
            { question: 5, text: "Two of them." },
            { question: 6, text: "Three of them." },
        ],
    );
    // The questions are answered in their peer's chat, the line in the chief's.
    assert.deepEqual(answers, ["Done."]);
    assert.deepEqual(settled, ["done", "done", "done"]);
    assert.deepEqual(agent.reported, [
        "a reply was refused: the question is answered",
        "a reply was not taken: actor.action_timed_out: The action timed out.",
    ]);
});

test("end_hire's reason is trimmed, and a blank one is left out", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { end: () => ({ type: "done" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("end_hire", { hire: "hire#4", reason: " Done. " }, "c1"),
            toolCall("end_hire", { hire: "hire#4", reason: "  " }, "c2"),
            fauxAssistantMessage("Ended."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "End."));
    await agent.idle();
    await agent.stop();

    const four = { type: "agent", agent: 4 };
    assert.deepEqual(acts(sent, "end"), [
        { hire: four, why: "Done." },
        { hire: four },
    ]);
});

const COMMIT = "ab".repeat(20);

/** The text of each tool result of the agent's turns, in order. */
async function resultTexts(agent: Woken): Promise<string[]> {
    return (await agent.entries())
        .filter((entry) => entry.kind === "pi.tool-result")
        .map((entry) => entry.text);
}

test("the profile tools' arguments leave as their acts: a scholar's address as its profile, `main`, a commit or a version as a revision, and an empty path as a listing", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            create_profile: () => ({
                type: "made",
                profile: 4,
                label: "scholar#4 (Ada)",
            }),
            read_profile: () => ({ type: "listed", paths: [] }),
            edit_profile: () => ({ type: "done" }),
            tag: () => ({ type: "tagged", version: "v1", commit: COMMIT }),
            revert: () => ({ type: "unchanged" }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("create_scholar", { name: " Ada ", character: "" }, "c1"),
            toolCall(
                "create_scholar",
                { name: "Ada", character: "You write careful tests." },
                "c2",
            ),
            toolCall(
                "read_scholar",
                { scholar: " scholar#4 ", path: "" },
                "c3",
            ),
            toolCall(
                "read_scholar",
                {
                    scholar: "scholar#4",
                    path: "memory/tests.md",
                    version: "v1",
                },
                "c4",
            ),
            toolCall(
                "read_scholar",
                { scholar: "scholar#4", version: "main" },
                "c5",
            ),
            toolCall(
                "read_scholar",
                { scholar: "scholar#4", version: COMMIT },
                "c6",
            ),
            toolCall(
                "edit_scholar",
                {
                    scholar: "scholar#4",
                    path: " CHARACTER.md ",
                    content: "You write careful tests.",
                    message: "Sharper",
                },
                "c7",
            ),
            toolCall(
                "tag",
                {
                    scholar: "scholar#4",
                    tag: " v1 ",
                    note: "Before the refactor",
                },
                "c8",
            ),
            toolCall(
                "revert",
                {
                    scholar: "scholar#4",
                    commit: COMMIT,
                    reason: "It broke the tests",
                },
                "c9",
            ),
            toolCall("revert", { scholar: "scholar#4", commit: "v1" }, "c10"),
            toolCall("read_scholar", { scholar: "Ada" }, "c11"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Go."));
    await agent.idle();

    assert.deepEqual(
        {
            create_profile: acts(sent, "create_profile"),
            read_profile: acts(sent, "read_profile"),
            edit_profile: acts(sent, "edit_profile"),
            tag: acts(sent, "tag"),
            revert: acts(sent, "revert"),
        },
        {
            create_profile: [
                { name: "Ada" },
                { name: "Ada", character: "You write careful tests." },
            ],
            read_profile: [
                { profile: 4 },
                {
                    profile: 4,
                    at: { type: "version", version: "v1" },
                    path: "memory/tests.md",
                },
                { profile: 4, at: { type: "main" } },
                { profile: 4, at: { type: "commit", commit: COMMIT } },
            ],
            edit_profile: [
                {
                    profile: 4,
                    path: "CHARACTER.md",
                    text: "You write careful tests.",
                    message: "Sharper",
                },
            ],
            tag: [{ profile: 4, version: "v1", note: "Before the refactor" }],
            revert: [
                {
                    profile: 4,
                    what: { type: "commit", commit: COMMIT },
                    reason: "It broke the tests",
                },
                { profile: 4, what: { type: "version", version: "v1" } },
            ],
        },
    );
    assert.equal((await resultTexts(agent)).at(-1), "`Ada` is not an address");
    await agent.stop();
});

test("the profile tools answer in today's words", async (t) => {
    const answers = {
        read_profile: [
            {
                type: "listed",
                paths: [{ path: "memory/a.md" }, { path: "skills/b/SKILL.md" }],
            },
            { type: "listed", paths: [] },
            { type: "text", text: "# Tests\n", cut: false },
            { type: "text", text: "# Tests", cut: true },
        ],
        revert: [
            { type: "reverted", commit: COMMIT, clashes: [] },
            {
                type: "reverted",
                commit: COMMIT,
                clashes: [{ path: "memory/a.md" }, { path: "memory/b.md" }],
            },
        ],
    } as const;
    const { calls } = fakeCalls({
        realm: {
            create_profile: () => ({
                type: "made",
                profile: 4,
                label: "scholar#4 (Ada)",
            }),
            read_profile: () => answers.read_profile[read++] as never,
            edit_profile: () => ({ type: "done" }),
            tag: () => ({ type: "tagged", version: "v1", commit: COMMIT }),
            revert: () => answers.revert[reverted++] as never,
            ask_librarian: () => ({ type: "done" }),
        },
    });
    let read = 0;
    let reverted = 0;
    const addressed = { scholar: " scholar#4 " };
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("create_scholar", { name: "Ada" }, "c1"),
            ...[2, 3, 4, 5].map((n) =>
                toolCall(
                    "read_scholar",
                    { ...addressed, path: n > 3 ? "a" : "" },
                    `c${n}`,
                ),
            ),
            toolCall(
                "edit_scholar",
                {
                    ...addressed,
                    path: "memory/a.md",
                    content: "A",
                    message: "M",
                },
                "c6",
            ),
            toolCall("tag", { ...addressed, tag: "v1" }, "c7"),
            toolCall("revert", { ...addressed, commit: "v1" }, "c8"),
            toolCall("revert", { ...addressed, commit: "v1" }, "c9"),
            toolCall(
                "ask_librarian",
                { question: "How do I lint the docs?" },
                "c10",
            ),
            toolCall("ask_librarian", { question: "  " }, "c11"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Go."));
    await agent.idle();

    assert.deepEqual(await resultTexts(agent), [
        "The scholar scholar#4 (Ada) lives in the city; hire it next.",
        "- memory/a.md\n- skills/b/SKILL.md",
        "scholar#4 has no files.",
        "# Tests\n",
        "# Tests\n[cut here]",
        "Wrote memory/a.md of scholar#4.",
        "Tagged scholar#4's main as v1.",
        "Reverted the commit on scholar#4's main.",
        "Reverted the commit on scholar#4's main; these files changed again since and kept their later lines: memory/a.md, memory/b.md",
        "The librarian is looking.",
        "Say what you need a skill for.",
    ]);
    await agent.stop();
});

test("submit_skill offers its folder's files to the library under the tool's call, then names the kept offer to the realm; a name, a missing folder or the library's refusal is refused in today's words", async (t) => {
    const { calls, sent } = fakeCalls({
        library: {
            offer: (offer) =>
                offer.skill === "taken"
                    ? {
                          type: "refused",
                          why: { type: "already_shelved", skill: "taken" },
                          words: "a skill named `taken` is on the shelves already",
                      }
                    : { type: "kept", offer: 9 },
        },
        realm: { submit_skill: () => ({ type: "done" }) },
    });
    const files = fakeFiles({
        "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n",
        "skills/fixtures/bin/run.sh": "echo run\n",
        "skills/taken/SKILL.md": "---\nname: taken\n---\n",
        "memory/a.md": "Not a skill.",
    });
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            toolCall(
                "submit_skill",
                { skill: " fixtures ", reason: "Every hire writes fixtures" },
                "c1",
            ),
            toolCall("submit_skill", { skill: "../fixtures" }, "c2"),
            toolCall("submit_skill", { skill: "missing" }, "c3"),
            toolCall("submit_skill", { skill: "taken" }, "c4"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Share it.", "task-1"),
    );
    await agent.idle();

    const offers = named(sent, "offer").map((call) => call.message);
    assert.deepEqual(offers[0], {
        call: (offers[0] as { call: string }).call,
        skill: "fixtures",
        reason: "Every hire writes fixtures",
        part: 0,
        last: true,
        files: [
            {
                path: "SKILL.md",
                offset: 0,
                bytes: Buffer.from("---\nname: fixtures\n---\n").toString(
                    "base64",
                ),
                last: true,
            },
            {
                path: "bin/run.sh",
                offset: 0,
                bytes: Buffer.from("echo run\n").toString("base64"),
                last: true,
            },
        ],
    });
    assert.match((offers[0] as { call: string }).call, /^t[1-9][0-9]*$/);
    assert.deepEqual(acts(sent, "submit_skill"), [
        { offer: 9, skill: "fixtures", reason: "Every hire writes fixtures" },
    ]);
    assert.deepEqual(await resultTexts(agent), [
        "The librarian is judging it.",
        "`skill` is its folder under skills/, in letters, digits, hyphens and underscores.",
        "no skill is named `missing`",
        "a skill named `taken` is on the shelves already",
    ]);
    await agent.stop();
});

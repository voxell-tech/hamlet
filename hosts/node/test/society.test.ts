import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type Context,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai";
import type { AgentInput } from "../src/agent/messages/agent.ts";
import type { SocietyActs, SocietyTool } from "../src/agent/messages/realm.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    asked,
    cappedFiles,
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    interrupted,
    named,
    shown,
    timedOut,
    toolCall,
    toolResults,
    until,
    type Woken,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import { context, fixture } from "./testing.ts";

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
        name: "merge_hires",
        description:
            "Merge one hire under you into another: the kept hire takes what the other learned, its roads and its hires.",
        parameters: {
            type: "object",
            properties: { absorbed: address, into: address, reason: REASON },
            required: ["absorbed", "into"],
        },
    },
    {
        name: "remove_hamlet",
        description: "Remove a hamlet; a chief approves it first.",
        parameters: {
            type: "object",
            properties: { hamlet: text("The hamlet's id."), reason: REASON },
            required: ["hamlet"],
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
    {
        name: "ask_chief",
        description:
            "Ask the chief a question you cannot go on without. Waits for the answer, so ask only when no agent and no file can tell you. Use object to argue against an instruction.",
        parameters: {
            type: "object",
            properties: {
                message: { type: "string" },
                questions: {
                    type: "array",
                    minItems: 1,
                    maxItems: 10,
                    items: {
                        type: "object",
                        properties: {
                            question: { type: "string" },
                            description: { type: "string" },
                            options: {
                                type: "array",
                                maxItems: 6,
                                items: {
                                    anyOf: [
                                        { type: "string" },
                                        {
                                            type: "object",
                                            properties: {
                                                title: { type: "string" },
                                                description: { type: "string" },
                                            },
                                            required: ["title"],
                                        },
                                    ],
                                },
                            },
                            text_input: {
                                type: "boolean",
                                description:
                                    "Allow custom words. Defaults to true.",
                            },
                        },
                        required: ["question"],
                    },
                },
            },
            required: ["questions"],
        },
    },
    {
        name: "object",
        description:
            "Object to the chief's instruction you are working on, with your reason. The chief rules: go on, or withdraw it.",
        parameters: {
            type: "object",
            properties: {
                reason: text("Why the instruction should not be done."),
            },
            required: ["reason"],
        },
    },
    {
        name: "start_worker",
        description:
            "Start a worker on a task: a helper with your tools that works in a scratch folder and where you work, then answers once. With `wait`, you wait for its answer; without, the answer comes to you as a reply.",
        parameters: {
            type: "object",
            properties: {
                task: text("What the worker is to do, and what to answer."),
                tools: {
                    type: "array",
                    items: { type: "string" },
                    description:
                        "Your built-in tools it may use; all of them when left out.",
                },
                wait: { type: "boolean", description: "Wait for its answer." },
            },
            required: ["task"],
        },
    },
    {
        name: "read_budget",
        description:
            "Read what you may spend: your hamlet's or the realm's spend this period against its budget, your own, your allowance and your task's cap. A call over any of them is refused.",
        parameters: { type: "object", properties: {}, required: [] },
    },
    {
        name: "set_model",
        description:
            "Choose the model a hire under you runs; leave out `model` for its hamlet's.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                model: text("The model, as `provider::id`."),
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "set_allowance",
        description:
            "Give a hire under you an allowance in dollars for the budget's period, or none.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                dollars: { type: "number" },
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "nickname",
        description:
            "Give a hire under you a short nickname it is shown by in the world; leave out `nickname` for its own name.",
        parameters: {
            type: "object",
            properties: {
                hire: address,
                nickname: text("At most 24 letters."),
                reason: REASON,
            },
            required: ["hire"],
        },
    },
    {
        name: "agree_road",
        description:
            "As a mayor, agree a road between a hire of your hamlet and one of another. It opens once both mayors agree.",
        parameters: {
            type: "object",
            properties: { mine: address, theirs: address, reason: REASON },
            required: ["mine", "theirs"],
        },
    },
    {
        name: "read_record",
        description: "Read a hamlet's record: every decision with its reason.",
        parameters: {
            type: "object",
            properties: { hamlet: text("The hamlet's id.") },
            required: ["hamlet"],
        },
    },
];

/**
 * `npm run check` fails here when an act gains a field, so a parameter that
 * `DROPPED` lists is mapped to it.
 */
const ACT_FIELDS = {
    found: ["name", "mayor", "tier", "purpose"],
    hire: ["hamlet", "under", "profile", "version", "purpose", "name"],
    end: ["hire", "why"],
    seat: ["hire", "why"],
    tell: ["to", "text", "paths"],
    ask: ["to", "text", "conversation", "paths"],
    lay_road: ["a", "b", "why"],
    take_up_road: ["a", "b", "why"],
    request_road: ["to", "why"],
    set_tools: ["hire", "off", "on", "why"],
    ask_chief: ["task", "conversation", "message", "questions"],
    object: ["task", "conversation", "reason"],
    start_worker: ["brief", "tools", "task", "hamlet"],
    set_allowance: ["hire", "micros", "why"],
    read_budget: ["task"],
    agree_road: ["mine", "theirs", "why"],
    set_nickname: ["hire", "name", "why"],
    set_model: ["of", "model", "effort", "fast", "why"],
    read_record: ["hamlet"],
    create_profile: ["name", "character"],
    read_profile: ["profile", "at", "path"],
    edit_profile: ["profile", "path", "text", "message"],
    merge_hires: ["absorbed", "into", "why"],
    remove_hamlet: ["hamlet", "why"],
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
    ask_chief: true,
    object: true,
    start_worker: true,
    set_allowance: true,
    read_budget: true,
    agree_road: true,
    set_nickname: true,
    set_model: true,
    read_record: true,
    create_profile: true,
    read_profile: true,
    edit_profile: true,
    merge_hires: true,
    remove_hamlet: true,
    tag: true,
    revert: true,
    ask_librarian: true,
    submit_skill: true,
};

/** `start_worker`'s act has no field for `wait`: its answer always comes as a reply. */
const DROPPED: Record<string, readonly string[]> = {
    start_worker: ["wait"],
};

const HIRE: AgentInput = {
    ...(fixture("agent/hire.input") as AgentInput),
    copies: {},
};

/** A hire with no lead, a mayor, at `agrees_roads`. */
function mayor(agrees_roads: boolean): AgentInput {
    const { lead: _, ...place } = HIRE.place;
    return { ...HIRE, place: { ...place, agrees_roads } };
}

/** A mayor while another hamlet lives, offered every tool a hire is. */
const MAYOR = mayor(true);

test("a letter's and a question's relative paths start from the project folder, else the agent's own, as the file tools' do", async (t) => {
    const sentPaths: unknown[] = [];
    for (const input of [HIRE, undefined]) {
        const { calls, sent } = fakeCalls({
            realm: {
                tell: () => ({ type: "sent", to: shown(1) }),
                ask: () => ({ type: "waiting", on: shown(1), question: 1 }),
            },
        });
        const paths = ["big.bin", "notes/../plan.md", "/work/notes.md"];
        const agent = await wakeAgent(t, {
            calls,
            ...(input === undefined ? {} : { input }),
            responses: [
                toolCall("tell", { to: "mayor", text: "Here.", paths }, "c1"),
                toolCall(
                    "ask",
                    { to: "mayor", question: "Right?", paths },
                    "c2",
                ),
                fauxAssistantMessage("Sent."),
            ],
        });
        await agent.actions.submit(
            agent.by(from.realm),
            line("input-1", "Send them."),
        );
        await agent.idle();
        await agent.stop();
        sentPaths.push(
            ["tell", "ask"].map((name) =>
                named(sent, name).map(
                    ({ message }) =>
                        (message as { act: { paths?: unknown } }).act.paths,
                ),
            ),
        );
    }

    const at = (folder: string) => [
        { path: `${folder}/big.bin` },
        { path: `${folder}/plan.md` },
        { path: "/work/notes.md" },
    ];
    assert.deepEqual(sentPaths, [
        [[at("/work/orchard")], [at("/work/orchard")]],
        [[at(FAKE_ROOT)], [at(FAKE_ROOT)]],
    ]);
});

test("tell carries t<task id>, the turn's task and its conversation, and a refusal's words are the tool's result", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            tell: (message) =>
                message.act.to.type === "mayor"
                    ? { type: "sent", to: shown(1) }
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
            "Sent to hire#1.",
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
                return { type: "sent", to: shown(1) };
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
            { id: "c1", text: "Sent to hire#1." },
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
            ask: () => ({ type: "waiting", on: shown(1), question: 1 }),
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
            set_tools: () => ({ type: "done", agent: shown(3) }),
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
        "found_hamlet",
        "seat_mayor",
        "hire",
        "end_hire",
        "lay_road",
        "take_up_road",
        "set_tools",
    ];
    assert.deepEqual(
        (await offered(agent)).filter((name) => names.includes(name)).sort(),
        names.sort(),
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
                confinement: { type: "sandbox", name: "bwrap" },
            }),
        },
    });
    const place = (version: number, off: string[]) => ({
        version,
        lead: 1,
        hires: [],
        roads: [],
        tools_off: off.map((name) => ({ name })),
        manual: false,
        agrees_roads: false,
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
                return { type: "sent" as const, to: shown(1) };
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
    const names = TODAY.map((tool) => tool.name);
    const seen = new Map<string, Awaited<ReturnType<typeof schemas>>[number]>();
    for (const input of [undefined, MAYOR]) {
        const agent = await wakeAgent(t, {
            calls: fakeCalls({}).calls,
            ...(input === undefined ? {} : { input }),
        });
        for (const tool of await schemas(agent)) seen.set(tool.name, tool);
        await agent.stop();
    }
    const byName = (a: { name: string }, b: { name: string }) =>
        a.name.localeCompare(b.name);
    assert.deepEqual(
        [...seen.values()]
            .filter((tool) => names.includes(tool.name))
            .sort(byName),
        [...TODAY].sort(byName),
    );
});

/**
 * Today's society tools by kind (`crates/hamlet_server/src/calls/place.rs`):
 * every agent's in a chief's chat, a lead's, and the Hand's own; a hire's
 * road tools.
 */
const EVERYONE = [
    "tell",
    "ask",
    "start_worker",
    "submit_skill",
    "ask_librarian",
    "ask_chief",
    "read_budget",
    "object",
];
const LEADS = [
    "create_scholar",
    "hire",
    "end_hire",
    "merge_hires",
    "set_model",
    "set_tools",
    "set_allowance",
    "nickname",
    "lay_road",
    "take_up_road",
];
const SOCIETY = new Set(TODAY.map((tool) => tool.name));
const OFFERED_TO_THE_HAND = new Set([
    ...EVERYONE,
    ...LEADS,
    "found_hamlet",
    "seat_mayor",
    "remove_hamlet",
    "read_record",
    "read_scholar",
    "edit_scholar",
    "tag",
    "revert",
]);

test("the Hand and a hire are each offered today's society tools for their kind, and a worker none", async (t) => {
    const kinds: [string, AgentInput | undefined, string[]][] = [
        ["hand", undefined, [...OFFERED_TO_THE_HAND]],
        ["hire", HIRE, [...EVERYONE, "request_road", ...LEADS]],
        ["worker", fixture("agent/worker.input") as AgentInput, []],
    ];
    for (const [kind, input, expected] of kinds) {
        const agent = await wakeAgent(t, {
            calls: fakeCalls({}).calls,
            ...(input === undefined ? {} : { input }),
        });
        const society = (await offered(agent)).filter((name) =>
            SOCIETY.has(name),
        );
        await agent.stop();
        assert.deepEqual(society.sort(), [...expected].sort(), kind);
    }
});

test("a mayor is offered agree_road while another hamlet lives, as today, and a new place offers it again", async (t) => {
    const agrees = async (agent: Woken) =>
        (await offered(agent)).includes("agree_road");
    for (const [input, expected] of [
        [HIRE, false],
        [mayor(false), false],
        [MAYOR, true],
    ] as const) {
        const agent = await wakeAgent(t, { calls: fakeCalls({}).calls, input });
        assert.equal(
            await agrees(agent),
            expected,
            JSON.stringify(input.place),
        );
        await agent.stop();
    }

    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        input: mayor(false),
    });
    const { version, ...rest } = mayor(true).place;
    await agent.actions.place(agent.by(from.realm), {
        ...rest,
        version: Number(version) + 1,
    });
    assert.equal(await agrees(agent), true);
    await agent.stop();
});

test("object is offered in a chief's chat and not in a letter's conversation, as today", async (t) => {
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        input: HIRE,
        responses: [asked([], "Read.")],
    });
    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        input: { type: "letter", from: { agent: 12 }, text: "The tests pass" },
    });
    await agent.idle();
    const harness = await agent.harness();
    const letters = await conversationOfChat(agent.by(from.client), {
        with: 7,
        from: 12,
    });
    assert.ok(letters !== undefined, "the letter has a chat");
    const offers = [
        await harness.conversation(letters, context),
        await harness.root(context),
    ];
    const objecting = [];
    for (const conversation of offers) {
        assert.ok(conversation !== undefined);
        const { tools } = await conversation.agent(context);
        objecting.push(tools.some((tool) => tool.name === "object"));
    }
    await agent.stop();

    assert.deepEqual(objecting, [false, true]);
});

function queue<T>(...answers: T[]): () => T {
    return () => answers.shift() as T;
}

test("each society tool's result names the agents it acted on, in today's words", async (t) => {
    const ada = (agent: number) => ({ agent, name: `hire#${agent} (Ada)` });
    const { calls } = fakeCalls({
        realm: {
            tell: () => ({ type: "sent", to: ada(12) }),
            ask: () => ({ type: "waiting", on: ada(12), question: 1 }),
            hire: () => ({ type: "hired", agent: ada(22) }),
            end: () => ({ type: "done", agent: ada(22) }),
            merge_hires: () => ({
                type: "done",
                absorbed: ada(22),
                into: ada(21),
            }),
            lay_road: () => ({ type: "done", a: ada(1), b: ada(2) }),
            take_up_road: () => ({ type: "done", a: ada(1), b: ada(2) }),
            set_tools: () => ({ type: "done", agent: ada(3) }),
            ask_chief: () => ({ type: "done" }),
            object: () => ({ type: "done" }),
            start_worker: () => ({
                type: "started",
                worker: { agent: 15, name: "worker#15" },
            }),
            set_allowance: () => ({ type: "done", agent: ada(4) }),
            read_budget: queue(
                fixture("realm/read_budget.budget.answer") as never,
                fixture("realm/read_budget.budget.month.answer") as never,
                {
                    type: "budget",
                    spend: {
                        payer: { type: "realm" },
                        spent_micros: 100_000,
                        budget_micros: 10_000_000,
                        spent_ms: 1_800_000,
                        budget_ms: 3_600_000,
                    },
                    period: "day",
                    own_micros: 0,
                },
            ),
            set_nickname: () => ({ type: "done", agent: ada(4) }),
            set_model: () => ({ type: "done", agent: ada(4) }),
            read_record: queue(
                fixture("realm/read_record.read.answer") as never,
                { type: "read", lines: [] },
            ),
        },
    });
    const calling: [string, Parameters<typeof toolCall>[1]][] = [
        ["tell", { to: "hire#12", text: "Done." }],
        ["ask", { to: "hire#12", question: "Why?" }],
        ["hire", { purpose: "Dig" }],
        ["end_hire", { hire: "hire#22" }],
        ["merge_hires", { absorbed: "hire#22", into: "hire#21" }],
        ["lay_road", { a: "hire#1", b: "hire#2" }],
        ["take_up_road", { a: "hire#1", b: "hire#2" }],
        ["set_tools", { hire: "hire#3", off: ["tell"] }],
        ["ask_chief", { questions: [{ question: "Go on?" }] }],
        ["object", { reason: "Too soon." }],
        ["start_worker", { task: "Check the links" }],
        ["set_allowance", { hire: "hire#4", dollars: 5 }],
        ["read_budget", {}],
        ["read_budget", {}],
        ["read_budget", {}],
        ["nickname", { hire: "hire#4", nickname: "Spade" }],
        ["nickname", { hire: "hire#4" }],
        ["set_model", { hire: "hire#4", model: "anthropic::claude-opus" }],
        ["set_model", { hire: "hire#4" }],
        ["read_record", { hamlet: "h1" }],
        ["read_record", { hamlet: "h1" }],
    ];
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            ...calling.map(([tool, args], n) => toolCall(tool, args, `w${n}`)),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Do it all.", "task-1"),
    );
    await agent.idle();
    const results = await toolResults(agent);
    await agent.stop();

    assert.deepEqual(
        results.map(({ output, isError }) => [output, isError]),
        [
            "Sent to hire#12 (Ada).",
            "Waiting for hire#12 (Ada)'s answer.",
            "Hired hire#22 (Ada).",
            "Ended hire#22 (Ada).",
            "Merged hire#22 (Ada) into hire#21 (Ada).",
            "Laid a road between hire#1 (Ada) and hire#2 (Ada).",
            "Took up the road between hire#1 (Ada) and hire#2 (Ada).",
            "Changed the tools of hire#3 (Ada).",
            "This call waits for a chief's answer.",
            "The chief is considering an objection.",
            "Started worker#15; its answer comes to you as a reply.",
            "Set the allowance of hire#4 (Ada).",
            "Hamlet today: $0.25 of $5.00\nYour spend this period: $0.04.\nYour allowance, with what everything under you spends: $2.00.",
            "Realm this month: $1.25\nYour spend this period: $0.00.",
            "Realm today: 30m of 1h 00m\nYour spend this period: $0.00.",
            "hire#4 (Ada) is called Spade now.",
            "hire#4 (Ada) goes by its name again.",
            "hire#4 (Ada) runs anthropic::claude-opus now.",
            "hire#4 (Ada) runs its hamlet's model now.",
            "- hire#2 (ada) laid a road from hire#3 to hire#4: to share the tests",
            "Nothing is decided yet.",
        ].map((words) => [words, false]),
    );
});

test("a hire is told of a road agreed by one mayor, and of one both have agreed, by its two ends", async (t) => {
    const { calls } = fakeCalls({
        realm: {
            agree_road: queue(
                fixture("realm/agree_road.waiting.answer") as never,
                fixture("realm/agree_road.laid.answer") as never,
            ),
        },
    });
    const road = { mine: "hire#12", theirs: "hire#31" };
    const agent = await wakeAgent(t, {
        calls,
        input: MAYOR,
        responses: [
            toolCall("agree_road", road, "a1"),
            toolCall("agree_road", road, "a2"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Agree it.", "task-1"),
    );
    await agent.idle();
    const results = await toolResults(agent);
    await agent.stop();

    assert.deepEqual(
        results.map(({ output }) => output),
        [
            "You agree a road between hire#12 (Ada) and hire#31 (Ada); it opens once the other mayor agrees.",
            "The road between hire#12 (Ada) and hire#31 (Ada) is open.",
        ],
    );
});

test("every one of today's parameters leaves as a field of its act, but those this agent does not send", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            tell: () => ({ type: "sent", to: shown(1) }),
            ask: () => ({ type: "waiting", on: shown(1), question: 1 }),
            request_road: () => ({ type: "done" }),
            found: () => ({ type: "founded", hamlet: "h1", mayor: 21 }),
            seat: () => ({ type: "done", agent: shown(22), hamlet: "h" }),
            hire: () => ({ type: "hired", agent: shown(22) }),
            end: () => ({ type: "done", agent: shown(22) }),
            lay_road: () => ({ type: "done", a: shown(1), b: shown(2) }),
            take_up_road: () => ({ type: "done", a: shown(1), b: shown(2) }),
            set_tools: () => ({ type: "done", agent: shown(3) }),
            ask_chief: () => ({ type: "done" }),
            object: () => ({ type: "done" }),
            start_worker: () => ({ type: "started", worker: shown(15) }),
            set_allowance: () => ({ type: "done", agent: shown(4) }),
            read_budget: () => ({
                type: "budget",
                spend: {
                    payer: { type: "realm" },
                    spent_micros: 0,
                    spent_ms: 0,
                },
                period: "day",
                own_micros: 0,
            }),
            agree_road: () => ({
                type: "waiting",
                mine: shown(4),
                theirs: shown(31),
            }),
            set_nickname: () => ({ type: "done", agent: shown(4) }),
            set_model: () => ({ type: "done", agent: shown(4) }),
            read_record: () => ({ type: "read", lines: [] }),
            merge_hires: () => ({
                type: "done",
                absorbed: shown(4),
                into: shown(5),
            }),
            remove_hamlet: () => ({
                type: "refused",
                why: { type: "reserved", wait: 1 },
                words: "This call waits for a chief's answer.",
            }),
        },
    });
    const every: Record<string, Parameters<typeof toolCall>[1]> = {
        tell: { to: "mayor", text: "Done.", paths: ["/work/notes.md"] },
        ask: { to: "hand", question: "Why?", paths: ["/work/notes.md"] },
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
        ask_chief: {
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
        object: { reason: "The field is better left alone" },
        start_worker: { task: "Check the links", tools: ["read"], wait: true },
        set_allowance: { hire: "hire#4", dollars: 5000, reason: "More." },
        read_budget: {},
        agree_road: { mine: "hire#4", theirs: "mayor@h2", reason: "Trade." },
        nickname: { hire: "hire#4", nickname: "Spade", reason: "Short." },
        set_model: {
            hire: "hire#4",
            model: "anthropic::claude-opus",
            reason: "Harder work.",
        },
        read_record: { hamlet: "h1" },
        merge_hires: {
            absorbed: "hire#4",
            into: "hire#5",
            reason: "Same work.",
        },
        remove_hamlet: { hamlet: " h1 ", reason: "Done with it." },
    };
    const without = (tool: string) =>
        Object.fromEntries(
            Object.entries(every[tool] ?? {}).filter(
                ([key]) => !DROPPED[tool]?.includes(key),
            ),
        );
    for (const [tool, dropped] of Object.entries(DROPPED)) {
        const today =
            TODAY.find((one) => one.name === tool)?.parameters.properties ?? {};
        for (const parameter of dropped)
            assert.ok(
                parameter in today,
                `${tool}.${parameter} is offered today`,
            );
    }
    for (const input of [undefined, MAYOR]) {
        const probe = await wakeAgent(t, {
            calls: fakeCalls({}).calls,
            ...(input === undefined ? {} : { input }),
        });
        const mine = new Set(await offered(probe));
        await probe.stop();
        const calling = Object.keys(every).filter(
            (tool) =>
                mine.has(tool) &&
                // The Hand's calls cover the tools both are offered.
                (input === undefined || !OFFERED_TO_THE_HAND.has(tool)),
        );
        const agent = await wakeAgent(t, {
            calls,
            ...(input === undefined ? {} : { input }),
            responses: [
                ...calling.map((tool, n) =>
                    toolCall(tool, every[tool] ?? {}, `all${n}`),
                ),
                ...Object.keys(DROPPED)
                    .filter((tool) => calling.includes(tool))
                    .map((tool, n) =>
                        toolCall(tool, without(tool), `kept${n}`),
                    ),
                ...(calling.includes("request_road")
                    ? [toolCall("request_road", { to: "hire#4" }, "no-reason")]
                    : []),
                fauxAssistantMessage("Done."),
            ],
        });
        await agent.actions.submit(
            agent.by(from.realm),
            line("input-1", "Do it all.", "task-1"),
        );
        await agent.idle();
        await agent.stop();
    }

    const four = { type: "agent", agent: 4 };
    const [ask] = acts(sent, "ask") as { conversation: string }[];
    const [asked] = acts(sent, "ask_chief") as { conversation: string }[];
    const [objected] = acts(sent, "object") as { conversation: string }[];
    assert.deepEqual(
        ["tell", "ask", "request_road"].flatMap((name) =>
            named(sent, name).map(
                (call) => (call.message as { task?: string }).task,
            ),
        ),
        Array(4).fill("task-1"),
        "a letter's call carries the turn's task",
    );
    const why = (text: string) => ({ why: text });
    assert.deepEqual(
        Object.fromEntries(
            [
                "tell",
                "ask",
                "request_road",
                "found",
                "seat",
                "hire",
                "end",
                "lay_road",
                "take_up_road",
                "set_tools",
                "ask_chief",
                "object",
                "start_worker",
                "set_allowance",
                "read_budget",
                "agree_road",
                "set_nickname",
                "set_model",
                "read_record",
                "merge_hires",
                "remove_hamlet",
            ].map((name) => [name, acts(sent, name)]),
        ),
        {
            tell: [
                {
                    to: { type: "mayor" },
                    text: "Done.",
                    paths: [{ path: "/work/notes.md" }],
                },
            ],
            ask: [
                {
                    to: { type: "hand" },
                    text: "Why?",
                    conversation: ask?.conversation,
                    paths: [{ path: "/work/notes.md" }],
                },
            ],
            request_road: [
                { to: four, ...why("We share a file.") },
                { to: four },
            ],
            found: [
                { name: "Mill", tier: "fifty", purpose: "Grind", mayor: 2 },
            ],
            seat: [{ hire: four, ...why("Cy leads now.") }],
            hire: [{ purpose: "Dig", hamlet: "h1", under: four, name: "Cy" }],
            end: [{ hire: four, ...why("Done.") }],
            lay_road: [
                {
                    a: four,
                    b: { type: "mayor_of", hamlet: "h1" },
                    ...why("They share a file."),
                },
            ],
            take_up_road: [
                {
                    a: four,
                    b: { type: "mayor_of", hamlet: "h1" },
                    ...why("Done."),
                },
            ],
            set_tools: [
                {
                    hire: four,
                    off: [{ name: "tell" }],
                    on: [{ name: "found" }],
                    ...why("Quiet."),
                },
            ],
            ask_chief: [
                {
                    task: "task-1",
                    conversation: asked?.conversation,
                    message: "Before planting",
                    questions: [
                        {
                            question: "Which seed?",
                            description: "The field is wet",
                            options: [
                                { title: "Oats" },
                                { title: "Barley", description: "Slower" },
                            ],
                            text_input: false,
                        },
                        { question: "When?", text_input: true },
                    ],
                },
            ],
            object: [
                {
                    task: "task-1",
                    conversation: objected?.conversation,
                    reason: "The field is better left alone",
                },
            ],
            start_worker: Array(2).fill({
                brief: "Check the links",
                tools: [{ name: "read" }],
                task: "task-1",
            }),
            set_allowance: [
                { hire: four, micros: "5000000000", ...why("More.") },
            ],
            read_budget: [{}],
            agree_road: [
                {
                    mine: four,
                    theirs: { type: "mayor_of", hamlet: "h2" },
                    ...why("Trade."),
                },
            ],
            set_nickname: [{ hire: four, name: "Spade", ...why("Short.") }],
            set_model: [
                {
                    of: { type: "hire", hire: four },
                    model: "anthropic::claude-opus",
                    ...why("Harder work."),
                },
            ],
            read_record: [{ hamlet: "h1" }],
            merge_hires: [
                {
                    absorbed: four,
                    into: { type: "agent", agent: 5 },
                    ...why("Same work."),
                },
            ],
            remove_hamlet: [{ hamlet: "h1", ...why("Done with it.") }],
        },
    );
});

test("a scholar's address is read as today's server reads it, a pinned version is hired at it, and one it cannot read is refused with its words", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            hire: () => ({ type: "hired", agent: shown(22) }),
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
            "Hired hire#22.",
            "`hire#3` is not an address",
        ],
    );
    await agent.stop();
});

test("a society tool whose call fails says it was interrupted, under the tool's name, and logs why", async (t) => {
    const logged = t.mock.method(console, "error", () => {});
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
    assert.deepEqual(
        logged.mock.calls.map((call) => call.arguments),
        [
            [
                "found_hamlet was interrupted: actor.action_timed_out: The action timed out.",
            ],
        ],
    );
    await agent.stop();
});

test("hire with both `scholar` and `name` is refused in today's words, and nothing is sent", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { hire: () => ({ type: "hired", agent: shown(22) }) },
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
            seat: () => ({ type: "done", agent: shown(22), hamlet: "h" }),
            hire: () => ({ type: "hired", agent: shown(22) }),
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

test("a seated mayor is named with its hamlet, in today's words", async (t) => {
    const { results } = await seatAndHire(t, ["hire#22"]);
    assert.deepEqual(results, [`${shown(22).name} is the mayor of h now.`]);
});

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
        { hire: { type: "worker", worker: 3 } },
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
        realm: { end: () => ({ type: "done", agent: shown(3) }) },
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

test("the profile tools' arguments leave as their acts: `scholar#<n>` as its profile, `main`, a commit or a version as a revision, and an empty path as a listing", async (t) => {
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
    } as const;
    const { calls } = fakeCalls({
        realm: {
            create_profile: () => ({
                type: "made",
                profile: 4,
                label: "scholar#4 (Ada)",
            }),
            read_profile: () => answers.read_profile[read++] as never,
            ask_librarian: () => ({ type: "done" }),
        },
    });
    let read = 0;
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

test("submit_skill of a folder whose walk stops at its cap is refused, and nothing is offered", async (t) => {
    const { calls, sent } = fakeCalls({
        library: { offer: () => ({ type: "kept", offer: 9 }) },
        realm: { submit_skill: () => ({ type: "done" }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        files: cappedFiles({
            "skills/fixtures/SKILL.md": "---\nname: fixtures\n---\n",
        }),
        responses: [
            toolCall("submit_skill", { skill: "fixtures" }, "c1"),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Share it."),
    );
    await agent.idle();

    assert.deepEqual(named(sent, "offer"), []);
    assert.deepEqual(await resultTexts(agent), [
        "`fixtures` holds more files than the agent can list, 20000, so it was not offered.",
    ]);
    await agent.stop();
});

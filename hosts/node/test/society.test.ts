import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { SocietyActs, SocietyTool } from "../src/agent/messages.ts";
import {
    fakeCalls,
    from,
    interrupted,
    named,
    timedOut,
    toolCall,
    toolResults,
    type Woken,
    wakeAgent,
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
];

/**
 * `npm run check` fails here when an act gains a field, so a parameter that
 * `DROPPED` lists is mapped to it.
 */
const ACT_FIELDS = {
    found: ["name", "mayor", "tier", "purpose"],
    hire: ["hamlet", "under", "profile", "purpose"],
    end: ["hire", "why"],
    seat: ["hire"],
    tell: ["to", "text", "task"],
    ask: ["to", "text", "conversation", "task"],
    lay_road: ["a", "b"],
    take_up_road: ["a", "b"],
    request_road: ["to", "why", "task"],
    set_tools: ["hire", "off", "on"],
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

test("tell carries t<task id> and the turn's task, and a refusal's words are the tool's result", async (t) => {
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
    const call = (first.message as { call: string }).call;
    assert.match(call, /^t[1-9][0-9]*$/);
    assert.deepEqual(first, {
        to: "realm/local",
        name: "tell",
        message: {
            call,
            act: {
                to: { type: "mayor" },
                text: "The tests pass",
                task: "task-1",
            },
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

test("each society tool is offered under today's name, and set_tools reads today's names as acts", async (t) => {
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
                { hire: "hire#21", off: ["found_hamlet", "tell"] },
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
    assert.deepEqual(acts(sent, "set_tools"), [
        {
            hire: { type: "agent", agent: 21 },
            off: [{ name: "found" }, { name: "tell" }],
        },
    ]);
    const results = (await agent.entries()).filter(
        (entry) => entry.kind === "pi.tool-result",
    );
    assert.equal(results.at(-1)?.text, "`shell` is not a society tool.");
    await agent.stop();
});

test("a place turns tools off for the next request, an older one changes nothing, and a new wake keeps it", async (t) => {
    const { calls } = fakeCalls({});
    const first = await wakeAgent(t, { calls });
    const place = (version: number, off: string[]) => ({
        version,
        hires: [],
        roads: [],
        tools_off: off.map((name) => ({ name })),
    });
    const all = await offered(first);
    const without = all.filter(
        (name) => name !== "tell" && name !== "found_hamlet",
    );

    assert.equal(
        await first.actions.place(
            first.by(from.realm),
            place(3, ["tell", "found", "a_newer_tool"]) as never,
        ),
        null,
    );
    assert.deepEqual(await offered(first), without);

    await first.actions.place(first.by(from.realm), place(2, []) as never);
    await first.actions.place(first.by(from.realm), place(3, []) as never);
    assert.deepEqual(await offered(first), without);
    await first.stop();

    const second = await first.again();
    assert.deepEqual(await offered(second), without);
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
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Start.", "task-1"),
    );
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
            (call) => (call.message as { act: { task?: string } }).act.task,
        ),
        ["task-1", "task-2"],
    );
    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [{ question: 5, text: "Two of them." }],
    );
    await agent.stop();
});

test("each society tool is offered with today's name, description and parameters, word for word", async (t) => {
    const agent = await wakeAgent(t, { calls: fakeCalls({}).calls });
    const names = TODAY.map((tool) => tool.name);
    assert.deepEqual(
        (await schemas(agent)).filter((tool) => names.includes(tool.name)),
        TODAY,
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
            tell: Array(2).fill({
                to: { type: "mayor" },
                text: "Done.",
                task: "task-1",
            }),
            ask: Array(2).fill({
                to: { type: "hand" },
                text: "Why?",
                conversation: ask?.conversation,
                task: "task-1",
            }),
            request_road: [
                { to: four, why: "We share a file.", task: "task-1" },
                { to: four, task: "task-1" },
            ],
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

test("a scholar's address is read as today's server reads it, and one it cannot read is refused with its words", async (t) => {
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
            // A pinned version has no field in the act, so it is refused, not hired at `main`.
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
    ]);
    assert.deepEqual(acts(sent, "found"), []);
    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "pi.tool-result")
            .map((entry) => entry.text)
            .slice(2),
        [
            "`Ada` is not an address",
            "`scholar#3@v1` is not an address",
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
    const root = await harness.root(context);
    const settled = await harness.commit(
        async (tx) =>
            Promise.all(
                ["input-1", "input-2", "input-3"].map(
                    async (id) =>
                        (await tx.submissionByRequest(root.id, id))?.status,
                ),
            ),
        context,
    );
    await agent.stop();

    assert.deepEqual(
        named(sent, "reply").map((call) => call.message),
        [
            { question: 5, text: "Two of them." },
            { question: 6, text: "Three of them." },
        ],
    );
    assert.deepEqual(answers, ["Two of them.", "Three of them.", "Done."]);
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

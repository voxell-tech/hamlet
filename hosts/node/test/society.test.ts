import assert from "node:assert/strict";
import { test } from "node:test";
import {
    type Context,
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai";
import {
    FAKE_ROOT,
    fakeCalls,
    from,
    line,
    named,
    shown,
    toolCall,
    until,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import {
    acts,
    HIRE,
    MAYOR,
    OFFERED,
    OVERLAP_MS,
    offered,
    schemas,
} from "./society-testing.ts";

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

test("two society tools in one answer run one after the other, in call order (C16), each answered under its own call id, and the next request holds both results", async (t) => {
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

    assert.equal(met, "alone");
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
        tells.map(({ task, act }) => ({ task, to: act.to })),
        [
            { task: "task-1", to: { type: "mayor" } },
            { task: "task-1", to: { type: "agent", agent: 12 } },
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

test("each society tool is offered under its name, and set_tools sends a society tool by its act and any other name as given", async (t) => {
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

    // A hamlet an agent founds has the default tier, 50.
    assert.deepEqual(acts(sent, "found"), [
        { name: "Mill", tier: "fifty", purpose: "Grind" },
    ]);
    // No name is refused.
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

test("each society tool is offered with its name, description and parameters, word for word", async (t) => {
    const names = OFFERED.map((tool) => tool.name);
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
        [...OFFERED].sort(byName),
    );
});

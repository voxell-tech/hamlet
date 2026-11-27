import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { AgentInput } from "../src/agent/messages/agent.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import {
    asked,
    fakeCalls,
    from,
    interrupted,
    line,
    named,
    shown,
    timedOut,
    toolCall,
    toolResults,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import {
    acts,
    DROPPED,
    EVERYONE,
    HIRE,
    LEADS,
    MAYOR,
    mayor,
    OFFERED,
    OFFERED_TO_THE_HAND,
    offered,
    queue,
    SOCIETY,
} from "./society-testing.ts";
import { context, fixture } from "./testing.ts";

test("the Hand and a hire are each offered the society tools for their kind, and a worker none", async (t) => {
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

test("a mayor is offered agree_road while another hamlet lives, and a new place offers it again", async (t) => {
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

test("object is offered in a chief's chat and not in a letter's conversation", async (t) => {
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

test("each society tool's result names the agents it acted on, in its words", async (t) => {
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
                        spent_tokens: 0,
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

test("every one of a tool's parameters leaves as a field of its act, but those this agent does not send", async (t) => {
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
                    spent_tokens: 0,
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
        found_hamlet: { name: "Mill", profile: "profile#2", purpose: "Grind" },
        seat_mayor: { hire: "hire#4", reason: "Cy leads now." },
        // `profile` and `name` are refused together; the profile form is the
        // profile address test's.
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
        const offered =
            OFFERED.find((one) => one.name === tool)?.parameters.properties ??
            {};
        for (const parameter of dropped)
            assert.ok(parameter in offered, `${tool}.${parameter} is offered`);
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

test("a profile's address is read as an address, a pinned version is hired at it, and one it cannot read is refused with its words", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: {
            hire: () => ({ type: "hired", agent: shown(22) }),
            found: () => ({ type: "founded", hamlet: "h1", mayor: 21 }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("hire", { profile: " profile#3 ", purpose: "Dig" }, "c1"),
            toolCall(
                "hire",
                { profile: "  ", under: "", hamlet: "", purpose: "Dig" },
                "c2",
            ),
            toolCall("hire", { profile: "Ada", purpose: "Dig" }, "c3"),
            toolCall("hire", { profile: "profile#3@v1", purpose: "Dig" }, "c4"),
            toolCall(
                "found_hamlet",
                { name: "Mill", profile: "hire#3", purpose: "Grind" },
                "c5",
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Hire."));
    await agent.idle();

    // An empty or blank parameter is one left out.
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
    assert.deepEqual(agent.reported, [
        "found_hamlet was interrupted: actor.action_timed_out: The action timed out.",
    ]);
    await agent.stop();
});

test("hire with both `profile` and `name` is refused in its words, and nothing is sent", async (t) => {
    const { calls, sent } = fakeCalls({
        realm: { hire: () => ({ type: "hired", agent: shown(22) }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall(
                "hire",
                { profile: "profile#3", name: "Ada", purpose: "Dig" },
                "c1",
            ),
            toolCall(
                "hire",
                { profile: "profile#3", name: " ", purpose: "Dig" },
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
        "Give `profile` to hire one of the city, or `name` for a blank agent, not both.",
    );
    await agent.stop();
});

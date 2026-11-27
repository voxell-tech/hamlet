import assert from "node:assert/strict";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { AgentInput, Copies } from "../src/agent/messages/agent.ts";
import type { Done } from "../src/agent/messages/realm.ts";
import { from, modelFailed, named, until, wakeAgent } from "./agent-testing.ts";
import {
    answer,
    anthropic,
    COMMANDED,
    chief,
    FAILED_WORDS,
    failed,
    HAND,
    KEY,
    LAST_WORDS,
    LOGIN,
    MAYOR,
    newSession,
    onAnthropic,
    REWINDING,
    retry,
    rewound,
    SETTINGS,
    STOPPING,
    say,
    signedIn,
    stopped,
    uuid,
    type Written,
    working,
} from "./messages-testing.ts";
import { TURN_WRITES } from "./messages-written-turns.ts";
import { fixture } from "./testing.ts";

export const WRITTEN: readonly Written[] = [
    ...TURN_WRITES,
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
        agent: MAYOR,
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
        agent: MAYOR,
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
        fixture: "realm/merge_hires",
        tool: [
            "merge_hires",
            {
                absorbed: "hire#12",
                into: "hire#2",
                reason: "Their work overlaps",
            },
        ],
        answers: {
            realm: { merge_hires: () => answer("realm/merge_hires.done") },
        },
        own: ["call", "conversation"],
    },
    {
        fixture: "realm/remove_hamlet",
        tool: [
            "remove_hamlet",
            { hamlet: "5f0d2c1b-7a3e-4d9f-8b6a-1c2d3e4f5a6b" },
        ],
        answers: {
            realm: {
                remove_hamlet: () =>
                    answer("realm/remove_hamlet.refused.reserved"),
            },
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
    { fixture: "realm/read_own_place" },
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
        fixture: "realm/working",
        drive: working,
        answers: { realm: { working: () => answer("realm/working") } },
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

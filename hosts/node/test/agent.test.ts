import assert from "node:assert/strict";
import { test } from "node:test";
import {
    createAssistantMessageEventStream,
    fauxAssistantMessage,
    type Models,
} from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Harness,
    LiveDoc,
} from "@earendil-works/pi-durable";
import { flattenActionHandlers } from "../src/actor/actor/actions.ts";
import { isActorErrorCode } from "../src/actor/actor/mod.ts";
import { agentActor } from "../src/agent/actor.ts";
import type { Turns } from "../src/agent/turn.ts";
import { piRuntime } from "../src/pi/runtime.ts";
import {
    fakeCalls,
    from,
    named,
    toolCall,
    until,
    type Woken,
    wakeAgent,
    within,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

test("an input is taken once, and its entry keeps its task", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            fauxAssistantMessage("Hello."),
            fauxAssistantMessage("never"),
        ],
    });
    const line = {
        id: "input-1",
        task: "task-1",
        input: { type: "line" as const, text: "Write the tests" },
    };

    assert.equal(await agent.actions.submit(agent.by(from.realm), line), null);
    assert.equal(await agent.actions.submit(agent.by(from.realm), line), null);
    await agent.idle();

    assert.equal(agent.faux.state.callCount, 1);
    assert.deepEqual(
        (await agent.entries()).filter((entry) =>
            entry.kind.startsWith("hamlet."),
        ),
        [
            {
                kind: "hamlet.line",
                data: { task: "task-1" },
                text: "Write the tests",
            },
        ],
    );
    await agent.stop();
});

test("a wrong caller is refused, and nothing is taken", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls });
    const wrong = (error: unknown) =>
        isActorErrorCode(error, "message", "wrong_caller");
    const line = {
        id: "input-1",
        input: { type: "line" as const, text: "Hi" },
    };
    const end = {
        call: "t9",
        outcome: {
            type: "run" as const,
            end: {
                type: "not_run" as const,
                why: { type: "chief_refused" },
                words: "no",
            },
        },
    };
    const here = {
        version: 2,
        hires: [],
        roads: [],
        tools_off: [],
        manual: false,
    };

    await assert.rejects(
        agent.actions.submit(agent.by(from.workspace), line),
        wrong,
    );
    await assert.rejects(
        agent.actions.submit(agent.by(from.agent(12)), line),
        wrong,
    );
    await assert.rejects(agent.actions.submit(agent.by("client"), line), wrong);
    await assert.rejects(agent.actions.submit(agent.by(null), line), wrong);
    await assert.rejects(
        agent.actions.finished(agent.by(from.realm), end),
        wrong,
    );
    await assert.rejects(
        agent.actions.place(agent.by(from.workspace), here),
        wrong,
    );
    await assert.rejects(
        agent.actions.fill(agent.by(from.workspace), {
            profile: 2,
        }),
        wrong,
    );
    await assert.rejects(agent.actions.pi.wake(agent.by(from.realm)), wrong);

    assert.deepEqual(await agent.entries(), []);
    await agent.stop();
});

test("a null field is read as unset, an unknown field is ignored, and an unreadable message is refused", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("Hello.")],
    });
    // As a newer realm might write it: `null` for an unset task, and fields this agent does not know.
    const newer = {
        id: "input-1",
        task: null,
        sent_at: 1_700_000_000,
        input: { type: "line", text: "Hi", images: null, colour: "red" },
    };
    assert.equal(
        await agent.actions.submit(agent.by(from.realm), newer as never),
        null,
    );
    await agent.idle();
    // The entry keeps no task, so the standing task pays for the turn.
    assert.deepEqual(
        (await agent.entries()).filter((entry) =>
            entry.kind.startsWith("hamlet."),
        ),
        [{ kind: "hamlet.line", data: {}, text: "Hi" }],
    );
    const place = {
        version: 2,
        lead: null,
        hires: [],
        roads: [],
        tools_off: [{ name: "a_newer_tool" }],
        colour: 1,
    };
    assert.equal(
        await agent.actions.place(agent.by(from.realm), place as never),
        null,
    );

    const unreadable = (error: unknown) =>
        isActorErrorCode(error, "message", "unreadable");
    await assert.rejects(
        agent.actions.submit(agent.by(from.realm), {
            input: { type: "line", text: "No id" },
        } as never),
        unreadable,
    );
    await agent.stop();
});

test("a place keeps only the fields the agent knows, also one a newer realm sends as a bigint", async (t) => {
    const { calls } = fakeCalls({});
    const newer = {
        version: 1,
        hires: [2],
        roads: [],
        tools_off: [],
        future: 2n ** 70n,
    };
    const agent = await wakeAgent(t, {
        calls,
        input: {
            kind: "hand",
            task: "standing",
            place: newer as never,
            copies: {},
        },
    });
    const kept = async () =>
        JSON.parse(
            (await agent.c.db.get<{ place: string }>("SELECT place FROM agent"))
                ?.place ?? "",
        ) as unknown;
    assert.deepEqual(await kept(), {
        hires: [2],
        roads: [],
        tools_off: [],
        manual: false,
    });

    assert.equal(
        await agent.actions.place(agent.by(from.realm), {
            ...newer,
            version: 2,
            lead: 3,
        } as never),
        null,
    );
    assert.deepEqual(await kept(), {
        lead: 3,
        hires: [2],
        roads: [],
        tools_off: [],
        manual: false,
    });
    await agent.stop();
});

test("an answer goes to the conversation it names, and to the root when that is not one", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [fauxAssistantMessage("a"), fauxAssistantMessage("b")],
    });
    const harness = await agent.harness();
    const other = await harness.createConversation(
        { ownership: { kind: "ownerless" } },
        context,
    );
    const answer = (id: string, conversation: string) => ({
        id,
        input: {
            type: "answer" as const,
            question: 5,
            from: { agent: 12, name: "Ada" },
            text: "Two of them",
            conversation,
        },
    });
    await agent.actions.submit(
        agent.by(from.realm),
        answer("input-1", String(other.id)),
    );
    await agent.actions.submit(agent.by(from.realm), answer("input-2", "gone"));
    await agent.idle();

    const answers = async (conversation: ConversationId) =>
        (
            await (
                await harness.conversation(conversation, context)
            )?.entries({ order: "ascending" }, 100, undefined, context)
        )?.items
            .filter((entry) => entry.kind === "hamlet.answer")
            .map((entry) => entry.conversationId);
    assert.deepEqual(await answers(other.id), [other.id]);
    const root = await harness.root(context);
    assert.deepEqual(await answers(root.id), [root.id]);
    await agent.stop();
});

test("the kind's definition passes actor()'s own checks, and its actions are the agent's messages, a client's calls and pi.wake", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls });
    const definition = agentActor(agent.config);
    assert.deepEqual(
        Object.keys(flattenActionHandlers(definition.config.actions)).sort(),
        [
            "check",
            "copies",
            "fill",
            "finished",
            "login",
            "moved",
            "new_session",
            "pi.wake",
            "place",
            "read_chat",
            "read_history",
            "read_models",
            "read_prompt",
            "reflect",
            "remove_path",
            "resume",
            "retry",
            "rewind",
            "shelves_changed",
            "stop",
            "submit",
        ],
    );
    await agent.stop();
});

/**
 * A harness whose first read of a conversation's live document, once
 * `hold.armed`, waits until `hold.released`: the read is then held with the
 * harness already in hand, as a tool's would be.
 */
function holding(
    harness: Harness,
    hold: { armed: boolean; entered: () => void; released: Promise<void> },
): Harness {
    return new Proxy(harness, {
        get(target, key) {
            const value: unknown = Reflect.get(target, key, target);
            if (typeof value !== "function") return value;
            if (key !== "snapshot") return value.bind(target);
            return async (...args: Parameters<Harness["snapshot"]>) => {
                if (hold.armed && (args[0] as unknown) === LiveDoc) {
                    hold.armed = false;
                    hold.entered();
                    await hold.released;
                }
                return target.snapshot(...args);
            };
        },
    });
}

test("a wake whose harness does not open still lists the hand tools, and the next action offers them", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            list_tools: () => ({
                tools: [
                    {
                        name: "word_count",
                        description: "Count the words of a file.",
                        parameters: JSON.stringify({ type: "object" }),
                    },
                ],
                faults: [],
                confinement: { type: "sandbox", name: "bwrap" },
            }),
            run: () => ({ type: "running", command: 9 }),
        },
    });
    const first = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("word_count", { path: "/project/a.md" }),
            fauxAssistantMessage("Counted."),
        ],
    });
    await until(() => named(sent, "list_tools").length === 1);
    await first.stop();

    const failed = first.failNextCommit();
    const second = await first.again();
    await failed;
    assert.ok(
        second.reported.some((line) => line.includes("could not open on wake")),
        "the wake's open failed",
    );
    await until(() => named(sent, "list_tools").length === 2);
    await second.actions.submit(second.by(from.realm), {
        id: "input-1",
        input: { type: "line", text: "Count." },
    });
    await second.idle();

    assert.equal(named(sent, "run").length, 1);
    await second.stop();
});

test("a commit that fails while the harness's own work reads the turn reopens the harness, and the agent goes on", async (t) => {
    const entered = Promise.withResolvers<void>();
    const released = Promise.withResolvers<void>();
    const hold = {
        armed: false,
        entered: entered.resolve,
        released: released.promise,
    };
    let agent: Woken | undefined;
    const turns = () =>
        (
            agent?.c.vars as
                | Record<symbol, { peers: { turns: Turns } } | undefined>
                | undefined
        )?.[Symbol.for("@hamlet/agent")]?.peers.turns;
    // A model whose first request reads its turn first, from inside the
    // harness's own work, as the gate does, then answers.
    let requests = 0;
    const reading = (models: Models): Models =>
        new Proxy(models, {
            get(target, key, receiver) {
                const value: unknown = Reflect.get(target, key, receiver);
                if (typeof value !== "function") return value;
                if (key !== "streamSimple") return value.bind(target);
                return (...args: Parameters<Models["streamSimple"]>) => {
                    const out = createAssistantMessageEventStream();
                    void (async () => {
                        requests += 1;
                        if (requests === 1) {
                            hold.armed = true;
                            await turns()
                                ?.ofSession(args[2]?.sessionId)
                                .catch(() => undefined);
                        }
                        const source = target.streamSimple(...args);
                        for await (const event of source) out.push(event);
                        out.end(await source.result());
                    })();
                    return out;
                };
            },
        });
    const { calls } = fakeCalls({});
    agent = await wakeAgent(t, {
        calls,
        models: reading,
        responses: ["cut off", "Hello.", "Still here.", "spare"].map((text) =>
            fauxAssistantMessage(text),
        ),
    });
    const runtime = piRuntime(agent.c);
    const open = await runtime.harness;
    assert.ok(open);
    runtime.harness = Promise.resolve({
        ...open,
        harness: holding(open.harness, hold),
    });
    const first = await agent.harness();

    await agent.actions.submit(agent.by(from.realm), {
        id: "input-1",
        task: "task-1",
        input: { type: "line", text: "Go." },
    });
    await entered.promise;
    // The read is held. Now another input's commit fails, as on a full disk,
    // and the wrapper reopens the harness for that action.
    const failed = agent.failNextCommit();
    const outside = agent.actions
        .submit(agent.by(from.realm), {
            id: "input-2",
            input: { type: "line", text: "And this." },
        })
        .then(
            () => "answered",
            (error: unknown) => `rejected: ${(error as Error).message}`,
        );
    await failed;
    await new Promise((resolve) => setTimeout(resolve, 50));
    released.resolve();

    const now = await within(
        agent.harness(),
        5_000,
        "the harness never opened again",
    );
    assert.notEqual(now, "the harness never opened again");
    assert.notEqual(now, first, "the harness was replaced");
    assert.equal(
        await within(
            agent.actions.submit(agent.by(from.realm), {
                id: "input-3",
                input: { type: "line", text: "Still there?" },
            }),
            5_000,
            "no answer in 5 s",
        ),
        null,
    );
    assert.match(
        String(await within(outside, 3_000, "no answer in 3 s")),
        /^rejected: .*disk I\/O error/,
    );
    await agent.idle();
    await agent.stop();
});

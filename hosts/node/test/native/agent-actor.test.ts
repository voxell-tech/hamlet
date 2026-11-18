import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import {
    type FauxResponseStep,
    fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import {
    ActorError,
    Address,
    actor,
    type CallOptions,
    type Calls,
} from "../../src/actor/actor/mod.ts";
import type { JsonCompatValue } from "../../src/actor/common/encoding.ts";
import { decodeCborCompat } from "../../src/actor/serde.ts";
import { agentActor } from "../../src/agent/actor.ts";
import type {
    AgentActions,
    AgentInput,
    Finished,
    Submit,
} from "../../src/agent/messages.ts";
import { fakeFiles } from "../agent-testing.ts";
import { fauxModels } from "../testing.ts";
import { eventually, inFolder, startLocalHost } from "./support.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };

const REALM_DEADLINE_MS = 4_000;

type AgentAction = keyof AgentActions | "pi.wake";

type Got = (
    | { readonly answer: unknown }
    | { readonly error: { readonly group: string; readonly code: string } }
) & { readonly ms: number };

async function send(
    c: Calls,
    n: number,
    name: AgentAction,
    message: unknown,
): Promise<Got> {
    const to = Address.of<Record<AgentAction, (message: unknown) => unknown>>(
        "agent",
        String(n),
    );
    const start = performance.now();
    try {
        const answer = await c.call(to, name, [message], {
            deadlineMs: REALM_DEADLINE_MS,
        });
        return { answer, ms: performance.now() - start };
    } catch (error) {
        if (!(error instanceof ActorError)) throw error;
        return {
            error: { group: error.group, code: error.code },
            ms: performance.now() - start,
        };
    }
}

const realm = actor({
    actions: {
        send,
        may_spend: () => ({ type: "done" }),
        charge: () => null,
        secrets: () => ({ version: 1, values: [] }),
    },
});

const workspace = actor({
    actions: {
        send,
        list_tools: () => ({ tools: [], faults: [] }),
    },
});

const HAND: AgentInput = {
    kind: "hand",
    task: "standing-1",
    place: { version: 1, hires: [], roads: [], tools_off: [] },
    copies: {},
};

function line(id: string, text: string): Submit {
    return { id, input: { type: "line", text } };
}

/** `opened` counts the agent's `onCreate`s and `onWake`s. */
function kindsAnswering(responses: FauxResponseStep[]) {
    const { faux, models, model } = fauxModels();
    faux.setResponses(responses);
    const files = { opened: 0 };
    const agent = agentActor({
        models,
        model,
        files: () => {
            files.opened += 1;
            return fakeFiles({});
        },
    });
    return { files, kinds: { agent, realm, workspace } };
}

async function startWithAgent(
    dir: string,
    kinds: ReturnType<typeof kindsAnswering>["kinds"],
) {
    const host = await startLocalHost(dir, kinds);
    await host.create(host.at("realm", "local"));
    await host.create(host.at("workspace", "local"));
    await host.create(
        host.at("agent", "1"),
        HAND as unknown as JsonCompatValue,
    );
    return host;
}

function agentFile(dir: string) {
    const database = new DatabaseSync(
        join(dir, "actors", "agent", "1.sqlite"),
        { readOnly: true },
    );
    try {
        return {
            submissions: database
                .prepare(
                    "SELECT request_id ->> '$' AS request FROM pi_submissions ORDER BY id",
                )
                .all()
                .map((row) => row.request),
            replies: database
                .prepare(
                    "SELECT record FROM pi_entries WHERE record ->> '$.kind' = 'pi.assistant' ORDER BY id",
                )
                .all()
                .map((row) => row.record),
        };
    } finally {
        database.close();
    }
}

const ENDED: Finished = {
    call: "t999",
    outcome: {
        type: "run",
        end: { type: "not_run", why: { type: "denied" }, words: "Not run." },
    },
};

test("every agent action answers null to its caller across the host", async () => {
    await inFolder(async (dir) => {
        const { kinds } = kindsAnswering([
            fauxAssistantMessage("Done."),
            fauxAssistantMessage("Done."),
        ]);
        const host = await startWithAgent(dir, kinds);
        const r = host.at("realm", "local");
        const w = host.at("workspace", "local");

        const answers = {
            submit: await host.call(
                r,
                "send",
                [1, "submit", line("s1", "Hello.")],
                CALL,
            ),
            place: await host.call(
                r,
                "send",
                [1, "place", { ...HAND.place, version: 2 }],
                CALL,
            ),
            fill: await host.call(
                r,
                "send",
                [1, "fill", { profile: 3, branch: "main" }],
                CALL,
            ),
            finished: await host.call(w, "send", [1, "finished", ENDED], CALL),
        };
        const alarm = await host.call(
            host.at("agent", "1") as unknown as Address<{
                "pi.wake"(): unknown;
            }>,
            "pi.wake",
            [],
            CALL,
        );
        await host.shutdown();

        for (const [name, got] of Object.entries(answers)) {
            assert.ok("answer" in got, `${name}: ${JSON.stringify(got)}`);
            assert.equal(got.answer, null, name);
        }
        assert.equal(alarm, null, "pi.wake");
    });
});

test("a wrong caller across the host gets message.wrong_caller, and nothing is admitted", async () => {
    await inFolder(async (dir) => {
        const { kinds } = kindsAnswering([fauxAssistantMessage("Done.")]);
        const host = await startWithAgent(dir, kinds);
        const r = host.at("realm", "local");
        const w = host.at("workspace", "local");

        const refused = [
            await host.call(
                w,
                "send",
                [1, "submit", line("from-workspace", "Hi.")],
                CALL,
            ),
            await host.call(r, "send", [1, "finished", ENDED], CALL),
            await host.call(r, "send", [1, "pi.wake", {}], CALL),
        ];
        await assert.rejects(
            host.call(
                host.at("agent", "1") as unknown as Address<
                    Pick<AgentActions, "submit">
                >,
                "submit",
                [line("from-no-one", "Hi.")],
                CALL,
            ),
            (error) =>
                error instanceof ActorError &&
                error.group === "message" &&
                error.code === "wrong_caller",
        );
        const admitted = await host.call(
            r,
            "send",
            [1, "submit", line("from-realm", "Hi.")],
            CALL,
        );
        await host.shutdown();

        for (const got of refused) {
            assert.partialDeepStrictEqual(got, {
                error: { group: "message", code: "wrong_caller" },
            });
        }
        assert.partialDeepStrictEqual(admitted, { answer: null });
        assert.deepEqual(agentFile(dir).submissions, ["from-realm"]);
    });
});

const AGENT_FIXTURES = new URL(
    "../../../../crates/hamlet_messages/fixtures/agent/",
    import.meta.url,
);

function argsOf(name: string): unknown {
    const [message] = decodeCborCompat<unknown[]>(
        readFileSync(new URL(`${name}.args.cbor`, AGENT_FIXTURES)),
    );
    return message;
}

test("a client's call made by the realm, and an input the agent does not take, are refused, and the agent goes on", async () => {
    await inFolder(async (dir) => {
        const { kinds } = kindsAnswering([fauxAssistantMessage("Done.")]);
        const host = await startWithAgent(dir, kinds);
        const r = host.at("realm", "local");
        const takes = ["submit", "finished", "place", "fill", "copies"];
        // A client's calls the agent does not take yet.
        const later = ["read_models", "check"];
        const actions = [
            ...new Set(
                readdirSync(AGENT_FIXTURES)
                    .filter((file) => file.endsWith(".args.cbor"))
                    .map((file) => file.split(".")[0] ?? ""),
            ),
        ].filter(
            (action) => !takes.includes(action) && !later.includes(action),
        );
        const refused: Record<string, unknown> = {};
        for (const action of actions) {
            refused[action] = await host.call(
                r,
                "send",
                [1, action as AgentAction, argsOf(action)],
                CALL,
            );
        }
        // Every input the realm's fixtures hold is taken, so a newer
        // realm's stands for one the agent does not.
        const unknown = await host.call(
            r,
            "send",
            [1, "submit", { id: "later", input: { type: "later", text: "x" } }],
            CALL,
        );
        const admitted = await host.call(
            r,
            "send",
            [1, "submit", line("after", "Hello.")],
            CALL,
        );
        await eventually(
            async () => agentFile(dir).replies.length,
            (count) => count > 0,
        );
        await host.shutdown();

        assert.ok(actions.includes("login"), actions.join());
        for (const action of actions) {
            assert.partialDeepStrictEqual(refused[action], {
                error: { group: "message", code: "wrong_caller" },
            });
        }
        assert.partialDeepStrictEqual(unknown, {
            error: { group: "message", code: "unreadable" },
        });
        assert.partialDeepStrictEqual(admitted, { answer: null });
        assert.deepEqual(agentFile(dir).submissions, ["after"]);
        assert.match(JSON.stringify(agentFile(dir).replies), /Done\./);
    });
});

test("submit wakes a sleeping agent within 4 s", async (t) => {
    await inFolder(async (dir) => {
        const { files, kinds } = kindsAnswering([
            fauxAssistantMessage("Done."),
        ]);
        await (await startWithAgent(dir, kinds)).shutdown();
        const asleep = files.opened;

        const host = await startLocalHost(dir, kinds);
        await host.getOrStart(host.at("realm", "local"));
        assert.equal(files.opened, asleep, "the reopened host woke no agent");
        const got = await host.call(
            host.at("realm", "local"),
            "send",
            [1, "submit", line("after-restart", "Hello again.")],
            CALL,
        );
        const woken = files.opened - asleep;
        await host.shutdown();

        t.diagnostic(`submit to a sleeping agent took ${got.ms.toFixed(0)} ms`);
        assert.partialDeepStrictEqual(got, { answer: null });
        assert.ok(got.ms < REALM_DEADLINE_MS, `${got.ms} ms`);
        assert.equal(woken, 1, "the submit woke the agent");
    });
});

test("the faux model's reply reaches the conversation after the submit", async () => {
    await inFolder(async (dir) => {
        const seen: unknown[][] = [];
        const { kinds } = kindsAnswering([
            fauxAssistantMessage("Paris."),
            (context) => {
                seen.push(context.messages);
                return fauxAssistantMessage("Noted.");
            },
        ]);
        const host = await startWithAgent(dir, kinds);
        const r = host.at("realm", "local");

        for (const [id, text] of [
            ["q1", "What is the capital of France?"],
            ["q2", "Thank you."],
        ] as const) {
            assert.partialDeepStrictEqual(
                await host.call(r, "send", [1, "submit", line(id, text)], CALL),
                { answer: null },
            );
        }
        await eventually(
            async () => seen.length,
            (count) => count > 0,
        );
        await host.shutdown();

        assert.match(JSON.stringify(seen[0]), /Paris\./);
        assert.match(JSON.stringify(agentFile(dir).replies), /Paris\./);
    });
});

import assert from "node:assert/strict";
import { join } from "node:path";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { AgentInput, Retired } from "../src/agent/messages/agent.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    named,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import {
    actionOf,
    answer,
    CREATED,
    HAND,
    LAST_WORDS,
    NOT_WORDS,
    REPORTED,
    TAKEN,
    TAKERS,
} from "./messages-testing.ts";
import { context, fixture, scratchFolder } from "./testing.ts";

for (const call of TAKEN) {
    const name = `${call}.args`;
    test(`${name} is read and taken, and answered as its answer fixture holds`, async (t) => {
        const action = actionOf(name).action as keyof typeof TAKERS;
        const { calls, sent } = fakeCalls({
            realm: {
                reply: () => answer("realm/reply.done"),
                fetch_part: () => answer("realm/fetch_part.bytes.last"),
            },
            workspace: { fit_image: () => answer("workspace/fit_image.image") },
            profile: {
                // A retire answers the tip the agent was filled at.
                files: () =>
                    action === "retire"
                        ? {
                              type: "part",
                              files: [],
                              removed: [],
                              tip: (fixture(TAKERS.retire.answer) as Retired)
                                  .tip,
                          }
                        : answer("profile/files.part.last"),
            },
        });
        const agent = await wakeAgent(t, {
            calls,
            input: HAND,
            responses: [fauxAssistantMessage(LAST_WORDS)],
            // An erase removes the folder at the filesystem's root.
            files: { ...fakeFiles({}), root: join(scratchFolder(t), "7.fs") },
        });
        // A profile's message comes from the profile the agent was filled from.
        if (name.startsWith("profile/") || action === "retire") {
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

test("realm/read_own_place.answer is kept at the wake, so the first request offers its tools", async (t) => {
    const given = answer("realm/read_own_place");
    const { calls } = fakeCalls({ realm: { read_own_place: () => given } });
    const agent = await wakeAgent(t, { calls, input: HAND });
    const row = await agent.c.db.get<{ place_version: number; place: string }>(
        "SELECT place_version, place FROM agent",
    );
    const root = await (await agent.harness()).root(context);
    const names = (await root.agent(context)).tools.map((tool) => tool.name);
    assert.deepEqual(agent.reported, []);
    await agent.stop();

    assert.equal(row?.place_version, 4);
    assert.equal(
        (JSON.parse(row?.place ?? "") as { folder?: unknown }).folder,
        "/work/orchard",
    );
    assert.ok(!names.includes("bash"), names.join());
    assert.ok(names.includes("read"), names.join());
});

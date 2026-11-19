import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { test } from "node:test";
import { ActorError } from "../src/actor/actor/mod.ts";
import { type ClientActions, read, readClient } from "../src/agent/messages.ts";
import { fakeCalls, from, wakeAgent } from "./agent-testing.ts";
import { FIXTURES, fixture } from "./testing.ts";

const FOLDER = new URL("agent/", FIXTURES);

function args(name: string): unknown[] {
    return fixture(`agent/${name}.args`) as unknown[];
}

function named(pattern: RegExp): string[] {
    return readdirSync(FOLDER)
        .map((file) => file.replace(/\.args\.cbor$/, ""))
        .filter((name) => pattern.test(name));
}

const CLIENT = Object.keys(readClient) as (keyof ClientActions)[];

test("every client call's fixture reads back as itself", () => {
    for (const action of CLIENT) {
        const fixtures = named(new RegExp(`^${action}(\\.[a-z_]+)?$`));
        assert.notEqual(fixtures.length, 0, action);
        for (const name of fixtures) {
            assert.deepStrictEqual(readClient[action](args(name)), args(name));
        }
    }
});

test("every copies fixture reads back as itself", () => {
    for (const name of named(/^copies(\.[a-z]+)?$/)) {
        const [message] = args(name);
        assert.deepStrictEqual(read.copies(message), message);
    }
});

test("a client's agent number past a safe integer names no agent, and a session number keeps its value", () => {
    const [resume] = readClient.resume([
        { id: "r", number: 2n ** 63n - 1n, to: 2n ** 64n - 1n },
    ]);
    assert.equal(resume.number, 2n ** 63n - 1n);
    assert.equal(resume.to, Number.POSITIVE_INFINITY);
});

test("a copy a newer realm sends with more fields keeps only the agent's", () => {
    const copies = read.copies({
        model: {
            version: 2,
            value: { model: "faux::faux-1", tier: "gold", fast: null },
        },
        later: { version: 1, value: "x" },
    });
    assert.deepStrictEqual(copies, {
        model: { version: 2, value: { model: "faux::faux-1" } },
    });
});

test("a client's call made by another actor is refused as the wrong caller", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, { calls });
    await assert.rejects(
        agent.actions.read_history(agent.by(from.realm)),
        (error: unknown) =>
            error instanceof ActorError && error.code === "wrong_caller",
    );
    await agent.stop();
});

import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type TestContext, test } from "node:test";
import { fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import type { Storage } from "@earendil-works/pi-durable";
import type { AgentDatabase } from "../src/agent/database.ts";
import {
    context,
    type Letter,
    openAgent,
    type StoredResult,
    stored,
    type TellArguments,
} from "./testing.ts";

function scratchFile(t: TestContext): string {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-agent-"));
    t.after(() => rmSync(folder, { recursive: true, force: true }));
    return join(folder, "agent.sqlite");
}

/** One assistant message that calls `tell` once per letter, then "done". */
function tellThenDone(letters: Record<string, TellArguments>) {
    return [
        fauxAssistantMessage(
            Object.entries(letters).map(([id, args]) =>
                fauxToolCall("tell", { ...args }, { id }),
            ),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("done"),
    ];
}

const byCall = (results: StoredResult[]) =>
    Object.fromEntries(results.map((result) => [result.call, result.isError]));

test("a send commits with its tool result, a small result and one over 8 KB", async (t) => {
    const forms = new Set<string>();
    const agent = await openAgent(scratchFile(t), {
        base: (database) => ({
            ...database,
            transaction: (callback) =>
                database.transaction((tx) =>
                    callback({
                        ...tx,
                        run: (sql, ...params) => {
                            if (sql.startsWith("INSERT INTO entries")) {
                                forms.add(
                                    sql.includes("json_each")
                                        ? "json"
                                        : "bound",
                                );
                            }
                            return tx.run(sql, ...params);
                        },
                    }),
                ),
        }),
    });
    t.after(agent.close);
    agent.faux.setResponses(
        tellThenDone({
            small: { to: "bob", body: "hi" },
            // Over 8,192 characters, pi-durable binds the entries row column
            // by column instead of in one JSON array.
            large: { to: "eve", body: "a long result", size: 9000 },
        }),
    );
    const submission = await agent.root.submit(
        { type: "input", content: "write" },
        context,
    );
    assert.equal((await submission.wait(context)).status, "done");

    const file = await stored(agent.database);
    assert.deepEqual(byCall(file.results), { small: false, large: false });
    assert.deepEqual(file.letters, [
        { call: "large", to: "eve", body: "a long result" },
        { call: "small", to: "bob", body: "hi" },
    ]);
    assert.equal(agent.layer.buffered, 0);
    assert.deepEqual([...forms].sort(), ["bound", "json"]);
});

test("a send whose tool then fails leaves no row", async (t) => {
    const agent = await openAgent(scratchFile(t));
    t.after(agent.close);
    agent.faux.setResponses(
        tellThenDone({
            kept: { to: "bob", body: "kept" },
            failed: { to: "eve", body: "never sent", fail: true },
        }),
    );
    const submission = await agent.root.submit(
        { type: "input", content: "write" },
        context,
    );
    assert.equal((await submission.wait(context)).status, "done");

    const file = await stored(agent.database);
    assert.deepEqual(byCall(file.results), { kept: false, failed: true });
    assert.deepEqual(file.letters, [{ call: "kept", to: "bob", body: "kept" }]);
    assert.equal(agent.layer.buffered, 0);
});

test("a refused batch leaves no row, for it or a later batch, and an empty buffer", async (t) => {
    let refusedWith: number | undefined;
    const refuseFirstCommitWithLetters = (
        storage: Storage,
        layer: AgentDatabase<Letter>,
    ): Storage =>
        new Proxy(storage, {
            get(target, key) {
                if (key === "commit") {
                    const commit: Storage["commit"] = (...args) => {
                        if (refusedWith === undefined && layer.buffered > 0) {
                            refusedWith = layer.buffered;
                            return Promise.resolve({
                                stored: 0,
                                refused: "refused by the test",
                            });
                        }
                        return target.commit(...args);
                    };
                    return commit;
                }
                const value: unknown = Reflect.get(target, key);
                return typeof value === "function" ? value.bind(target) : value;
            },
        });
    const agent = await openAgent(scratchFile(t), {
        storage: refuseFirstCommitWithLetters,
    });
    t.after(agent.close);
    agent.faux.setResponses(
        tellThenDone({
            first: { to: "bob", body: "one" },
            second: { to: "eve", body: "two" },
        }),
    );
    const submission = await agent.root.submit(
        { type: "input", content: "write" },
        context,
    );
    await assert.rejects(submission.wait(context));
    const end = await agent.harness.closed;

    assert.equal(end.kind, "failed");
    assert.equal(refusedWith, 2);
    assert.equal(agent.layer.buffered, 0);
    const file = await stored(agent.database);
    assert.deepEqual(file, { results: [], letters: [] });
});

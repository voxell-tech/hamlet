import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { scratchFolder } from "../testing.ts";

/** Runs `pi-kill-child.ts` on `dir` to its end, and parses what it wrote. */
function child(dir: string, replay: string, mode: "kill" | "resume") {
    const run = spawnSync(
        process.execPath,
        [join(import.meta.dirname, "pi-kill-child.ts"), dir, replay, mode],
        { encoding: "utf8", timeout: 60_000 },
    );
    const events = run.stdout
        .split("\n")
        .filter((line) => line !== "")
        .map((line) => JSON.parse(line) as Record<string, unknown>);
    return { signal: run.signal, status: run.status, events, run };
}

/**
 * What the agent's file holds, read from a copy with its sidecar files, so
 * the recovery that opening it runs changes only the copy.
 */
function inFile(dir: string, copy: string) {
    const folder = join(dir, "actors", "agent");
    mkdirSync(copy);
    for (const name of readdirSync(folder)) {
        if (name.startsWith("a1.sqlite")) {
            cpSync(join(folder, name), join(copy, name));
        }
    }
    const database = new DatabaseSync(join(copy, "a1.sqlite"));
    try {
        const rows = (sql: string) =>
            database
                .prepare(sql)
                .all()
                .map((row) => ({ ...row }));
        return {
            entries: rows(
                "SELECT record ->> '$.kind' AS kind FROM pi_entries ORDER BY id",
            ).map((row) => row.kind),
            submissions: rows(
                "SELECT request_id ->> '$' AS request, status FROM pi_submissions ORDER BY id",
            ),
            tasks: rows(
                "SELECT record ->> '$.kind' AS kind, status FROM pi_tasks ORDER BY id",
            ),
        };
    } finally {
        database.close();
    }
}

/** What the model sees in place of the result of a tool a kill cut off. */
const INTERRUPTED =
    "<harness>\n[error] Tool slow was interrupted and may have partially run\n</harness>";

// A record of what the wrapper and pi-durable do after a kill in a tool, not
// a bar: Hamlet's host starts the actor whose run the kill cut off, and the
// wrapper's onWake resumes the run before any call arrives. An `unsafe` tool
// is not run again and the model gets an error result; a `safe` one runs
// again. The same prompt then gets the stored answer.
for (const replay of ["unsafe", "safe"] as const) {
    test(`after a kill inside a ${replay} tool, the restarted host finishes the turn by itself`, async (t) => {
        const dir = scratchFolder(t);
        const scratch = scratchFolder(t);

        const killed = child(dir, replay, "kill");
        assert.equal(killed.signal, "SIGKILL", killed.run.stderr);
        assert.deepEqual(
            killed.events.map((event) => event.event),
            ["started", "model", "tool"],
        );
        assert.deepEqual(inFile(dir, join(scratch, "after-kill")), {
            entries: ["pi.user", "pi.system", "pi.assistant"],
            submissions: [{ request: "prompt-1", status: "placed" }],
            tasks: [
                { kind: "pi.generation", status: "waiting" },
                { kind: "pi.tool", status: "running" },
            ],
        });

        const resumed = child(dir, replay, "resume");
        assert.equal(resumed.status, 0, resumed.run.stderr);
        const of = (name: string) =>
            resumed.events.filter((event) => event.event === name);
        assert.deepEqual(of("started"), [
            { event: "started", due: ["agent/a1"] },
        ]);
        assert.equal(of("tool").length, replay === "safe" ? 1 : 0);
        const toolSaid = replay === "safe" ? "slow is done" : INTERRUPTED;
        assert.deepEqual(of("model"), [
            {
                event: "model",
                answered: true,
                isError: replay === "unsafe",
                text: toolSaid,
            },
        ]);
        // The model answered before the prompt was sent again.
        assert.deepEqual(of("waited"), [{ event: "waited", modelCalls: 1 }]);
        assert.partialDeepStrictEqual(of("answer"), [
            {
                answer: { status: "done", text: `the tool said: ${toolSaid}` },
                modelCalls: 1,
            },
        ]);
        assert.deepEqual(inFile(dir, join(scratch, "after-resume")), {
            entries: [
                "pi.user",
                "pi.system",
                "pi.assistant",
                "pi.tool-result",
                "pi.assistant",
            ],
            submissions: [{ request: "prompt-1", status: "done" }],
            tasks: [
                { kind: "pi.generation", status: "terminal" },
                { kind: "pi.tool", status: "terminal" },
                { kind: "pi.generation", status: "terminal" },
            ],
        });
    });
}

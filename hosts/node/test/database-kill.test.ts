import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { stored } from "./agent-fixture.ts";
import { plainDatabase } from "./plain-database.ts";

const RUNS = 20;

/** Where the child dies between buffering its letter and COMMIT. */
const BEFORE_COMMIT = [
    "after-buffer",
    "after-result-insert",
    "after-send-insert",
    "before-commit",
];

/** What the file holds after a child running `tell` was killed at `point`. */
async function killedAt(folder: string, run: number, point: string) {
    const file = join(folder, `${run}-${point}.sqlite`);
    const child = spawnSync(
        process.execPath,
        [join(import.meta.dirname, "database-kill-child.ts"), file, point],
        { encoding: "utf8", timeout: 60_000 },
    );
    assert.equal(child.signal, "SIGKILL", child.stderr);
    assert.equal(child.stdout, `reached ${point}\n`);
    // Opening the file read-write rolls back what the kill left unfinished.
    const database = plainDatabase(file);
    try {
        return await stored(database);
    } finally {
        await database.close();
    }
}

test(`a kill leaves the tool result and its letter both or neither, ${RUNS} runs`, async (t) => {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-kill-"));
    t.after(() => rmSync(folder, { recursive: true, force: true }));
    for (let run = 0; run < RUNS; run++) {
        const before = BEFORE_COMMIT[run % BEFORE_COMMIT.length] ?? "";
        assert.deepEqual(
            await killedAt(folder, run, before),
            { results: [], letters: [] },
            `killed at ${before}`,
        );
        assert.deepEqual(await killedAt(folder, run, "after-commit"), {
            results: [{ call: "letter", isError: false }],
            letters: [{ call: "letter", to: "bob", body: "hi" }],
        });
    }
});

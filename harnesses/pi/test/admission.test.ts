import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { DatabaseSync } from "node:sqlite";
import { afterEach, expect, test } from "vitest";

const FIXTURE = join(import.meta.dirname, "fixtures", "admit.ts");
const folders: string[] = [];

afterEach(() => {
    for (const folder of folders.splice(0))
        rmSync(folder, { recursive: true, force: true });
});

/** Runs the fixture in `phase` until its first line, then kills it at once, as a crash would; returns the line. */
async function admitThenKill(store: string, phase: string): Promise<unknown> {
    const child = spawn(
        process.execPath,
        ["--disable-warning=ExperimentalWarning", FIXTURE, store, phase],
        { stdio: ["ignore", "pipe", "inherit"] },
    );
    const exited = new Promise((resolve) => child.once("exit", resolve));
    const lines = createInterface({ input: child.stdout });
    const [line] = await Promise.race([
        new Promise<[string]>((resolve) =>
            lines.once("line", (l) => resolve([l])),
        ),
        exited.then(() => {
            throw new Error(`the fixture ended in ${phase} before admitting`);
        }),
    ]);
    child.kill("SIGKILL");
    await exited;
    return JSON.parse(line);
}

test("an input admitted before a crash is admitted once when submitted again", async () => {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-pi-admit-"));
    folders.push(folder);
    const store = join(folder, "pi.sqlite");
    const first = await admitThenKill(store, "first");
    // The same request id again, in a new process, after the first was killed.
    const again = await admitThenKill(store, "again");
    expect(again).toEqual(first);
    const db = new DatabaseSync(store, { readOnly: true });
    const rows = db.prepare("SELECT count(*) AS n FROM submissions").all();
    db.close();
    expect(rows).toEqual([{ n: 1 }]);
});

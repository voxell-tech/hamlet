import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import {
    mkdirSync,
    readdirSync,
    readFileSync,
    rmdirSync,
    watch,
    writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { openAgentFilesystem } from "../../src/agent/native-filesystem.ts";
import { context, scratchFolder } from "../testing.ts";

/** How many rounds of file tools run while the folder is swapped. */
const ROUNDS = 2000;

const WATCH_WAIT_MS = 10_000;

type Touched = { event: string; name: string | null; at: number };

test("a command that swaps a folder for a link while file tools run never lets a tool reach outside", {
    timeout: 120_000,
}, async (t) => {
    const realm = scratchFolder(t);
    const outside = join(realm, "outside");
    mkdirSync(outside);
    writeFileSync(join(outside, "secret"), "SECRET");
    const filesystem = openAgentFilesystem(realm, 1);
    const work = join(filesystem.root, "memory", "work");
    mkdirSync(work);
    writeFileSync(join(work, "kept"), "kept");

    const touched: Touched[] = [];
    const watched = performance.now();
    const watcher = watch(outside, (event, name) =>
        touched.push({ event, name, at: performance.now() }),
    );
    t.after(() => watcher.close());
    const counted = await countFromHere(outside, touched);

    const stop = join(realm, "stop");
    const swapper = spawn(
        process.execPath,
        [
            join(import.meta.dirname, "filesystem-swap-child.ts"),
            work,
            outside,
            stop,
        ],
        { stdio: ["ignore", "pipe", "inherit"] },
    );
    let printed = "";
    swapper.stdout.setEncoding("utf8");
    swapper.stdout.on("data", (chunk: string) => {
        printed += chunk;
    });
    const exited = once(swapper, "exit");
    // Stops the swapper also when an assertion below fails.
    t.after(() => swapper.kill("SIGKILL"));
    while (!printed.startsWith("ready\n")) await sleep(10);

    let written = 0;
    let refused = 0;
    const decoder = new TextDecoder();
    try {
        for (let round = 0; round < ROUNDS; round++) {
            const wrote = await filesystem.write(
                `memory/work/new-${round}`,
                "made by a tool",
                context,
            );
            if (wrote.ok) written++;
            else refused++;
            const read = await filesystem.read("memory/work/secret", context);
            assert.ok(
                !read.ok,
                `read ${decoder.decode(read.ok ? read.value : undefined)}`,
            );
            const listed = await filesystem.list("memory/work", context);
            if (listed.ok) {
                assert.deepEqual(
                    listed.value.filter((entry) => entry.name === "secret"),
                    [],
                );
            }
            const found = await filesystem.find("secret", "memory", context);
            if (found.ok) assert.deepEqual(found.value.found, []);
            const matched = await filesystem.search(
                "SECRET",
                "memory",
                undefined,
                context,
            );
            if (matched.ok) assert.deepEqual(matched.value.found, []);
        }
    } finally {
        writeFileSync(stop, "");
        await exited;
    }
    const swaps = Number(printed.split("\n")[1]);
    // Let the watcher report what the last rounds did.
    await sleep(200);

    assert.deepEqual(readdirSync(outside), ["secret"]);
    assert.equal(readFileSync(join(outside, "secret"), "utf8"), "SECRET");
    assert.deepEqual(
        touched.map(
            ({ event, name, at }) =>
                `${event} ${name}, ${Math.round(at - watched)} ms after the watch began, ${Math.round(at - counted)} ms after the markers`,
        ),
        [],
    );
    assert.ok(swaps > 100, `the link was in place ${swaps} times`);
    assert.ok(written > 0, "no write went through");
    assert.ok(refused > 0, "no write was refused");
});

/**
 * Clears `touched` once the watch on `outside` has reported a marker folder
 * and then its removal, the last setup change. The watch reports in order,
 * so all it reports after that happened after the setup. On macOS it may
 * begin late (https://github.com/nodejs/node/issues/52601) and may report
 * changes from before it began.
 */
async function countFromHere(
    outside: string,
    touched: Touched[],
): Promise<number> {
    const end = performance.now() + WATCH_WAIT_MS;
    const reported = (name: string, from = 0) =>
        touched.slice(from).some((touch) => touch.name === name);
    const made: string[] = [];
    let shown: string | undefined;
    while (shown === undefined) {
        if (performance.now() > end) {
            assert.fail(
                `the watch on ${outside} reported none of ${made.length} marker folders in ${WATCH_WAIT_MS} ms`,
            );
        }
        const name = `marker-${made.length}`;
        mkdirSync(join(outside, name));
        made.push(name);
        await sleep(20);
        shown = made.find((marker) => reported(marker));
    }
    for (const name of made) {
        if (name !== shown) rmdirSync(join(outside, name));
    }
    const removed = touched.length;
    rmdirSync(join(outside, shown));
    while (!reported(shown, removed)) {
        if (performance.now() > end) {
            assert.fail(
                `the watch on ${outside} did not report the removal of ${shown} in ${WATCH_WAIT_MS} ms`,
            );
        }
        await sleep(20);
    }
    touched.length = 0;
    return performance.now();
}

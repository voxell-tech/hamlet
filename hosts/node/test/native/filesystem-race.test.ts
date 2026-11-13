import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import {
    mkdirSync,
    readdirSync,
    readFileSync,
    watch,
    writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { openAgentFilesystem } from "../../src/agent/filesystem.ts";
import { context, scratchFolder } from "../testing.ts";

/** How many rounds of file tools run while the folder is swapped. */
const ROUNDS = 2000;

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

    // Anything made, changed or removed outside, for the whole run.
    const touched: string[] = [];
    const watcher = watch(outside, (event, name) =>
        touched.push(`${event} ${name}`),
    );
    t.after(() => watcher.close());

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
    while (!printed.startsWith("ready\n")) {
        await new Promise((resolve) => setTimeout(resolve, 10));
    }

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
    await new Promise((resolve) => setTimeout(resolve, 200));

    assert.deepEqual(touched, []);
    assert.deepEqual(readdirSync(outside), ["secret"]);
    assert.equal(readFileSync(join(outside, "secret"), "utf8"), "SECRET");
    assert.ok(swaps > 100, `the link was in place ${swaps} times`);
    assert.ok(written > 0, "no write went through");
    assert.ok(refused > 0, "no write was refused");
});

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { join } from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

/** The Node host's folder, where `@hamlet/actor-node` resolves. */
const HOST = fileURLToPath(new URL("../..", import.meta.url));

/** The repository's folder. */
const ROOT = fileURLToPath(new URL("../../../..", import.meta.url));

/** `hamlet_os`'s example that runs its arguments as its bound child, which `cargo xtask node` builds. */
const BOUND = join(
    process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"),
    "debug",
    "examples",
    process.platform === "win32" ? "bound.exe" : "bound",
);

/** A Node host that takes the link's ends, then uses stdout and stdin as any code would. */
const CHILD = `
require("@hamlet/actor-node").takeLink();
process.stdout.write("written to stdout\\n");
console.log("logged");
let read = "";
process.stdin.on("data", (chunk) => {
    read += chunk;
});
process.stdin.on("end", () => {
    process.stderr.write(\`stdin ended after \${JSON.stringify(read)}\\n\`);
});
`;

/** A Node host that takes the link's ends, says so with its pid on stderr, and stays alive. */
const HELD = `
require("@hamlet/actor-node").takeLink();
process.stderr.write(\`taken \${process.pid}\\n\`);
setInterval(() => {}, 1_000);
`;

/** Whether the process `pid` still runs. */
function isAlive(pid: number): boolean {
    try {
        process.kill(pid, 0);
        return true;
    } catch {
        return false;
    }
}

test("takeLink keeps what Node writes to stdout off the link, and stdin empty", async () => {
    const child = spawn(BOUND, [process.execPath, "-e", CHILD], {
        cwd: HOST,
        env: { PATH: process.env.PATH },
        signal: AbortSignal.timeout(20_000),
        killSignal: "SIGKILL",
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
        stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
        stderr += chunk;
    });
    const [code] = await once(child, "close");
    child.stdin.end();

    assert.equal(code, 0, stderr);
    assert.equal(stdout, "");
    assert.match(stderr, /^written to stdout$/m);
    assert.match(stderr, /^logged$/m);
    assert.match(stderr, /^stdin ended after ""$/m);
});

test("a Node host whose parent is killed is gone within 2 seconds", async () => {
    const parent = spawn(BOUND, [process.execPath, "-e", HELD], {
        cwd: HOST,
        env: { PATH: process.env.PATH },
        stdio: ["ignore", "ignore", "pipe"],
        signal: AbortSignal.timeout(20_000),
        killSignal: "SIGKILL",
    });
    let stderr = "";
    parent.stderr.on("data", (chunk) => {
        stderr += chunk;
    });
    const taken = Date.now() + 20_000;
    let shown = /^taken (\d+)$/m.exec(stderr);
    while (shown === null) {
        assert.ok(Date.now() < taken, stderr);
        await sleep(10);
        shown = /^taken (\d+)$/m.exec(stderr);
    }
    const pid = Number(shown[1]);

    parent.kill("SIGKILL");
    const deadline = Date.now() + 2_000;
    while (isAlive(pid)) {
        if (Date.now() > deadline) {
            process.kill(pid, "SIGKILL");
            assert.fail(`the Node host ${pid} outlived its parent`);
        }
        await sleep(20);
    }
});

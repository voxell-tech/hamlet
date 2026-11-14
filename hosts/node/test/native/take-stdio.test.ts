import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

/** The Node host's folder, where `@hamlet/actor-node` resolves. */
const HOST = fileURLToPath(new URL("../..", import.meta.url));

/** A Node host that takes its stdio for the link, then uses stdout and stdin as any code would. */
const CHILD = `
require("@hamlet/actor-node").takeStdio();
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

/** A Node host that takes its stdio for the link, says so on stderr, and stays alive. */
const HELD = `
require("@hamlet/actor-node").takeStdio();
process.stderr.write("taken\\n");
setInterval(() => {}, 1_000);
`;

/** A process that starts `HELD` as its child, prints the child's pid, and stays alive. */
const MIDDLE = `
const { spawn } = require("node:child_process");
const host = spawn(process.execPath, ["-e", ${JSON.stringify(HELD)}], {
    cwd: ${JSON.stringify(HOST)},
    env: { PATH: process.env.PATH, HAMLET_LINK_PARENT: String(process.pid) },
    stdio: ["pipe", "pipe", "inherit"],
});
process.stdout.write(\`\${host.pid}\\n\`);
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

test("takeStdio leaves stdout to the link and stdin to nothing", async () => {
    const child = spawn(process.execPath, ["-e", CHILD], {
        cwd: HOST,
        env: {
            PATH: process.env.PATH,
            HAMLET_LINK_PARENT: String(process.pid),
        },
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
    const middle = spawn(process.execPath, ["-e", MIDDLE], {
        cwd: HOST,
        stdio: ["ignore", "pipe", "pipe"],
        signal: AbortSignal.timeout(20_000),
        killSignal: "SIGKILL",
    });
    let stdout = "";
    let stderr = "";
    middle.stdout.on("data", (chunk) => {
        stdout += chunk;
    });
    middle.stderr.on("data", (chunk) => {
        stderr += chunk;
    });
    const taken = Date.now() + 20_000;
    while (!/^\d+$/m.test(stdout) || !/^taken$/m.test(stderr)) {
        assert.ok(Date.now() < taken, stderr);
        await sleep(10);
    }
    const pid = Number(stdout.trim());

    middle.kill("SIGKILL");
    const deadline = Date.now() + 2_000;
    while (isAlive(pid)) {
        if (Date.now() > deadline) {
            process.kill(pid, "SIGKILL");
            assert.fail(`the Node host ${pid} outlived its parent`);
        }
        await sleep(20);
    }
});

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readdirSync } from "node:fs";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { inFolder } from "../native/support.ts";
import { example, MAIN, startDemo } from "./support.ts";

const SKIP = { skip: process.platform === "win32" };

const ONLY_LINUX = { skip: process.platform !== "linux" };

const NO_LINK = /no link to a Rust host.*launcher.*HAMLET_NODE/;

/** Runs `argv` with the realm folder `dir`, and answers its exit code and standard error. */
async function run(
    argv: readonly [string, ...string[]],
    dir: string,
): Promise<{ code: number | null; stderr: string }> {
    const child = spawn(argv[0], argv.slice(1), {
        env: { PATH: process.env.PATH, HAMLET_HOME: dir },
        stdio: ["ignore", "ignore", "pipe"],
        signal: AbortSignal.timeout(20_000),
        killSignal: "SIGKILL",
    });
    let stderr = "";
    child.stderr.on("data", (chunk) => {
        stderr += chunk;
    });
    const [code] = await once(child, "close");
    return { code, stderr };
}

test("main.ts started by hand says Node has no link, names HAMLET_NODE, exits 1 and opens nothing", async () => {
    await inFolder(async (dir) => {
        const { code, stderr } = await run(
            [process.execPath, MAIN, "--kind", "agent"],
            dir,
        );
        assert.equal(code, 1, stderr);
        assert.match(stderr, NO_LINK);
        assert.deepEqual(readdirSync(dir), []);
    });
});

test(
    "main.ts started through a shell by a Rust host fails the same way",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            // The echo keeps the shell from replacing itself with Node.
            const { stderr } = await run(
                [
                    example("bound"),
                    "/bin/sh",
                    "-c",
                    `"${process.execPath}" "${MAIN}" --kind agent; echo "code $?" >&2`,
                ],
                dir,
            );
            assert.match(stderr, NO_LINK);
            assert.match(stderr, /^code 1$/m);
            assert.deepEqual(readdirSync(dir), []);
        });
    },
);

test(
    "a kind demo.ts lacks ends the Node host with its name, and so does no kind",
    SKIP,
    async () => {
        await inFolder(async (dir) => {
            const lacking = await run(
                [
                    example("bound"),
                    process.execPath,
                    MAIN,
                    "--kind",
                    "agent",
                    "--kind",
                    "spare",
                ],
                dir,
            );
            assert.equal(lacking.code, 1, lacking.stderr);
            assert.match(lacking.stderr, /'spare'/);
            const none = await run(
                [example("bound"), process.execPath, MAIN],
                dir,
            );
            assert.equal(none.code, 1, none.stderr);
            assert.match(none.stderr, /--kind/);
            assert.deepEqual(readdirSync(dir), []);
        });
    },
);

/** How many Node hosts the second demo starts before the test stops it. */
const ATTEMPTS = 5;

test(
    "each Node host of a second demo on a folder another Node host holds says node.lock is held before it ends, and the first still answers",
    ONLY_LINUX,
    async () => {
        await inFolder(async (dir) => {
            const first = startDemo(dir);
            assert.equal(await first.ask("link"), "link/ready 1");
            const second = spawn(
                example("demo"),
                ["--name", "second", "--kind", "spare", "--link", "link"],
                {
                    env: {
                        PATH: process.env.PATH,
                        HAMLET_HOME: dir,
                        HAMLET_NODE: process.execPath,
                    },
                    stdio: ["ignore", "ignore", "pipe"],
                    signal: AbortSignal.timeout(60_000),
                    killSignal: "SIGKILL",
                },
            );
            let stderr = "";
            second.stderr.on("data", (chunk) => {
                stderr += chunk;
            });
            const closed = once(second, "close");
            const ended = () =>
                stderr.match(/the Node host's output ended/g)?.length ?? 0;
            const until = Date.now() + 30_000;
            while (ended() < ATTEMPTS) {
                if (second.exitCode !== null || Date.now() > until) {
                    assert.fail(
                        `the second demo started too few Node hosts:\n${stderr}`,
                    );
                }
                await sleep(20);
            }
            second.kill("SIGTERM");
            await closed;
            const held =
                stderr.match(/^hamlet node host: .*node\.lock/gm)?.length ?? 0;
            // A Node host that is starting when the demo stops never reaches the lock.
            assert.ok(held >= ended() - 1, stderr);
            assert.equal(await first.ask("make 1"), "agent/1 1");
            assert.equal(await first.stop(), 0, first.stderr());
        });
    },
);

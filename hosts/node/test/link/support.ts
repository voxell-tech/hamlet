import { spawn } from "node:child_process";
import { once } from "node:events";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../../../..", import.meta.url));

/** How long `ask` and `logged` wait. */
const PATIENCE_MS = 30_000;

/** The Node host's entry. */
export const MAIN = fileURLToPath(
    new URL("../../src/main.ts", import.meta.url),
);

/** The built example `name` of the Rust workspace. */
export function example(name: string): string {
    return join(
        process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"),
        "debug",
        "examples",
        process.platform === "win32" ? `${name}.exe` : name,
    );
}

/** A Rust demo host with its Node host, driven through `demo --link serve`. */
export interface Demo {
    /** The demo host's process id. */
    readonly pid: number;
    /**
     * Writes one command line, and answers the next line the demo prints.
     * Rejects when the demo exits first.
     */
    ask(line: string): Promise<string>;
    /** What the demo and its Node host have written to standard error. */
    stderr(): string;
    /** Waits until standard error holds `pattern`. */
    logged(pattern: RegExp): Promise<void>;
    /** Ends the demo's input, and answers its exit code. */
    stop(): Promise<number | null>;
}

/**
 * Starts `demo --link serve` on the realm folder `dir`, with the Node that
 * runs this test as its Node host's program.
 */
export function startDemo(dir: string): Demo {
    const child = spawn(example("demo"), ["--link", "serve"], {
        env: {
            PATH: process.env.PATH,
            HAMLET_HOME: dir,
            HAMLET_NODE: process.execPath,
        },
        stdio: ["pipe", "pipe", "pipe"],
        signal: AbortSignal.timeout(120_000),
        killSignal: "SIGKILL",
    });
    const pid = child.pid;
    if (pid === undefined) throw new Error("the demo did not start");
    let stderr = "";
    child.stderr.on("data", (chunk) => {
        stderr += chunk;
    });
    const lines = createInterface({ input: child.stdout })[
        Symbol.asyncIterator
    ]();
    const closed = once(child, "close");
    return {
        pid,
        async ask(line) {
            child.stdin.write(`${line}\n`);
            const next = await Promise.race([
                lines.next(),
                sleep(PATIENCE_MS, undefined, { ref: false }).then(() => {
                    throw new Error(`no answer to \`${line}\`:\n${stderr}`);
                }),
            ]);
            if (next.done) {
                throw new Error(
                    `the demo ended before \`${line}\`:\n${stderr}`,
                );
            }
            return next.value;
        },
        stderr: () => stderr,
        async logged(pattern) {
            const until = Date.now() + PATIENCE_MS;
            while (!pattern.test(stderr)) {
                if (Date.now() > until) {
                    throw new Error(`never logged ${pattern}:\n${stderr}`);
                }
                await sleep(20);
            }
        },
        async stop() {
            child.stdin.end();
            const [code] = await closed;
            return code;
        },
    };
}

/** The JSON after `name` on a line `<name> <json>` the demo printed. */
export function answer(line: string, name: string): unknown {
    const prefix = `${name} `;
    if (!line.startsWith(prefix)) {
        throw new Error(`the demo printed \`${line}\`, not ${name}`);
    }
    return JSON.parse(line.slice(prefix.length));
}

/** The process ids of `pid`'s children, on Linux. */
export function childrenOf(pid: number): number[] {
    return readdirSync(`/proc/${pid}/task`).flatMap((task) =>
        readFileSync(`/proc/${pid}/task/${task}/children`, "utf8")
            .split(" ")
            .filter((word) => word !== "")
            .map(Number),
    );
}

/** Whether the process `pid` has ended, also when no parent collected it, on Linux. */
export function hasEnded(pid: number): boolean {
    try {
        const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
        return stat
            .slice(stat.lastIndexOf(")") + 1)
            .trimStart()
            .startsWith("Z");
    } catch {
        return true;
    }
}

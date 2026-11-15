import { spawn } from "node:child_process";
import { once } from "node:events";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { eventually } from "../native/support.ts";

const ROOT = fileURLToPath(new URL("../../../..", import.meta.url));

const PATIENCE_MS = 30_000;

const STREAMED = /^watch\/[^/ ]+\//;

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
    ask(line: string): Promise<string>;
    streamed(): readonly string[];
    streamedLine(pattern: RegExp): Promise<void>;
    /** Everything the demo and its Node host have written to standard error. */
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
    const replies: string[] = [];
    const streamed: string[] = [];
    let asked = 0;
    let ended = false;
    createInterface({ input: child.stdout })
        .on("line", (line) =>
            (STREAMED.test(line) ? streamed : replies).push(line),
        )
        .on("close", () => {
            ended = true;
        });
    const closed = once(child, "close");
    async function until<T>(
        found: () => T | undefined,
        what: string,
    ): Promise<T> {
        const value = await eventually(
            async () => found(),
            (value) => value !== undefined,
            PATIENCE_MS,
        ).catch((error: unknown) => {
            throw new Error(`${what}:\n${stderr}`, { cause: error });
        });
        if (value === undefined) throw new Error(what);
        return value;
    }
    return {
        pid,
        ask(line) {
            child.stdin.write(`${line}\n`);
            return until(() => {
                if (asked < replies.length) return replies[asked++];
                if (ended) {
                    throw new Error(
                        `the demo ended before \`${line}\`:\n${stderr}`,
                    );
                }
                return undefined;
            }, `no answer to \`${line}\``);
        },
        streamed: () => streamed,
        async streamedLine(pattern) {
            await until(
                () => streamed.some((line) => pattern.test(line)) || undefined,
                `never streamed ${pattern}`,
            );
        },
        stderr: () => stderr,
        async logged(pattern) {
            await until(
                () => pattern.test(stderr) || undefined,
                `never logged ${pattern}`,
            );
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

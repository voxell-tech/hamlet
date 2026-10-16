import { spawn } from "node:child_process";
import type { CommandIO } from "./io.ts";

/** Starts a child on the chief's descriptors, or forwards caller-owned test streams. */
export function startProcess(
    file: string,
    args: readonly string[],
    env: Readonly<NodeJS.ProcessEnv>,
    io?: CommandIO,
) {
    const child = spawn(file, args, {
        env,
        stdio:
            io === undefined || io.childStdio === "inherit"
                ? "inherit"
                : "pipe",
    });
    if (io && io.childStdio !== "inherit") {
        if (child.stdin) {
            const input = child.stdin;
            io.stdin.pipe(input);
            child.once("close", () => io.stdin.unpipe(input));
        }
        child.stdout?.pipe(io.stdout, { end: false });
        child.stderr?.pipe(io.stderr, { end: false });
    }
    return child;
}

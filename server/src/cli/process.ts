import { spawn } from "node:child_process";
import type { CommandIO } from "./io.ts";

/** Starts a child on the chief's streams, or an unreferenced session with closed stdin. */
export function startProcess(
    file: string,
    args: readonly string[],
    env: Readonly<NodeJS.ProcessEnv>,
    io?: CommandIO,
    background?: { output?: number },
) {
    const child = spawn(file, args, {
        env,
        detached: background !== undefined,
        stdio:
            background !== undefined
                ? [
                      "ignore",
                      background.output ?? "ignore",
                      background.output ?? "ignore",
                  ]
                : io === undefined || io.childStdio === "inherit"
                  ? "inherit"
                  : "pipe",
    });
    if (background !== undefined) child.unref();
    if (background === undefined && io && io.childStdio !== "inherit") {
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

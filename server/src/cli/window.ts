import { spawn } from "node:child_process";
import { accessSync, constants, statSync } from "node:fs";
import { delimiter, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { ServerAddress } from "./client.ts";
import type { CommandIO } from "./io.ts";

/** The window executable named by the environment or found on this machine. */
export function findApp(env: NodeJS.ProcessEnv): string {
    if (env.HAMLET_APP) return env.HAMLET_APP;
    const repo = fileURLToPath(new URL("../../../", import.meta.url));
    const candidates = [
        ...(env.PATH?.split(delimiter) ?? []).map((dir) =>
            resolve(dir, "hamlet-app"),
        ),
        join(repo, "target", "release", "hamlet-app"),
        join(repo, "target", "debug", "hamlet-app"),
    ];
    for (const candidate of candidates) {
        try {
            accessSync(candidate, constants.X_OK);
            if (statSync(candidate).isFile()) return candidate;
        } catch {}
    }
    throw new Error(
        "No hamlet-app found. Build it with cargo build --bin hamlet-app, or set HAMLET_APP.",
    );
}

/** A window process with its server's token confined to its environment (S4). */
export function launch(
    app: string,
    options: ServerAddress & {
        view?: string;
        env?: Readonly<NodeJS.ProcessEnv>;
        io?: CommandIO;
    },
) {
    const child = spawn(app, [], {
        stdio: options.io ? "pipe" : "inherit",
        env: {
            ...(options.env ?? process.env),
            HAMLET_SERVER: options.url,
            HAMLET_TOKEN: options.token,
            ...(options.view === undefined
                ? {}
                : { HAMLET_VIEW: options.view }),
        },
    });
    if (options.io) {
        const { stdin, stdout, stderr } = child;
        if (stdin) {
            options.io.stdin.pipe(stdin);
            const input = options.io.stdin;
            child.once("close", () => input.unpipe(stdin));
        }
        stdout?.pipe(options.io.stdout, { end: false });
        stderr?.pipe(options.io.stderr, { end: false });
    }
    return child;
}

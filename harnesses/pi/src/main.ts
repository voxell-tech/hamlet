#!/usr/bin/env -S node --disable-warning=ExperimentalWarning
// A pi harness runtime, started by the Hamlet server:
// main.ts --runtime <id> --store <file> --transport stdio [--fresh] [--faux <script>]
import { Console } from "node:console";
import { Channel } from "./channel.ts";
import { run } from "./runtime.ts";

// Standard output carries the runtime protocol alone.
globalThis.console = new Console({
    stdout: process.stderr,
    stderr: process.stderr,
});

try {
    const args = process.argv.slice(2);
    const named = new Map<string, string>();
    let fresh = false;
    for (let i = 0; i < args.length; i++) {
        const flag = args[i];
        if (flag === "--fresh") {
            fresh = true;
            continue;
        }
        const value = args[++i];
        if (flag === undefined || value === undefined)
            throw new Error(`${flag} needs a value`);
        if (!["--runtime", "--store", "--transport", "--faux"].includes(flag))
            throw new Error(`unknown argument ${flag}`);
        named.set(flag, value);
    }
    const runtime = named.get("--runtime");
    const store = named.get("--store");
    if (runtime === undefined || store === undefined)
        throw new Error("--runtime and --store are needed");
    if (named.get("--transport") !== "stdio")
        throw new Error(
            "--transport stdio is the only transport this runtime speaks",
        );
    await run(
        { runtime, store, fresh, faux: named.get("--faux") },
        new Channel(process.stdin, process.stdout),
    );
} catch (error) {
    process.stderr.write(
        `hamlet-pi: ${error instanceof Error ? error.message : String(error)}\n`,
    );
    process.exit(1);
}

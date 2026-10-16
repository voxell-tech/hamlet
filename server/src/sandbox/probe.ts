import { spawnSync } from "node:child_process";
import { platformSandbox, type Sandbox } from "./platform.ts";

/** Checks that `sandbox` works on this machine, and throws with what it needs where it does not (C18). */
export function probeSandbox(sandbox: Sandbox = platformSandbox()): void {
    const {
        argv: [program = "", ...args],
        hint,
    } = sandbox.probe;
    const ran = spawnSync(program, args, { encoding: "utf8" });
    if (ran.error) {
        throw new Error(
            `failed to run ${program}: ${ran.error.message}. ${hint}`,
        );
    }
    if (ran.status !== 0) {
        throw new Error(
            `${sandbox.name} probe failed (${ran.status ?? ran.signal}): ${ran.stderr.trim()}. ${hint}`,
        );
    }
}

import { Bwrap } from "./bwrap.ts";
import type { Policy } from "./policy.ts";
import { Seatbelt } from "./seatbelt.ts";

/** A command confined to a policy: the program to run and its whole environment. */
export type Confined = {
    argv: string[];
    env: Record<string, string>;
};

/** A way to confine a command on one platform. */
export interface Sandbox {
    readonly name: string;
    /** A command that succeeds only where the sandbox works, and what to do where it fails. */
    readonly probe: { argv: string[]; hint: string };
    /** The environment tools start from. */
    baseEnv(): Record<string, string>;
    /** `argv`, run from `cwd`, confined to `policy`, with `tmp` as its temp folder. */
    command(
        policy: Policy,
        argv: readonly string[],
        cwd: string,
        tmp: string,
    ): Confined;
}

/** The sandbox of this machine's platform (C18). */
export function platformSandbox(): Sandbox {
    switch (process.platform) {
        case "darwin":
            return Seatbelt;
        case "linux":
            return Bwrap;
        default:
            throw new Error(
                `There is no sandbox for ${process.platform}, so tools cannot run here.`,
            );
    }
}

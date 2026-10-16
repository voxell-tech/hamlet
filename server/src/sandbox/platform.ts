import { Bwrap } from "./bwrap.ts";
import type { Limits, Policy } from "./policy.ts";
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

/** `argv`, run by bash once `limits` hold for it and for every process it starts. */
export function limited(limits: Limits, argv: readonly string[]): string[] {
    return underLimits(
        [
            ["t", limits.cpuSecs],
            ["u", limits.maxProcs],
            ["f", limits.maxFileMb * 1024],
            ["c", 0],
        ],
        argv,
    );
}

/** File and core limits for a persistent helper, without a command's CPU or process budget. */
export function fileLimited(limits: Limits, argv: readonly string[]): string[] {
    return underLimits(
        [
            ["f", limits.maxFileMb * 1024],
            ["c", 0],
        ],
        argv,
    );
}

function underLimits(
    limits: readonly (readonly [string, number])[],
    argv: readonly string[],
): string[] {
    // bash counts file sizes in KiB. A lower machine limit stays in force.
    const script = limits
        .map(
            ([flag, value]) =>
                `ulimit -${flag} ${value} 2>/dev/null || ulimit -${flag} hard`,
        )
        .join("\n");
    return ["bash", "-c", `${script}\nexec "$@"`, "bash", ...argv];
}

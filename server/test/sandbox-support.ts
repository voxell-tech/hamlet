import type { Limits } from "../src/sandbox/policy.ts";
import { closedPolicy } from "../src/sandbox/reach.ts";
import { SandboxEnv } from "../src/sandbox.ts";
import { scratch } from "./support.ts";

/** An environment on a fresh house, with `limits` changed from the defaults. */
export function houseEnv(limits?: Partial<Limits>): {
    house: string;
    env: SandboxEnv;
} {
    const house = scratch();
    const policy = closedPolicy(house);
    return {
        house,
        env: new SandboxEnv({
            cwd: house,
            policy: limits === undefined ? policy : { ...policy, limits },
        }),
    };
}

import { platformSandbox } from "./platform.ts";
import { hostEnv, type Policy } from "./policy.ts";

/** An agent's policy while its hamlet works on no folder: its house, which is its home, and the shared skills to read (C18). */
export function closedPolicy(house: string, shared: string[] = []): Policy {
    return {
        ro: shared,
        rw: [house],
        open: false,
        hidden: [],
        locked: [],
        net: false,
        services: [],
        env: { ...platformSandbox().baseEnv(), HOME: house },
    };
}

/** An agent's policy while its hamlet works on a folder: the machine to read but Hamlet's home, the folder and its house to write (C18, C20). */
export function openPolicy(options: {
    folder: string;
    house: string;
    home: string;
    shared?: string[];
}): Policy {
    return {
        ro: options.shared ?? [],
        rw: [options.folder, options.house],
        open: true,
        hidden: [options.home],
        locked: [],
        net: false,
        services: [],
        // The chief's own, so the project's tools are found.
        env: hostEnv(),
    };
}

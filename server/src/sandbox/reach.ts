import { join } from "node:path";
import { hostEnv, isDir, type Policy } from "./policy.ts";
import { platformSandbox } from "./sandbox.ts";

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
    const git = join(options.folder, ".git");
    return {
        ro: options.shared ?? [],
        rw: [options.folder, options.house],
        open: true,
        hidden: [options.home],
        // A hook or a setting written here runs later with no sandbox. Where
        // there is no repository none can be made, since it would come with
        // both.
        locked: isDir(git) ? [join(git, "hooks"), join(git, "config")] : [git],
        net: false,
        services: [],
        // The chief's own, so the project's tools are found.
        env: hostEnv(),
    };
}

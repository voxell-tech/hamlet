import { existsSync } from "node:fs";
import type { Confined, Sandbox } from "./platform.ts";
import {
    aboveRoots,
    fixedRoots,
    isDir,
    tempRoot,
    within,
    writeRoots,
} from "./policy.ts";

/** Linux bubblewrap. */
export const Bwrap: Sandbox = {
    name: "bwrap",
    probe: {
        argv: ["bwrap", "--unshare-all", "--ro-bind", "/", "/", "true"],
        hint: "bubblewrap is required for sandboxing. Install it (e.g. `apt install bubblewrap`). On Ubuntu 24.04+, AppArmor restricts unprivileged user namespaces; allow bwrap via an AppArmor profile or set kernel.apparmor_restrict_unprivileged_userns=0",
    },
    baseEnv: () => ({
        PATH: "/usr/local/bin:/usr/bin:/bin",
        HOME: "/tmp",
        LANG: "C.UTF-8",
    }),
    command(policy, argv, cwd, tmp): Confined {
        const args = ["bwrap", "--unshare-all"];
        if (policy.net) args.push("--share-net");
        args.push("--die-with-parent", "--new-session");
        if (policy.open) {
            args.push("--ro-bind", "/", "/");
        } else {
            args.push("--ro-bind", "/usr", "/usr");
            for (const path of ["/bin", "/sbin", "/lib", "/lib64"]) {
                args.push("--ro-bind-try", path, path);
            }
            const etc = [
                "alternatives",
                "ssl",
                "ca-certificates",
                "passwd",
                "group",
                "localtime",
                "ld.so.cache",
                "hosts",
            ];
            if (policy.net) etc.push("resolv.conf");
            for (const name of etc) {
                args.push("--ro-bind-try", `/etc/${name}`, `/etc/${name}`);
            }
        }
        args.push("--proc", "/proc", "--dev", "/dev");
        if (!policy.open) args.push("--tmpfs", "/tmp");
        if (policy.open) {
            // An empty folder over each. One that is missing has nothing to
            // hide and cannot be mounted over.
            for (const path of [...policy.hidden, tempRoot()].filter(isDir)) {
                args.push("--tmpfs", path);
            }
        }
        for (const path of policy.ro) args.push("--ro-bind", path, path);
        for (const path of writeRoots(policy, tmp))
            args.push("--bind", path, path);
        // Mount points keep roots and their writable ancestors in place.
        for (const path of [
            ...new Set([
                ...aboveRoots(policy, tmp),
                ...fixedRoots(policy, tmp),
            ]),
        ].sort((a, b) => a.length - b.length)) {
            if (
                existsSync(path) &&
                writeRoots(policy, tmp).some(
                    (rw) => path !== rw && within(path, rw),
                )
            )
                args.push("--bind", path, path);
        }
        for (const path of policy.locked) {
            args.push("--ro-bind-try", path, path);
        }
        args.push("--chdir", cwd, "--", ...argv);
        return { argv: args, env: { ...policy.env, TMPDIR: tmp } };
    },
};

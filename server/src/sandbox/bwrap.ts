import { opendirSync } from "node:fs";
import { join } from "node:path";
import { aboveLocked, isDir, type Service } from "./policy.ts";
import type { Confined, Sandbox } from "./sandbox.ts";

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
        const locked = [
            ...new Set([
                ...policy.locked,
                ...(policy.open ? gitLocks(policy.rw) : []),
            ]),
        ];
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
        args.push("--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp");
        if (policy.open) {
            // An empty folder over each. One that is missing has nothing to
            // hide and cannot be mounted over.
            for (const path of policy.hidden.filter(isDir)) {
                args.push("--tmpfs", path);
            }
        }
        for (const path of policy.ro) args.push("--ro-bind", path, path);
        for (const path of [...policy.rw, tmp]) args.push("--bind", path, path);
        // Linux refuses to move a mount point, and a folder that holds a
        // locked path would carry the lock away with it. A path that does
        // not exist yet cannot be mounted, so it is not guarded here.
        for (const path of aboveLocked({ ...policy, locked }).filter(isDir)) {
            args.push("--bind", path, path);
        }
        for (const path of locked) {
            args.push("--ro-bind-try", path, path);
        }
        // Without the network the command has a network of its own, so
        // each service's port in it is bridged to the service's socket.
        const bridged = policy.net ? [] : policy.services;
        for (const { socket } of bridged) args.push("--bind", socket, socket);
        args.push("--chdir", cwd, "--", ...bridge(bridged, argv));
        return { argv: args, env: { ...policy.env, TMPDIR: tmp } };
    },
};

const MAX_GIT_ENTRIES = 100_000;

/** Hooks and config of repositories present under `roots`, without following symbolic links. An incomplete scan refuses the command. */
function gitLocks(roots: readonly string[]): string[] {
    const locked: string[] = [];
    const pending = [...roots];
    let seen = 0;
    for (let path = pending.pop(); path !== undefined; path = pending.pop()) {
        const dir = opendirSync(path);
        try {
            for (let entry = dir.readSync(); entry; entry = dir.readSync()) {
                if (++seen > MAX_GIT_ENTRIES) {
                    throw new Error(
                        `Cannot guard nested repositories after ${MAX_GIT_ENTRIES} folder entries.`,
                    );
                }
                if (!entry.isDirectory()) continue;
                const child = join(path, entry.name);
                if (entry.name.toLowerCase() === ".git") {
                    locked.push(join(child, "hooks"), join(child, "config"));
                }
                pending.push(child);
            }
        } finally {
            dir.closeSync();
        }
    }
    return locked;
}

/** `argv`, run once each service's port is bridged to its socket. */
function bridge(
    services: readonly Service[],
    argv: readonly string[],
): string[] {
    if (services.length === 0) return [...argv];
    const script = [
        ...services.map(({ port, socket }) => {
            if (/[,:!"'\\\s]/.test(socket)) {
                throw new Error(`A service's socket cannot be at ${socket}.`);
            }
            return `socat TCP-LISTEN:${port},bind=127.0.0.1,fork,reuseaddr UNIX-CONNECT:${socket} >/dev/null 2>&1 &`;
        }),
        ...services.map(
            ({ port }) =>
                `i=0; until (: </dev/tcp/127.0.0.1/${port}) 2>/dev/null; do i=$((i+1)); [ "$i" -ge 500 ] && break; sleep 0.01; done`,
        ),
        'exec "$@"',
    ].join("\n");
    return ["bash", "-c", script, "bash", ...argv];
}

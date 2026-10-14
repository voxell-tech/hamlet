import { createHash } from "node:crypto";
import {
    chmodSync,
    existsSync,
    lstatSync,
    mkdirSync,
    readlinkSync,
    realpathSync,
    statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";

/** A service on this machine a command reaches even without the network: a local port, and the Unix socket that serves the same (S11). */
export type Service = {
    port: number;
    socket: string;
};

/** The reads, writes and reach of a sandboxed command. */
export type Policy = {
    /** Paths readable at the same location. */
    ro: string[];
    /** Paths readable and writable at the same location. */
    rw: string[];
    /** The whole machine can be read, but for `hidden`. */
    open: boolean;
    /** Paths that stay unreadable when `open`. A path of `ro` or `rw` inside one is still granted. */
    hidden: string[];
    /** Paths inside `rw` that cannot be written, renamed or removed. */
    locked: string[];
    net: boolean;
    services: Service[];
    /** The complete environment; everything else is cleared. */
    env: Record<string, string>;
};

/** The host variables a normal process needs. */
export function hostEnv(): Record<string, string> {
    const env: Record<string, string> = { LANG: "C.UTF-8" };
    for (const name of ["PATH", "HOME", "LANG"]) {
        const value = process.env[name];
        if (value !== undefined) env[name] = value;
    }
    return env;
}

/** True when `path` is `parent` or lies inside it. */
export function within(path: string, parent: string): boolean {
    return (
        path === parent ||
        path.startsWith(parent.endsWith(sep) ? parent : parent + sep)
    );
}

/** The real path of `path`. The part of it that does not exist yet is kept as written, after the real path of the part that does. */
export function canon(path: string): string {
    let known = path;
    while (!existsSync(known)) {
        const up = dirname(known);
        if (up === known) return path;
        known = up;
    }
    try {
        return join(realpathSync(known), relative(known, path));
    } catch {
        return path;
    }
}

/** The folder that holds every private temp dir. */
export function tempRoot(): string {
    return join(canon(tmpdir()), "hamlet-sandbox-tmp");
}

/** The private temp dir a sandboxed command may write to: one for each set of writable paths, so each house has its own. */
export function privateTmp(policy: Policy): string {
    const digest = createHash("sha256");
    for (const path of policy.rw) {
        digest.update(canon(path));
        digest.update("\0");
    }
    const dir = join(tempRoot(), digest.digest("hex").slice(0, 16));
    mkdirSync(dir, { recursive: true });
    chmodSync(dir, 0o700);
    return dir;
}

/** The real path of `path`, through a symbolic link at its end too. */
function target(path: string): string {
    try {
        if (lstatSync(path).isSymbolicLink()) {
            return canon(resolve(dirname(path), readlinkSync(path)));
        }
    } catch {}
    return canon(path);
}

function inAny(path: string, paths: readonly string[]): boolean {
    return paths.some((each) => within(path, canon(each)));
}

/** Whether `policy` lets a command with `tmp` as its temp folder read `path`. */
export function readable(policy: Policy, tmp: string, path: string): boolean {
    const real = target(path);
    if (inAny(real, [...policy.ro, ...policy.rw, tmp])) return true;
    return policy.open && !inAny(real, [...policy.hidden, tempRoot()]);
}

/** Whether `policy` lets a command with `tmp` as its temp folder write `path`. */
export function writable(policy: Policy, tmp: string, path: string): boolean {
    const real = target(path);
    return inAny(real, [...policy.rw, tmp]) && !inAny(real, policy.locked);
}

/** Whether moving or removing `path` would carry a locked path with it. */
export function holdsLocked(policy: Policy, path: string): boolean {
    const real = canon(path);
    return policy.locked.some((locked) => within(canon(locked), real));
}

/** Whether `path` is a directory. */
export function isDir(path: string): boolean {
    try {
        return statSync(path).isDirectory();
    } catch {
        return false;
    }
}

/** The folders inside `rw` that hold a locked path: moving one would carry the lock away. */
export function aboveLocked(policy: Policy): string[] {
    const above = new Set<string>();
    for (const locked of policy.locked) {
        for (let up = dirname(locked); up !== dirname(up); up = dirname(up)) {
            if (policy.rw.some((rw) => within(up, rw) && up !== rw)) {
                above.add(up);
            }
        }
    }
    return [...above].sort();
}

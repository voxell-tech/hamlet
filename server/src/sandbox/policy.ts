import { createHash } from "node:crypto";
import {
    chmodSync,
    lstatSync,
    mkdirSync,
    readlinkSync,
    realpathSync,
    statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, resolve, sep } from "node:path";

/** A service on this machine a command reaches even without the network: a local port, and the Unix socket that serves the same (S11). */
export type Service = {
    port: number;
    socket: string;
};

/** The resources one command may use. */
export type Limits = {
    cpuSecs: number;
    wallSecs: number;
    maxProcs: number;
    maxFileMb: number;
};

/** The limits of a command whose policy changes none. A command also never dumps core. */
const DEFAULT_LIMITS: Readonly<Limits> = {
    cpuSecs: 60,
    wallSecs: 120,
    maxProcs: 4096,
    maxFileMb: 512,
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
    /** Extra roots whose own location is fixed, without granting reads or writes. */
    fixed?: string[];
    net: boolean;
    services: Service[];
    /** The complete environment; everything else is cleared. */
    env: Record<string, string>;
    /** Those of {@link DEFAULT_LIMITS} this policy changes. */
    limits?: Partial<Limits>;
};

/** The longest wall time in seconds that the environment's timer takes. */
const MAX_WALL_SECS = 2_147_483;

/** The limits a command under `policy` runs with. Each must be a whole number, since it is written into the command, and at least 1, since at 0 a command is refused or killed at once. */
export function limitsOf(policy: Pick<Policy, "limits">): Limits {
    const limits = { ...DEFAULT_LIMITS, ...policy.limits };
    for (const [name, value] of Object.entries(limits)) {
        if (!Number.isSafeInteger(value) || value < 1) {
            throw new Error(
                `The limit ${name} is not a whole number of at least 1: ${value}`,
            );
        }
    }
    if (limits.wallSecs > MAX_WALL_SECS) {
        throw new Error(
            `The limit wallSecs is more than ${MAX_WALL_SECS} seconds: ${limits.wallSecs}`,
        );
    }
    return limits;
}

/** A short digest of `policy` as `sandbox` runs it, which names the policy in a command's record. */
export function policyHash(policy: Policy, sandbox: string): string {
    let text = "";
    const field = (tag: string, value: string | number | boolean) => {
        const shown = String(value);
        text += `${tag}=${Buffer.byteLength(shown)}:${shown}\n`;
    };
    field("sandbox", sandbox);
    for (const path of policy.ro) field("ro", path);
    for (const path of policy.rw) field("rw", path);
    if (policy.open) field("open", true);
    for (const path of policy.hidden) field("hidden", path);
    for (const path of policy.locked) field("locked", path);
    for (const path of policy.fixed ?? []) field("fixed", path);
    field("net", policy.net);
    for (const { port, socket } of policy.services) {
        field("port", port);
        field("socket", socket);
    }
    const env = Object.entries(policy.env).sort(([a], [b]) => (a < b ? -1 : 1));
    for (const [name, value] of env) {
        field("envk", name);
        field("envv", value);
    }
    const limits = limitsOf(policy);
    field("cpu", limits.cpuSecs);
    field("wall", limits.wallSecs);
    field("procs", limits.maxProcs);
    field("file", limits.maxFileMb);
    return createHash("sha256").update(text).digest("hex").slice(0, 12);
}

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

/** The most symbolic links Linux follows in one path, more than macOS does. */
const MAX_LINKS = 40;

/** The real path of `path`, where a file made at it lands, with each name in the case its volume keeps. Every symbolic link on the way is followed, also one whose chain ends at a missing file. The part that does not exist yet is kept as written. */
export function canon(path: string): string {
    const rest = resolve(path).split(sep);
    let real: string = sep;
    for (let links = 0; ; ) {
        const part = rest.shift();
        if (part === undefined) return cased(real);
        if (part === "" || part === ".") continue;
        // `real` holds no link, so its parent is the folder `..` leads to.
        if (part === "..") {
            real = dirname(real);
            continue;
        }
        const next = join(real, part);
        let link: string;
        try {
            if (!lstatSync(next).isSymbolicLink()) {
                real = next;
                continue;
            }
            link = readlinkSync(next);
        } catch {
            return join(cased(real), part, ...rest);
        }
        // The system refuses a path through more links, so it leads nowhere.
        if (++links > MAX_LINKS) return path;
        if (isAbsolute(link)) real = sep;
        rest.unshift(...link.split(sep));
    }
}

/** `real`, a path without symbolic links, with each name in the case its volume keeps. */
function cased(real: string): string {
    try {
        return realpathSync.native(real);
    } catch {
        return real;
    }
}

/** A directory root's canonical spelling. Links and special files are refused. */
export function directoryRoot(path: string): string {
    const absolute = resolve(path);
    try {
        if (!lstatSync(absolute).isDirectory())
            throw new Error(
                `The sandbox root must be a real directory: ${path}`,
            );
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    return canon(absolute);
}

/** The folder that holds every private temp dir. */
export function tempRoot(): string {
    return join(canon(tmpdir()), "hamlet-sandbox-tmp");
}

/** The private temp dir a sandboxed command may write to: one for each set of writable paths and `scope`, so each house, and each scope in it, has its own. */
export function privateTmp(policy: Policy, scope?: string): string {
    const digest = createHash("sha256");
    for (const path of policy.rw) {
        digest.update(canon(path));
        digest.update("\0");
    }
    // No path is empty, so a second NUL tells the scope from another path.
    if (scope !== undefined) digest.update(`\0${scope}`);
    const dir = join(tempRoot(), digest.digest("hex").slice(0, 16));
    directoryRoot(tempRoot());
    directoryRoot(dir);
    mkdirSync(dir, { recursive: true });
    directoryRoot(dir);
    chmodSync(dir, 0o700);
    return dir;
}

/** The writable roots of a policy, including its private temp folder. */
export function writeRoots(policy: Policy, tmp: string): string[] {
    return [...policy.rw, tmp];
}

/** Every granted or fixed root, including the private temp folder. */
export function fixedRoots(policy: Policy, tmp: string): string[] {
    return [
        ...new Set([
            ...writeRoots(policy, tmp),
            ...policy.ro,
            ...policy.locked,
            ...policy.hidden,
            ...policy.services.map(({ socket }) => socket),
            ...(policy.fixed ?? []),
        ]),
    ];
}

function inAny(path: string, paths: readonly string[]): boolean {
    return paths.some((each) => within(path, canon(each)));
}

/** True when `path` lies in any of `paths`, compared without case. A refusal uses it, so that a volume that ignores case cannot carry a path past one. */
function inAnyFolded(path: string, paths: readonly string[]): boolean {
    const folded = path.toLowerCase();
    return paths.some((each) => within(folded, canon(each).toLowerCase()));
}

/** True when `policy` lets a command with `tmp` as its temp folder read `path`. */
export function readable(policy: Policy, tmp: string, path: string): boolean {
    const real = canon(path);
    if (inAny(real, [...policy.ro, ...writeRoots(policy, tmp)])) return true;
    return policy.open && !inAnyFolded(real, [...policy.hidden, tempRoot()]);
}

/** True when `policy` lets a command with `tmp` as its temp folder write `path`, or with `move` also move or remove it. */
export function writable(
    policy: Policy,
    tmp: string,
    path: string,
    move = false,
): boolean {
    const real = canon(path);
    return (
        inAny(real, writeRoots(policy, tmp)) &&
        !inAnyFolded(real, policy.locked) &&
        !(
            move &&
            fixedRoots(policy, tmp).some((root) =>
                within(canon(root).toLowerCase(), real.toLowerCase()),
            )
        )
    );
}

/** True when `path` is a directory. */
export function isDir(path: string): boolean {
    try {
        return statSync(path).isDirectory();
    } catch {
        return false;
    }
}

/** Writable ancestors whose move would carry a fixed root away. */
export function aboveRoots(policy: Policy, tmp: string): string[] {
    const above = new Set<string>();
    const writes = writeRoots(policy, tmp).map(canon);
    for (const root of fixedRoots(policy, tmp)) {
        for (let up = dirname(root); up !== dirname(up); up = dirname(up)) {
            const real = canon(up);
            if (writes.some((rw) => within(real, rw) && real !== rw))
                above.add(up);
        }
    }
    return [...above].sort();
}

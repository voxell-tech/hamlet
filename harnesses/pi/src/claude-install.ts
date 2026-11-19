// The Claude program's packages, fetched from npm the first time Claude is used where pi was installed without them, as
// a release is (ADR 0014). Each is the file pi's lockfile names, checked against the lockfile's integrity, so a realm
// gets what `npm ci` would have installed.
import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    renameSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { registerHooks } from "node:module";
import { basename, dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";

/** A package as pi's `package-lock.json` keeps it. */
export interface Locked {
    version?: string;
    resolved?: string;
    integrity?: string;
    dev?: boolean;
    dependencies?: Record<string, string>;
    optionalDependencies?: Record<string, string>;
    peerDependencies?: Record<string, string>;
    os?: string[];
    cpu?: string[];
    libc?: string[];
}

/** The machine a package must fit. */
export interface Machine {
    os: string;
    cpu: string;
    libc?: string;
}

const PI = join(import.meta.dirname, "..");

/** pi's optional packages: the Claude program and what drives it. */
export function roots(): string[] {
    const manifest = JSON.parse(
        readFileSync(join(PI, "package.json"), "utf8"),
    ) as { optionalDependencies?: Record<string, string> };
    return Object.keys(manifest.optionalDependencies ?? {});
}

function lockfile(): Record<string, Locked> {
    const lock = JSON.parse(
        readFileSync(join(PI, "package-lock.json"), "utf8"),
    ) as { packages: Record<string, Locked> };
    return lock.packages;
}

/** This machine, with its C library on Linux, where the Claude program comes built for glibc or musl. */
export function machine(): Machine {
    if (process.platform !== "linux")
        return { os: process.platform, cpu: process.arch };
    const header = process.report.getReport() as {
        header?: { glibcVersionRuntime?: string };
    };
    return {
        os: process.platform,
        cpu: process.arch,
        libc:
            header.header?.glibcVersionRuntime === undefined ? "musl" : "glibc",
    };
}

const PROGRAM = "@anthropic-ai/claude-agent-sdk-";

/** The Claude program's package names its C library only by a `-musl` suffix, not in the lockfile's `libc`. */
export function forLibc(name: string, libc: string | undefined): boolean {
    if (libc === undefined || !name.startsWith(`${PROGRAM}linux-`)) return true;
    return name.endsWith("-musl") === (libc === "musl");
}

function fits(key: string, entry: Locked, on: Machine): boolean {
    // npm's lists name what a package runs on, or with `!` what it does not.
    const allows = (list: string[] | undefined, value: string | undefined) => {
        if (list === undefined || value === undefined) return true;
        if (list.includes(`!${value}`)) return false;
        const named = list.filter((item) => !item.startsWith("!"));
        return named.length === 0 || named.includes(value);
    };
    const name = key.slice(key.lastIndexOf("node_modules/") + 13);
    return (
        allows(entry.os, on.os) &&
        allows(entry.cpu, on.cpu) &&
        allows(entry.libc, on.libc) &&
        forLibc(name, on.libc)
    );
}

/** Where `name`, needed by the package at `from`, is installed, as npm resolves it: nearest `node_modules` first. */
function place(
    packages: Record<string, Locked>,
    from: string,
    name: string,
): string | undefined {
    let at = from;
    for (;;) {
        const candidate = `${at === "" ? "" : `${at}/`}node_modules/${name}`;
        if (packages[candidate] !== undefined) return candidate;
        if (at === "") return undefined;
        const cut = at.lastIndexOf("/node_modules/");
        at = cut < 0 ? "" : at.slice(0, cut);
    }
}

/** Every package `names` need on `on`, by where the lockfile installs it. */
export function needed(
    packages: Record<string, Locked>,
    names: string[],
    on: Machine,
): [string, Locked][] {
    const found = new Map<string, Locked>();
    const visit = (from: string, name: string, optional: boolean) => {
        const key = place(packages, from, name);
        if (key === undefined) {
            if (optional) return;
            throw new Error(`pi's lockfile has no ${name}`);
        }
        const entry = packages[key] as Locked;
        if (found.has(key) || !fits(key, entry, on)) return;
        found.set(key, entry);
        for (const dependency of Object.keys(entry.dependencies ?? {}))
            visit(key, dependency, false);
        // A peer the lockfile does not install, as with legacy peer handling, is not needed.
        for (const dependency of Object.keys({
            ...entry.optionalDependencies,
            ...entry.peerDependencies,
        }))
            visit(key, dependency, true);
    };
    for (const name of names) visit("", name, false);
    return [...found];
}

/** Whether pi's lockfile names a Claude program built for this machine. */
export function downloadable(): boolean {
    try {
        const on = machine();
        return needed(lockfile(), roots(), on).some(
            ([key, entry]) =>
                key.startsWith(`node_modules/${PROGRAM}`) &&
                entry.os !== undefined,
        );
    } catch {
        return false;
    }
}

/** The folder in `home` the packages of this pi's lockfile go to. */
function folder(home: string): string {
    const sdk = lockfile()["node_modules/@anthropic-ai/claude-agent-sdk"];
    return join(home, "claude-program", sdk?.version ?? "unknown");
}

/** The folder holding the downloaded packages, once all are there. */
export function downloaded(home: string): string | undefined {
    const at = folder(home);
    return existsSync(join(at, "complete")) ? at : undefined;
}

/** How long a download may go without a byte before it fails. */
const STALL_MS = 60_000;

/** A download under way, shared by every caller, with whom it reports to and its last report. */
interface Running {
    done: Promise<string>;
    listeners: Set<(message: string) => void>;
    last?: string;
}

const downloads = new Map<string, Running>();

/**
 * Downloads the packages into `home` unless they are there, reporting each step to `progress`. Callers share one
 * download; `signal` stops only this caller's wait, so another's call or sign-in still gets the program.
 */
export function download(
    home: string,
    progress: (message: string) => void,
    signal?: AbortSignal,
): Promise<string> {
    const at = folder(home);
    const ready = downloaded(home);
    if (ready !== undefined) return Promise.resolve(ready);
    let running = downloads.get(at);
    if (running === undefined) {
        const started: Running = {
            done: Promise.resolve(at),
            listeners: new Set(),
        };
        started.done = fetchAll(at, (message) => {
            started.last = message;
            for (const listener of started.listeners) listener(message);
        }).finally(() => downloads.delete(at));
        downloads.set(at, started);
        running = started;
    }
    if (running.last !== undefined) progress(running.last);
    const { listeners, done } = running;
    // Each wait has its own listener, so one caller's progress may be another's too.
    const listener = (message: string) => progress(message);
    listeners.add(listener);
    return new Promise<string>((resolve, reject) => {
        const leave = () => {
            listeners.delete(listener);
            signal?.removeEventListener("abort", stop);
        };
        const stop = () => {
            leave();
            reject(signal?.reason);
        };
        if (signal?.aborted) return stop();
        signal?.addEventListener("abort", stop, { once: true });
        done.then(resolve, reject).finally(leave);
    });
}

async function fetchAll(
    at: string,
    progress: (message: string) => void,
): Promise<string> {
    const packages = needed(lockfile(), roots(), machine());
    // Built beside the folder and moved in whole, so a stopped download leaves no half-made folder.
    const building = `${at}.${randomUUID()}`;
    mkdirSync(building, { recursive: true });
    try {
        for (const [index, [key, entry]] of packages.entries()) {
            progress(
                `Downloading Claude's program from npm, once: ${index + 1} of ${packages.length}.`,
            );
            await unpack(key, entry, building);
        }
        writeFileSync(join(building, "complete"), "");
        if (!existsSync(at)) renameSync(building, at);
    } finally {
        rmSync(building, { recursive: true, force: true });
    }
    removeOthers(at);
    return at;
}

/** The file at `url`, failing when no byte comes for `STALL_MS`. */
async function fetchFile(key: string, url: string): Promise<Buffer> {
    const stop = new AbortController();
    const timer = setTimeout(
        () => stop.abort(new Error(`Downloading ${key} stalled`)),
        STALL_MS,
    );
    try {
        const response = await fetch(url, { signal: stop.signal });
        if (!response.ok || response.body === null)
            throw new Error(
                `Downloading ${key} failed: ${response.status} ${response.statusText}`,
            );
        const chunks: Uint8Array[] = [];
        for await (const chunk of response.body) {
            chunks.push(chunk);
            timer.refresh();
        }
        return Buffer.concat(chunks);
    } finally {
        clearTimeout(timer);
    }
}

async function unpack(key: string, entry: Locked, into: string): Promise<void> {
    if (entry.resolved === undefined || entry.integrity === undefined)
        throw new Error(`pi's lockfile gives no file for ${key}`);
    const file = await fetchFile(key, entry.resolved);
    const [algorithm, digest] = entry.integrity.split(" ")[0]?.split("-") ?? [];
    if (
        algorithm === undefined ||
        digest === undefined ||
        createHash(algorithm).update(file).digest("base64") !== digest
    )
        throw new Error(`${key} is not the file pi's lockfile names`);
    const archive = join(into, `${randomUUID()}.tgz`);
    writeFileSync(archive, file);
    const destination = join(into, key);
    mkdirSync(destination, { recursive: true });
    try {
        // An npm package is a gzipped tar with one top folder.
        await promisify(execFile)("tar", [
            "-xzf",
            archive,
            "-C",
            destination,
            "--strip-components=1",
        ]);
    } finally {
        rmSync(archive, { force: true });
    }
}

/** Removes the packages of other lockfiles, left by an older Hamlet. */
function removeOthers(at: string): void {
    const parent = dirname(at);
    const kept = basename(at);
    // Another server of the realm may still be building this same folder.
    for (const name of readdirSync(parent))
        if (name !== kept && !name.startsWith(`${kept}.`))
            rmSync(join(parent, name), { recursive: true, force: true });
}

let hooked = false;

/** Lets pi's imports that do not resolve from pi resolve from the downloaded packages in `at`. */
export function useDownloaded(at: string): void {
    if (hooked) return;
    hooked = true;
    // Resolved as from a file in `at`, which looks in its node_modules.
    const base = pathToFileURL(join(at, "index.js")).href;
    registerHooks({
        resolve: (specifier, context, next) => {
            try {
                return next(specifier, context);
            } catch (error) {
                const code = (error as { code?: string }).code;
                if (
                    code !== "ERR_MODULE_NOT_FOUND" &&
                    code !== "MODULE_NOT_FOUND"
                )
                    throw error;
                return next(specifier, { ...context, parentURL: base });
            }
        },
    });
}

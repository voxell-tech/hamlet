import { randomUUID } from "node:crypto";
import { lstat, mkdir, mkdtemp, readdir, rename, rm } from "node:fs/promises";
import {
    basename,
    dirname,
    extname,
    isAbsolute,
    join,
    relative,
    resolve,
    sep,
    win32,
} from "node:path";
import type { Context } from "@earendil-works/chord";
import type { Home } from "./home.ts";
import { MAX_GIFT_BYTES } from "./protocol.ts";
import { SandboxEnv } from "./sandbox/env.ts";
import { canon, directoryRoot, within } from "./sandbox/policy.ts";
import { closedPolicy } from "./sandbox/reach.ts";

const deliveries = new WeakMap<Home, Promise<unknown>>();
const REFUSED =
    "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.";

const MAX_GIFT_FILES = 4096;
const TREE_REFUSED = "The tree operation could not finish safely.";

/** Safe, distinct inbox paths and their total byte reservation (H25). */
export function manifestSize(files: { path: string; size: number }[]): number {
    if (files.length === 0)
        throw new Error("Choose a file or folder to hand over.");
    if (files.length > MAX_GIFT_FILES)
        throw new Error(
            `A gift takes at most ${MAX_GIFT_FILES} files at once.`,
        );
    let size = 0;
    const paths = new Set<string>();
    for (const file of files) {
        const parts = file.path.split("/");
        if (
            isAbsolute(file.path) ||
            win32.isAbsolute(file.path) ||
            file.path.includes("\\") ||
            file.path.includes("\0") ||
            parts.some(
                (part) =>
                    part === "" ||
                    part === "." ||
                    part === ".." ||
                    Buffer.byteLength(part) > 255,
            )
        )
            throw new Error(
                "A gift's path must be relative to the inbox, with ordinary file and folder names.",
            );
        size += file.size;
        if (size > MAX_GIFT_BYTES)
            throw new Error(
                "The gift is larger than the 32 MB an inbox takes at once.",
            );
        // Uppercasing expands folds such as sharp s; the final NFC joins decomposed names.
        const folded = file.path
            .normalize("NFC")
            .toLowerCase()
            .toUpperCase()
            .toLowerCase()
            .normalize("NFC");
        if (paths.has(folded))
            throw new Error(
                "The gift repeats a path, ignoring case and Unicode spelling.",
            );
        paths.add(folded);
    }
    for (const path of paths) {
        for (
            let parent = dirname(path);
            parent !== ".";
            parent = dirname(parent)
        ) {
            if (paths.has(parent))
                throw new Error(
                    "The gift uses one path as both a file and a folder.",
                );
        }
    }
    return size;
}

/** An inbox or skills shelf with no linked roots, including a shelf not yet made. */
export async function checkShelf(
    house: string,
    shelf: string,
    roots: string[],
): Promise<void> {
    const dir = join(house, shelf);
    const info = await lstat(dir).catch(() => undefined);
    if (info !== undefined && !info.isDirectory()) throw new Error(REFUSED);
    for (const root of roots) {
        if (
            (
                await lstat(join(dir, root)).catch(() => undefined)
            )?.isSymbolicLink()
        )
            throw new Error(REFUSED);
    }
}

/** Caps on a staged tree, including its root entry. */
export type TreeCaps = { entries: number; bytes: number };
/** The inbox and library's tree caps (H25). */
export const GIFT_CAPS: Readonly<TreeCaps> = {
    entries: MAX_GIFT_FILES,
    bytes: MAX_GIFT_BYTES,
};
/** A merge's tree caps. */
export const MERGE_CAPS: Readonly<TreeCaps> = {
    entries: 20000,
    bytes: 512 * 1024 * 1024,
};

class TreeError extends Error {
    readonly why: "entries" | "bytes" | "unsafe";

    constructor(why: "entries" | "bytes" | "unsafe", message: string) {
        super(message);
        this.why = why;
    }
}

/** A no-link skill tree of regular files with bounded bytes and entry count. */
export async function checkSkill(dir: string): Promise<void> {
    try {
        await checkTree(dir, GIFT_CAPS);
    } catch (error) {
        throw skillError(error);
    }
}

async function checkTree(
    dir: string,
    caps?: Readonly<TreeCaps>,
): Promise<void> {
    let bytes = 0,
        entries = 0;
    const walk = async (path: string): Promise<void> => {
        if (caps && ++entries > caps.entries)
            throw new TreeError("entries", "The tree exceeds its entries cap.");
        const info = await lstat(path);
        if (info.isDirectory())
            for (const name of await readdir(path))
                await walk(join(path, name));
        else if (info.isFile()) {
            bytes += info.size;
            if (caps && bytes > caps.bytes)
                throw new TreeError("bytes", "The tree exceeds its bytes cap.");
        } else
            throw new TreeError(
                "unsafe",
                "A tree cannot include links or special files.",
            );
    };
    await walk(dir);
}

function skillError(error: unknown): Error {
    if (!(error instanceof TreeError)) return new Error(REFUSED);
    return new Error(
        error.why === "entries"
            ? `A skill gift takes at most ${MAX_GIFT_FILES} files and folders at once.`
            : error.why === "bytes"
              ? "The skill gift is larger than 32 MB."
              : "A skill gift cannot include links or special files.",
    );
}

/** A giver's skill copied into a server-owned stage under the giver's house read policy. */
export async function stageSkill(
    home: Home,
    house: string,
    skill: string,
    stage: string,
    context: Context,
): Promise<void> {
    try {
        await stageTree(
            home,
            { root: house, rel: `skills/${skill}` },
            stage,
            GIFT_CAPS,
            context,
        );
    } catch (error) {
        throw skillError(error);
    }
}

/** A gift copied under the receiving house's kernel-enforced write policy. */
export async function deliver(
    home: Home,
    house: string,
    shelf: "inbox" | "skills",
    source: string,
    roots: string[],
    context: Context,
): Promise<string[]> {
    return serial(home, async () => {
        await checkShelf(house, shelf, roots);
        const targets: string[] = [];
        for (const root of roots) {
            let name = root;
            let number = 1;
            while (
                await lstat(join(house, shelf, name)).catch(() => undefined)
            ) {
                if (shelf === "skills")
                    throw new Error("The agent already has that skill.");
                const ext = extname(root);
                name = `${root.slice(0, root.length - ext.length)}-${++number}${ext}`;
            }
            targets.push(name);
        }
        await copyInto(
            home,
            house,
            shelf,
            source,
            roots,
            targets,
            GIFT_CAPS,
            context,
        );
        return targets;
    });
}

async function serial<T>(home: Home, work: () => Promise<T>): Promise<T> {
    const before = deliveries.get(home) ?? Promise.resolve();
    const next = before.catch(() => {}).then(work);
    deliveries.set(home, next);
    try {
        return await next;
    } finally {
        if (deliveries.get(home) === next) deliveries.delete(home);
    }
}

async function copyInto(
    home: Home,
    root: string,
    into: string,
    source: string,
    names: string[],
    targets: string[],
    caps: Readonly<TreeCaps>,
    context: Context,
): Promise<void> {
    const env = treeEnv(home, source, root, caps);
    const paths = targets.map((name) => join(root, into, name));
    let claimed: string | undefined;
    try {
        const journal = await env.createTempFile(
            { prefix: "claimed-" },
            context,
        );
        if (!journal.ok) throw new Error(REFUSED);
        claimed = journal.value;
        const copied = await env.exec(
            [
                "/bin/bash",
                "-c",
                `set -e
claimed=$1; dest=$2; shift 2
mkdir -p -- "$dest"
while [ "$#" -gt 0 ]; do
  source=$1; target=$2; shift 2
  copy=$source
  if [ -d "$source" ]; then
    mkdir -- "$target"
    copy="$source/."
  else
    set -C
    : > "$target"
    set +C
  fi
  printf '%s\\0' "$target" >> "$claimed"
  cp -RPp -- "$copy" "$target"
done`,
                "gift",
                claimed,
                join(root, into),
                ...names.flatMap((name, i) => [
                    join(source, name),
                    paths[i] ?? "",
                ]),
            ],
            undefined,
            context,
        );
        if (!copied.ok || copied.value.exitCode !== 0) throw new Error(REFUSED);
    } catch {
        if (claimed !== undefined) {
            const journal = await env.readTextFile(claimed, context);
            const made = journal.ok
                ? journal.value
                      .split("\0")
                      .filter((path) => paths.includes(path))
                : [];
            if (made.length > 0)
                await env.exec(
                    ["/bin/rm", "-rf", "--", ...made],
                    undefined,
                    context,
                );
        }
        throw new Error(REFUSED);
    } finally {
        await env.cleanup(context);
    }
}

/** A regular file listed without following links, relative to the source root. */
export type TreeFile = { path: string; size: number; sha256: string };
/** A tree's regular files and entries that cannot be copied. */
export type TreeListing = {
    files: TreeFile[];
    skipped: { path: string; kind: "link" | "special" }[];
};
type TreeReport = TreeListing & {
    limit?: "entries" | "bytes";
    failed?: boolean;
};

function treeRel(rel: string, whole = false): void {
    if (whole && rel === ".") return;
    if (
        !rel ||
        isAbsolute(rel) ||
        win32.isAbsolute(rel) ||
        rel.includes("\\") ||
        rel.includes("\0") ||
        rel
            .split("/")
            .some((part) => part === "" || part === "." || part === "..")
    )
        throw new Error(
            "A tree path must stay relative to its root, without dot or parent components.",
        );
}

function treeEnv(
    home: Home,
    root: string,
    stage?: string,
    caps: Readonly<TreeCaps> = MERGE_CAPS,
): SandboxEnv {
    directoryRoot(root);
    if (stage !== undefined) directoryRoot(stage);
    const envRoot = stage ?? root;
    const policy = closedPolicy(envRoot, [root, process.execPath]);
    policy.rw = stage === undefined ? [] : [stage];
    policy.env = { PATH: "/usr/bin:/bin", HOME: envRoot, LANG: "C.UTF-8" };
    policy.limits = {
        maxFileMb: Math.max(1, Math.ceil(caps.bytes / (1024 * 1024)) + 1),
        wallSecs: 120,
    };
    return new SandboxEnv({
        cwd: envRoot,
        policy,
        sandbox: home.sandbox,
        tmpScope: `trees-${randomUUID()}`,
    });
}

/** A no-follow copy into a private stage, using the source's basename or the whole root for `rel: "."`. */
export async function stageTree(
    home: Home,
    source: { root: string; rel: string },
    stage: string,
    caps: Readonly<TreeCaps>,
    context: Context,
): Promise<void> {
    source = { ...source, root: resolve(source.root) };
    stage = resolve(stage);
    treeRel(source.rel, true);
    if (
        ![caps.entries, caps.bytes].every(
            (n) => Number.isSafeInteger(n) && n >= 0,
        )
    )
        throw new Error(
            "Tree caps are whole counts of zero or more entries and bytes.",
        );
    if (
        within(canon(stage), canon(source.root)) ||
        within(canon(source.root), canon(stage))
    )
        throw new Error("A private stage must be separate from its source.");
    try {
        if (
            !(await lstat(stage)).isDirectory() ||
            (await readdir(stage)).length !== 0
        )
            throw new Error("The private stage must be an empty directory.");
        const env = treeEnv(home, source.root, stage, caps);
        let report: TreeReport;
        try {
            report = await runTree(
                env,
                [
                    "stage",
                    source.root,
                    join(source.root, source.rel),
                    stage,
                    String(caps.entries),
                    String(caps.bytes),
                ],
                context,
            );
        } finally {
            await env.cleanup(context);
        }
        if (report.limit)
            throw new TreeError(
                report.limit,
                `The tree exceeds its ${report.limit} cap.`,
            );
        if (report.skipped.length > 0)
            throw new TreeError(
                "unsafe",
                `A tree cannot include links or special files: ${report.skipped.map((e) => `${JSON.stringify(e.path)} (${e.kind})`).join(", ")}.`,
            );
        if (source.rel === ".") await checkTree(stage, caps);
        else await checkTree(join(stage, basename(source.rel)), caps);
    } catch (error) {
        if (error instanceof TreeError) throw error;
        throw new Error("The tree could not be staged safely.");
    }
}

/** A no-clobber delivery of named stage roots into the target, with rollback of its claims. */
export async function deliverTree(
    home: Home,
    target: { root: string; into: string },
    stage: string,
    names: string[],
    context: Context,
): Promise<void> {
    target = { ...target, root: resolve(target.root) };
    stage = resolve(stage);
    treeRel(target.into, true);
    for (const name of names) {
        treeRel(name);
        if (name.includes("/"))
            throw new Error("A delivery names top-level stage entries.");
    }
    if (new Set(names).size !== names.length)
        throw new Error("A delivery cannot repeat a stage entry.");
    return serial(home, async () => {
        try {
            let path = target.root;
            for (const part of target.into === "."
                ? []
                : target.into.split("/")) {
                path = join(path, part);
                const stat = await lstat(path).catch(
                    (error: NodeJS.ErrnoException) => {
                        if (error.code !== "ENOENT") throw error;
                        return undefined;
                    },
                );
                if (stat && !stat.isDirectory()) throw new Error(REFUSED);
            }
            for (const name of names) await checkTree(join(stage, name));
            await copyInto(
                home,
                target.root,
                target.into,
                stage,
                names,
                names,
                MERGE_CAPS,
                context,
            );
        } catch {
            throw new Error(
                "The tree could not be delivered inside its target. Nothing was handed over.",
            );
        }
    });
}

/** Regular files with sizes and SHA-256, and the links or special files left unread. */
export async function listTree(
    home: Home,
    root: string,
    rel: string,
    context: Context,
): Promise<TreeListing> {
    root = resolve(root);
    treeRel(rel, true);
    const report = await treeOperation(
        home,
        root,
        ["list", root, join(root, rel)],
        context,
    );
    return { files: report.files, skipped: report.skipped };
}

/** Paths removed under their root's sole write policy, with links removed as links. */
export async function removeTree(
    home: Home,
    root: string,
    rels: string[],
    context: Context,
): Promise<void> {
    root = resolve(root);
    for (const rel of rels) treeRel(rel);
    await treeOperation(
        home,
        root,
        ["remove", root, ...rels.map((rel) => join(root, rel))],
        context,
        root,
    );
}

/** A server-owned folder erased from houses or the librarian, with its regular bytes freed. */
export async function removeHouse(home: Home, dir: string): Promise<number> {
    const refused =
        "Only a server-owned folder strictly inside houses or the librarian can be erased.";
    if (!isAbsolute(dir) || dir.split(sep).includes(".."))
        throw new Error(refused);
    const absolute = resolve(dir);
    const roots = [home.layout.houses, home.layout.librarian];
    const root = roots.find(
        (root) => absolute !== resolve(root) && within(absolute, resolve(root)),
    );
    if (!root || !within(canon(dir), canon(root))) throw new Error(refused);
    try {
        let path = root;
        for (const part of relative(root, absolute).split(sep)) {
            path = join(path, part);
            if (!(await lstat(path)).isDirectory()) throw new Error(refused);
        }
        const erasing = home.layout.erasing;
        await mkdir(erasing, { recursive: true, mode: 0o700 });
        if (!(await lstat(erasing)).isDirectory()) throw new Error(refused);
        const slot = await mkdtemp(join(erasing, "house-"));
        const detached = join(slot, "tree");
        // Moving out of every house policy revokes lingering processes' writes.
        await rename(absolute, detached);
        const size = async (path: string): Promise<number> => {
            const info = await lstat(path);
            if (info.isFile()) return info.size;
            if (!info.isDirectory()) return 0;
            let bytes = 0;
            for (const name of await readdir(path))
                bytes += await size(join(path, name));
            return bytes;
        };
        const bytes = await size(detached);
        await rm(slot, { recursive: true, force: true });
        return bytes;
    } catch {
        throw new Error(refused);
    }
}

async function treeOperation(
    home: Home,
    root: string,
    args: string[],
    context: Context,
    writable?: string,
): Promise<TreeReport> {
    try {
        const env = treeEnv(home, root, writable);
        try {
            return await runTree(env, args, context);
        } finally {
            await env.cleanup(context);
        }
    } catch {
        throw new Error(TREE_REFUSED);
    }
}

async function runTree(
    env: SandboxEnv,
    args: string[],
    context: Context,
): Promise<TreeReport> {
    let output = "";
    const result = await env.exec(
        [process.execPath, "-e", TREE_SCRIPT, ...args],
        {
            onOutput: (text) => {
                output += text;
            },
        },
        context,
    );
    if (!result.ok || result.value.exitCode !== 0)
        throw new Error(TREE_REFUSED);
    try {
        const report = JSON.parse(output) as TreeReport;
        if (
            report.failed ||
            !Array.isArray(report.files) ||
            !Array.isArray(report.skipped)
        )
            throw new Error();
        return report;
    } catch {
        throw new Error(TREE_REFUSED);
    }
}

const TREE_SCRIPT = `
const fs = require("node:fs"), path = require("node:path"), crypto = require("node:crypto");
const [mode, root, source, stage, entryCap, byteCap] = process.argv.slice(1);
const report = { files: [], skipped: [] };
const rel = (p) => path.relative(root, p).split(path.sep).join("/") || ".";
let entries = 0, bytes = 0;
function parents(p) {
    const parts = path.relative(root, p).split(path.sep);
    let current = root;
    for (const part of parts.slice(0, -1)) {
        if (!part) continue;
        current = path.join(current, part);
        const info = fs.lstatSync(current);
        if (info.isSymbolicLink() || !info.isDirectory()) {
            report.skipped.push({ path: rel(current), kind: info.isSymbolicLink() ? "link" : "special" });
            return false;
        }
    }
    return true;
}
function visit(p, dest) {
    if (report.limit) return;
    if (mode === "stage" && ++entries > Number(entryCap)) { report.limit = "entries"; return; }
    const info = fs.lstatSync(p);
    if (info.isDirectory()) {
        if (dest && dest !== stage) fs.mkdirSync(dest, { mode: 0o700 });
        for (const name of fs.readdirSync(p).sort()) visit(path.join(p, name), dest && path.join(dest, name));
        if (dest) fs.chmodSync(dest, info.mode & 0o777);
    } else if (info.isFile()) {
        const file = fs.openSync(p, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
        let output;
        try {
            const opened = fs.fstatSync(file);
            if (!opened.isFile() || opened.dev !== info.dev || opened.ino !== info.ino) throw new Error();
            if (mode === "stage" && bytes + opened.size > Number(byteCap)) { report.limit = "bytes"; return; }
            if (dest) output = fs.openSync(dest, "wx", opened.mode & 0o777);
            const digest = mode === "list" ? crypto.createHash("sha256") : undefined, chunk = Buffer.alloc(128 * 1024);
            let length = 0, count;
            while ((count = fs.readSync(file, chunk, 0, chunk.length, null)) > 0) {
                bytes += count; length += count;
                if (mode === "stage" && bytes > Number(byteCap)) { report.limit = "bytes"; return; }
                digest?.update(chunk.subarray(0, count));
                if (output !== undefined) {
                    let written = 0;
                    while (written < count) written += fs.writeSync(output, chunk, written, count - written);
                }
            }
            if (output !== undefined) fs.fchmodSync(output, opened.mode & 0o777);
            if (mode === "list") report.files.push({ path: rel(p), size: length, sha256: digest.digest("hex") });
        } finally { if (output !== undefined) fs.closeSync(output); fs.closeSync(file); }
    } else report.skipped.push({ path: rel(p), kind: info.isSymbolicLink() ? "link" : "special" });
}
function remove(p) {
    let info;
    try { info = fs.lstatSync(p); } catch (error) { if (error.code === "ENOENT") return; throw error; }
    if (info.isDirectory()) {
        for (const name of fs.readdirSync(p)) remove(path.join(p, name));
        fs.rmdirSync(p);
    } else fs.unlinkSync(p);
}
try {
    if (mode === "remove") {
        for (const p of process.argv.slice(3)) {
            if (parents(p)) remove(p);
        }
        if (report.skipped.length) report.failed = true;
    } else if (parents(source)) {
        visit(source, mode === "stage" ? source === root ? stage : path.join(stage, path.basename(source)) : undefined);
    }
} catch { report.failed = true; }
process.stdout.write(JSON.stringify(report));
`;

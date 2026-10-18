import { createHash } from "node:crypto";
import { constants } from "node:fs";
import {
    chmod,
    lstat,
    mkdir,
    mkdtemp,
    open,
    readdir,
    rename,
    rm,
} from "node:fs/promises";
import {
    basename,
    extname,
    isAbsolute,
    join,
    posix,
    relative,
    resolve,
    sep,
    win32,
} from "node:path";
import { fileURLToPath } from "node:url";
import type { Context } from "@earendil-works/chord";
import type { FileError } from "@earendil-works/pi-durable/env";
import { type Static, Type } from "typebox";
import { Value } from "typebox/value";
import { treeWorker } from "../environment.ts";
import type { Home } from "../home.ts";
import { MAX_GIFT_BYTES } from "../protocol.ts";
import { err, ok, type Result } from "../result.ts";
import { canon, within } from "../sandbox/policy.ts";
import type { SandboxEnv } from "../sandbox.ts";
import type { WorkspaceEnv, WorkspaceTreeEntry } from "../workspace.ts";
import {
    type HousePlace,
    REMOTE_FILE_BYTES,
    REMOTE_HOUSE_ROOT,
    type RemoteHouse,
    remoteHouse,
} from "./remote.ts";

/** Caps include a staged tree's root entry. */
export type TreeCaps = { entries: number; bytes: number };
/** Inbox and skill gift caps (H25). */
export const GIFT_CAPS: Readonly<TreeCaps> = {
    entries: 4096,
    bytes: MAX_GIFT_BYTES,
};
/** A merge's bounded tree. */
export const MERGE_CAPS: Readonly<TreeCaps> = {
    entries: 20000,
    bytes: 512 * 1024 * 1024,
};

const Report = Type.Object({
    files: Type.Array(
        Type.Object({
            path: Type.String(),
            size: Type.Number(),
            sha256: Type.String(),
        }),
    ),
    skipped: Type.Array(
        Type.Object({
            path: Type.String(),
            kind: Type.Union([Type.Literal("link"), Type.Literal("special")]),
        }),
    ),
    limit: Type.Optional(
        Type.Union([Type.Literal("entries"), Type.Literal("bytes")]),
    ),
    failed: Type.Optional(Type.Boolean()),
});
type TreeReport = Static<typeof Report>;
/** Unsafe entries are reported without reading their contents. */
export type TreeListing = Pick<TreeReport, "files" | "skipped">;
type Operation =
    | "stage-tree"
    | "deliver-tree"
    | "list-tree"
    | "remove-tree"
    | "erase-house";
/**
 * Why work on a remote house stopped: its path names no single house, whole-house erasure is not supported, or the
 * target was changed in part or may have been changed.
 */
type RemoteStop = "unmappable" | "erase-unsupported" | "partial" | "uncertain";
/** A tree refusal may follow partial work; inspect its target before retrying. */
export type TreeFailure = { operation: Operation; retrySafe: false } & (
    | {
          kind: "entries-cap" | "bytes-cap";
          cap: number;
          /** Present when `cap` is the per-file remote transfer limit rather than the whole tree's cap. */
          scope?: "file";
      }
    | { kind: "unsafe-entry"; skipped: TreeListing["skipped"] }
    | {
          kind: "stage-not-empty" | "stage-overlaps-source" | "shelf-linked";
      }
    /** `remote` is present when a remote house's state is why the work stopped. */
    | { kind: "refused"; remote?: RemoteStop }
    | {
          kind: "path-invalid";
          path: string;
          constraint: "relative" | "top-level" | "distinct" | "caps";
      }
    | { kind: "clobber"; name: string }
);

/** Safe wording for a tree operation's classified refusal. */
export function treeFailureText(error: TreeFailure): string {
    switch (error.kind) {
        case "entries-cap":
            return "The tree exceeds its entries cap.";
        case "bytes-cap":
            return error.scope === "file"
                ? `A file in the tree is larger than ${error.cap / (1024 * 1024)} MiB, the most a remote house transfers in one file. Nothing was truncated; make the file smaller or move it another way.`
                : "The tree exceeds its bytes cap.";
        case "unsafe-entry":
            return `A tree cannot include links or special files: ${error.skipped.map((e) => `${JSON.stringify(e.path)} (${e.kind})`).join(", ")}.`;
        case "stage-not-empty":
            return "The private stage must be an empty directory.";
        case "stage-overlaps-source":
            return "A private stage must be separate from its source.";
        case "clobber":
            return "The tree could not be delivered inside its target. Nothing was handed over.";
        case "shelf-linked":
            return "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.";
        case "path-invalid": {
            switch (error.constraint) {
                case "relative":
                    return "A tree path must stay relative to its root, without dot or parent components.";
                case "top-level":
                    return "A delivery names top-level stage entries.";
                case "distinct":
                    return "A delivery cannot repeat a stage entry.";
                case "caps":
                    return "Tree caps are whole counts of zero or more entries and bytes.";
            }
            const constraint: never = error.constraint;
            throw new Error(`Unclassified tree constraint: ${constraint}`);
        }
        case "refused": {
            if (error.remote !== undefined) return remoteStopText(error.remote);
            switch (error.operation) {
                case "stage-tree":
                    return "The tree could not be staged safely.";
                case "deliver-tree":
                    return "The tree could not be delivered inside its target. Nothing was handed over.";
                case "erase-house":
                    return "Only a server-owned folder strictly inside houses or the librarian can be erased.";
                case "list-tree":
                case "remove-tree":
                    return "The tree operation could not finish safely.";
            }
            const operation: never = error.operation;
            throw new Error(`Unclassified tree operation: ${operation}`);
        }
    }
    const unhandled: never = error;
    throw new Error(`Unclassified tree failure: ${unhandled}`);
}
function remoteStopText(stop: RemoteStop): string {
    switch (stop) {
        case "unmappable":
            return "The path does not name one agent's remote house, so it was neither read nor changed. Report the failure with the agent's hamlet and number.";
        case "erase-unsupported":
            return "Erasing a whole remote house is not supported yet. The house was left as it is in its workspace.";
        case "partial":
            return "The tree operation stopped after changing part of its target. Inspect the target and keep or remove what is there before repeating it.";
        case "uncertain":
            return "The workspace did not confirm the tree operation, so part of it may have happened. Inspect the target before repeating it.";
    }
    const unhandled: never = stop;
    throw new Error(`Unclassified remote stop: ${unhandled}`);
}
function failure(
    operation: Operation,
    kind:
        | "refused"
        | "shelf-linked"
        | "stage-not-empty"
        | "stage-overlaps-source",
): Result<never, TreeFailure> {
    return err({ operation, kind, retrySafe: false });
}
function treeRel(
    path: string,
    operation: Operation,
    whole = false,
): Result<void, TreeFailure> {
    if (whole && path === ".") return ok(undefined);
    if (
        !path ||
        isAbsolute(path) ||
        win32.isAbsolute(path) ||
        path.includes("\\") ||
        path.includes("\0") ||
        path.split("/").some((part) => !part || part === "." || part === "..")
    )
        return err({
            kind: "path-invalid",
            path,
            constraint: "relative",
            operation,
            retrySafe: false,
        });
    return ok(undefined);
}
async function checkTree(
    path: string,
    operation: Operation,
    caps?: Readonly<TreeCaps>,
): Promise<Result<void, TreeFailure>> {
    const root = path;
    let entries = 0,
        bytes = 0;
    const walk = async (path: string): Promise<Result<void, TreeFailure>> => {
        if (caps && ++entries > caps.entries)
            return err({
                kind: "entries-cap",
                cap: caps.entries,
                operation,
                retrySafe: false,
            });
        const info = await lstat(path);
        if (info.isDirectory()) {
            for (const name of await readdir(path)) {
                const next = await walk(join(path, name));
                if (!next.ok) return next;
            }
        } else if (info.isFile()) {
            bytes += info.size;
            if (caps && bytes > caps.bytes)
                return err({
                    kind: "bytes-cap",
                    cap: caps.bytes,
                    operation,
                    retrySafe: false,
                });
        } else
            return err({
                kind: "unsafe-entry",
                operation,
                retrySafe: false,
                skipped: [
                    {
                        path: relative(root, path) || ".",
                        kind: info.isSymbolicLink() ? "link" : "special",
                    },
                ],
            });
        return ok(undefined);
    };
    try {
        return await walk(path);
    } catch {
        return failure(operation, "refused");
    }
}

function validCaps(
    caps: Readonly<TreeCaps>,
    operation: Operation,
): Result<void, TreeFailure> {
    if (
        ![caps.entries, caps.bytes].every(
            (n) => Number.isSafeInteger(n) && n >= 0,
        )
    )
        return err({
            operation,
            kind: "path-invalid",
            path: "caps",
            constraint: "caps",
            retrySafe: false,
        });
    return ok(undefined);
}

/** A no-follow copy into a separate empty host stage, keeping regular bytes and modes; a remote house source is read through its workspace. */
export async function stage(
    home: Home,
    source: { root: string; rel: string },
    staged: string,
    caps: Readonly<TreeCaps>,
    context: Context,
): Promise<Result<void, TreeFailure>> {
    const operation = "stage-tree";
    const from = place(home, source.root, operation);
    if (!from.ok) return from;
    const into = place(home, staged, operation);
    if (!into.ok) return into;
    // A stage is a server-owned transfer buffer on the host, never a house.
    if (into.value.kind === "remote") return failure(operation, "refused");
    if (from.value.kind === "remote") {
        const path = treeRel(source.rel, operation, true);
        if (!path.ok) return path;
        const valid = validCaps(caps, operation);
        if (!valid.ok) return valid;
        return remoteStage(from.value.house, source.rel, staged, caps, context);
    }
    source = { ...source, root: resolve(source.root) };
    staged = resolve(staged);
    const path = treeRel(source.rel, operation, true);
    if (!path.ok) return path;
    const valid = validCaps(caps, operation);
    if (!valid.ok) return valid;
    if (
        within(canon(staged), canon(source.root)) ||
        within(canon(source.root), canon(staged))
    )
        return failure(operation, "stage-overlaps-source");
    try {
        if (
            !(await lstat(staged)).isDirectory() ||
            (await readdir(staged)).length
        )
            return failure(operation, "stage-not-empty");
        const env = treeWorker(home, {
            root: source.root,
            stage: staged,
            caps,
        });
        let outcome: Result<TreeReport, TreeFailure>;
        try {
            outcome = await runTree(
                env,
                [
                    "stage",
                    source.root,
                    join(source.root, source.rel),
                    staged,
                    String(caps.entries),
                    String(caps.bytes),
                ],
                operation,
                context,
            );
        } finally {
            await env.cleanup(context);
        }
        if (!outcome.ok) return outcome;
        const report = outcome.value;
        if (report.limit)
            return err({
                kind: report.limit === "entries" ? "entries-cap" : "bytes-cap",
                cap: caps[report.limit],
                operation,
                retrySafe: false,
            });
        if (report.skipped.length)
            return err({
                kind: "unsafe-entry",
                skipped: report.skipped,
                operation,
                retrySafe: false,
            });
        return checkTree(
            source.rel === "." ? staged : join(staged, basename(source.rel)),
            operation,
            caps,
        );
    } catch {
        return failure(operation, "refused");
    }
}

/** Named stage roots delivered without clobbering; failed claims roll back and copier limits default to gifts. A remote house receives at most 4 MiB per file. */
export async function deliver(
    home: Home,
    target: {
        root: string;
        into: string;
        collision?: "number";
        /** Explicit copier limits for trees larger than gifts. */
        caps?: Readonly<TreeCaps>;
    },
    staged: string,
    names: string[],
    context: Context,
): Promise<Result<string[], TreeFailure>> {
    const operation = "deliver-tree";
    const to = place(home, target.root, operation);
    if (!to.ok) return to;
    const from = place(home, staged, operation);
    if (!from.ok) return from;
    if (from.value.kind === "remote") return failure(operation, "refused");
    const remote = to.value.kind === "remote" ? to.value.house : undefined;
    target = { ...target, root: resolve(target.root) };
    staged = resolve(staged);
    const caps = target.caps ?? GIFT_CAPS;
    const valid = validCaps(caps, operation);
    if (!valid.ok) return valid;
    const path = treeRel(target.into, operation, true);
    if (!path.ok) return path;
    for (const name of names) {
        const path = treeRel(name, operation);
        if (!path.ok) return path;
        if (name.includes("/"))
            return err({
                operation,
                kind: "path-invalid",
                path: name,
                constraint: "top-level",
                retrySafe: false,
            });
    }
    if (new Set(names).size !== names.length)
        return err({
            operation,
            kind: "path-invalid",
            path: names.join(","),
            constraint: "distinct",
            retrySafe: false,
        });
    if (remote !== undefined)
        return home.houses.deliver(() =>
            remoteDeliver(remote, { ...target, caps }, staged, names, context),
        );
    return home.houses.deliver(async () => {
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
                if (stat && !stat.isDirectory())
                    return failure(operation, "shelf-linked");
            }
            const targets: string[] = [];
            for (const name of names) {
                const tree = await checkTree(join(staged, name), operation);
                if (!tree.ok) return tree;
                let into = name,
                    number = 1;
                while (
                    await lstat(join(target.root, target.into, into)).catch(
                        () => undefined,
                    )
                ) {
                    if (target.collision !== "number")
                        return err({
                            operation,
                            kind: "clobber",
                            name,
                            retrySafe: false,
                        });
                    const ext = extname(name);
                    into = `${name.slice(0, name.length - ext.length)}-${++number}${ext}`;
                }
                targets.push(into);
            }
            return await copyInto(
                home,
                target.root,
                target.into,
                staged,
                names,
                targets,
                caps,
                context,
            );
        } catch {
            return failure(operation, "refused");
        }
    });
}
async function copyInto(
    home: Home,
    root: string,
    into: string,
    staged: string,
    names: string[],
    targets: string[],
    caps: Readonly<TreeCaps>,
    context: Context,
): Promise<Result<string[], TreeFailure>> {
    const operation = "deliver-tree";
    const env = treeWorker(home, {
        root: staged,
        stage: root,
        caps,
    });
    const paths = targets.map((name) => join(root, into, name));
    let claimed: string | undefined;
    let completed = false;
    try {
        const journal = await env.createTempFile(
            { prefix: "claimed-" },
            context,
        );
        if (!journal.ok) return failure(operation, "refused");
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
                "tree",
                claimed,
                join(root, into),
                ...names.flatMap((name, i) => [
                    join(staged, name),
                    paths[i] ?? "",
                ]),
            ],
            undefined,
            context,
        );
        completed = copied.ok && copied.value.exitCode === 0;
        return completed ? ok(targets) : failure(operation, "refused");
    } finally {
        if (!completed && claimed !== undefined) {
            const journalRead = await env.readTextFile(claimed, context);
            const made = journalRead.ok
                ? journalRead.value
                      .split("\0")
                      .filter((path) => paths.includes(path))
                : [];
            if (made.length)
                await env.exec(
                    ["/bin/rm", "-rf", "--", ...made],
                    undefined,
                    context,
                );
        }
        await env.cleanup(context);
    }
}

/** Regular files and digests, with unsafe entries left unread; a remote house is listed through its workspace. */
export async function list(
    home: Home,
    root: string,
    rel: string,
    context: Context,
): Promise<Result<TreeListing, TreeFailure>> {
    const path = treeRel(rel, "list-tree", true);
    if (!path.ok) return path;
    const where = place(home, root, "list-tree");
    if (!where.ok) return where;
    if (where.value.kind === "remote")
        return remoteList(where.value.house, rel, context);
    root = resolve(root);
    const report = await treeOperation(
        home,
        root,
        ["list", root, join(root, rel)],
        "list-tree",
        context,
    );
    return report.ok
        ? ok({ files: report.value.files, skipped: report.value.skipped })
        : report;
}

/** Removes paths under their sole root's write policy, removing links as links; a remote house is changed only through its workspace. */
export async function remove(
    home: Home,
    root: string,
    rels: string[],
    context: Context,
): Promise<Result<void, TreeFailure>> {
    for (const rel of rels) {
        const path = treeRel(rel, "remove-tree");
        if (!path.ok) return path;
    }
    const where = place(home, root, "remove-tree");
    if (!where.ok) return where;
    if (where.value.kind === "remote")
        return remoteRemove(where.value.house, rels, context);
    root = resolve(root);
    const report = await treeOperation(
        home,
        root,
        ["remove", root, ...rels.map((rel) => join(root, rel))],
        "remove-tree",
        context,
        root,
    );
    return report.ok ? ok(undefined) : report;
}

/** Detaches only server-owned house folders before freeing their regular bytes; remote houses are refused untouched. */
export async function erase(
    home: Home,
    dir: string,
): Promise<Result<number, TreeFailure>> {
    const operation = "erase-house";
    const where = place(home, dir, operation);
    if (!where.ok) return where;
    if (where.value.kind === "remote")
        return err({
            operation,
            kind: "refused",
            remote: "erase-unsupported",
            retrySafe: false,
        });
    if (!isAbsolute(dir) || dir.split(sep).includes(".."))
        return failure(operation, "refused");
    const absolute = resolve(dir);
    const root = [home.layout.houses, home.layout.librarian].find(
        (root) => absolute !== resolve(root) && within(absolute, resolve(root)),
    );
    if (!root || !within(canon(dir), canon(root)))
        return failure(operation, "refused");
    try {
        let path = root;
        for (const part of relative(root, absolute).split(sep)) {
            path = join(path, part);
            if (!(await lstat(path)).isDirectory())
                return failure(operation, "refused");
        }
        const erasing = home.layout.erasing;
        await mkdir(erasing, { recursive: true, mode: 0o700 });
        if (!(await lstat(erasing)).isDirectory())
            return failure(operation, "refused");
        const slot = await mkdtemp(join(erasing, "house-"));
        const detached = join(slot, "tree");
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
        return ok(bytes);
    } catch {
        return failure(operation, "refused");
    }
}
async function treeOperation(
    home: Home,
    root: string,
    args: string[],
    operation: Operation,
    context: Context,
    writable?: string,
): Promise<Result<TreeReport, TreeFailure>> {
    try {
        const env = treeWorker(home, {
            root,
            ...(writable === undefined ? {} : { stage: writable }),
            caps: MERGE_CAPS,
        });
        try {
            return await runTree(env, args, operation, context);
        } finally {
            await env.cleanup(context);
        }
    } catch {
        return failure(operation, "refused");
    }
}
async function runTree(
    env: SandboxEnv,
    args: string[],
    operation: Operation,
    context: Context,
): Promise<Result<TreeReport, TreeFailure>> {
    let output = "";
    const result = await env.exec(
        [
            process.execPath,
            "--disable-warning=ExperimentalWarning",
            fileURLToPath(
                new URL("../sandbox/tree-helper.ts", import.meta.url),
            ),
            ...args,
        ],
        {
            onOutput: (text) => {
                output += text;
            },
        },
        context,
    );
    if (!result.ok || result.value.exitCode !== 0)
        return failure(operation, "refused");
    let report: unknown;
    try {
        report = JSON.parse(output);
    } catch {
        return failure(operation, "refused");
    }
    if (!Value.Check(Report, report) || report.failed)
        return failure(operation, "refused");
    return ok(report);
}

/** Classifies a tree path before any host access; a remote-mode path naming no single house is refused. */
function place(
    home: Home,
    path: string,
    operation: Operation,
): Result<Exclude<HousePlace, { kind: "unmappable" }>, TreeFailure> {
    const where = remoteHouse(home, path);
    return where.kind === "unmappable"
        ? remoteStop(operation, "unmappable")
        : ok(where);
}
function remoteStop(
    operation: Operation,
    remote: RemoteStop,
): Result<never, TreeFailure> {
    return err({ operation, kind: "refused", remote, retrySafe: false });
}
/** A missing reply, cancellation or unclassified worker failure may have changed the house. */
function unconfirmed(error: FileError): boolean {
    return error.code === "unknown" || error.code === "aborted";
}
function digest(bytes: Uint8Array): string {
    return createHash("sha256").update(bytes).digest("hex");
}
/** `entry` relative to the tree's root, from a helper path relative to the listed `rel`. */
function underRoot(rel: string, entry: string): string {
    if (entry === ".") return rel;
    return rel === "." ? entry : posix.join(rel, entry);
}
function skippedKind(kind: string): "link" | "special" {
    return kind === "link" || kind === "symlink" ? "link" : "special";
}

/**
 * Requires every folder from `/house` down to `path`'s parent to be a real directory, without following links.
 * Linked folders below `root` are reported relative to it, as the local helper does.
 */
async function remoteParents(
    env: WorkspaceEnv,
    root: string,
    path: string,
    operation: Operation,
    context: Context,
): Promise<Result<void, TreeFailure>> {
    const parts = posix
        .relative(REMOTE_HOUSE_ROOT, path)
        .split("/")
        .filter((part) => part !== "")
        .slice(0, -1);
    let current = REMOTE_HOUSE_ROOT;
    for (const part of parts) {
        current = posix.join(current, part);
        const info = await env.fileInfo(current, context);
        if (!info.ok) return failure(operation, "refused");
        if (info.value.kind === "directory") continue;
        if (current.startsWith(`${root}/`))
            return err({
                kind: "unsafe-entry",
                skipped: [
                    {
                        path: posix.relative(root, current),
                        kind: skippedKind(info.value.kind),
                    },
                ],
                operation,
                retrySafe: false,
            });
        return failure(operation, "refused");
    }
    return ok(undefined);
}

/** The house's bounded tree under `rel`, checked as untrusted helper output. */
async function remoteTree(
    env: WorkspaceEnv,
    house: RemoteHouse,
    rel: string,
    operation: Operation,
    context: Context,
): Promise<Result<WorkspaceTreeEntry[], TreeFailure>> {
    const source = rel === "." ? house.path : posix.join(house.path, rel);
    const parents = await remoteParents(
        env,
        house.path,
        source,
        operation,
        context,
    );
    if (!parents.ok) return parents;
    // MERGE_CAPS is the confined helper's own ceiling; smaller caps are counted here.
    const listed = await env.tree(source, MERGE_CAPS, context);
    if (!listed.ok) return failure(operation, "refused");
    const root = listed.value[0];
    if (root?.path !== ".") return failure(operation, "refused");
    for (const entry of listed.value) {
        if (!treeRel(entry.path, operation, true).ok)
            return failure(operation, "refused");
        if (entry.kind === "file" && entry.sha256 === undefined)
            return failure(operation, "refused");
    }
    return ok(listed.value);
}

/** One remote regular file's bytes, read without following a final link and matched to its listing. */
async function remoteBytes(
    env: WorkspaceEnv,
    path: string,
    expected: WorkspaceTreeEntry,
    context: Context,
): Promise<Uint8Array | undefined> {
    const opened = await env.openBinaryReader(
        path,
        { noFollow: true },
        context,
    );
    if (!opened.ok) return undefined;
    try {
        const info = await opened.value.info(context);
        if (!info.ok || info.value.kind !== "file") return undefined;
        const read = await opened.value.read(0, expected.size + 1, context);
        if (
            !read.ok ||
            read.value.byteLength !== expected.size ||
            digest(read.value) !== expected.sha256
        )
            return undefined;
        return read.value;
    } finally {
        await opened.value.close(context);
    }
}

/** Copies a remote house tree into an empty host stage, verifying each file's size and digest. */
async function remoteStage(
    house: RemoteHouse,
    rel: string,
    staged: string,
    caps: Readonly<TreeCaps>,
    context: Context,
): Promise<Result<void, TreeFailure>> {
    const operation = "stage-tree";
    staged = resolve(staged);
    try {
        if (
            !(await lstat(staged)).isDirectory() ||
            (await readdir(staged)).length
        )
            return failure(operation, "stage-not-empty");
    } catch {
        return failure(operation, "refused");
    }
    const dest = rel === "." ? staged : join(staged, posix.basename(rel));
    const env = house.open();
    try {
        const listed = await remoteTree(env, house, rel, operation, context);
        if (!listed.ok) return listed;
        let bytes = 0;
        const skipped: TreeListing["skipped"] = [];
        for (const [index, entry] of listed.value.entries()) {
            if (index + 1 > caps.entries)
                return err({
                    kind: "entries-cap",
                    cap: caps.entries,
                    operation,
                    retrySafe: false,
                });
            if (entry.kind === "file") {
                bytes += entry.size;
                if (bytes > caps.bytes)
                    return err({
                        kind: "bytes-cap",
                        cap: caps.bytes,
                        operation,
                        retrySafe: false,
                    });
            } else if (entry.kind !== "directory")
                skipped.push({
                    path: underRoot(rel, entry.path),
                    kind: skippedKind(entry.kind),
                });
        }
        if (skipped.length)
            return err({
                kind: "unsafe-entry",
                skipped,
                operation,
                retrySafe: false,
            });
        if (
            listed.value.some(
                (entry) =>
                    entry.kind === "file" && entry.size > REMOTE_FILE_BYTES,
            )
        )
            return err({
                kind: "bytes-cap",
                cap: REMOTE_FILE_BYTES,
                scope: "file",
                operation,
                retrySafe: false,
            });
        const source = rel === "." ? house.path : posix.join(house.path, rel);
        const folders: { path: string; mode: number }[] = [];
        // The helper lists each folder before its contents.
        for (const entry of listed.value) {
            const to =
                entry.path === "."
                    ? dest
                    : join(dest, ...entry.path.split("/"));
            const mode = entry.mode & 0o777;
            if (entry.kind === "directory") {
                if (to !== staged) await mkdir(to, { mode: 0o700 });
                folders.push({ path: to, mode });
                continue;
            }
            const from =
                entry.path === "." ? source : posix.join(source, entry.path);
            const content = await remoteBytes(env, from, entry, context);
            if (content === undefined) return failure(operation, "refused");
            const file = await open(to, "wx", 0o600);
            try {
                await file.writeFile(content);
                await file.chmod(mode);
            } finally {
                await file.close();
            }
        }
        for (const folder of folders.reverse())
            await chmod(folder.path, folder.mode);
    } catch {
        return failure(operation, "refused");
    } finally {
        await env.cleanup(context);
    }
    return checkTree(dest, operation, caps);
}

/** Regular files and digests of a remote house tree, with links and special files left unread. */
async function remoteList(
    house: RemoteHouse,
    rel: string,
    context: Context,
): Promise<Result<TreeListing, TreeFailure>> {
    const env = house.open();
    try {
        const listed = await remoteTree(env, house, rel, "list-tree", context);
        if (!listed.ok) return listed;
        const listing: TreeListing = { files: [], skipped: [] };
        for (const entry of listed.value) {
            const path = underRoot(rel, entry.path);
            if (entry.kind === "file")
                listing.files.push({
                    path,
                    size: entry.size,
                    sha256: entry.sha256 ?? "",
                });
            else if (entry.kind !== "directory")
                listing.skipped.push({ path, kind: skippedKind(entry.kind) });
        }
        return ok(listing);
    } finally {
        await env.cleanup(context);
    }
}

/** Removes remote paths, links as links, stopping at the first linked parent or failed removal. */
async function remoteRemove(
    house: RemoteHouse,
    rels: string[],
    context: Context,
): Promise<Result<void, TreeFailure>> {
    const operation = "remove-tree";
    const env = house.open();
    let removed = 0;
    try {
        for (const rel of rels) {
            const path = posix.join(house.path, rel);
            const parents = await remoteParents(
                env,
                house.path,
                path,
                operation,
                context,
            );
            if (!parents.ok)
                return removed ? remoteStop(operation, "partial") : parents;
            const gone = await env.remove(
                path,
                { recursive: true, force: true },
                context,
            );
            // A recursive removal can fail after removing part of its tree.
            if (!gone.ok)
                return remoteStop(operation, removed ? "partial" : "uncertain");
            removed++;
        }
        return ok(undefined);
    } finally {
        await env.cleanup(context);
    }
}

type StagedEntry = {
    /** Relative to the delivered top-level entry; `.` is the entry itself. */
    rel: string;
    kind: "file" | "directory";
    mode: number;
    size: number;
    /** Set once the bytes are read for delivery. */
    sha256?: string;
};

/** A host stage entry's folders and regular files in pre-order; links were refused by `checkTree`. */
async function stagedEntries(root: string): Promise<StagedEntry[]> {
    const entries: StagedEntry[] = [];
    const walk = async (path: string, rel: string): Promise<void> => {
        const info = await lstat(path);
        const mode = info.mode & 0o777;
        if (info.isDirectory()) {
            entries.push({ rel, kind: "directory", mode, size: 0 });
            for (const name of (await readdir(path)).sort())
                await walk(
                    join(path, name),
                    rel === "." ? name : posix.join(rel, name),
                );
        } else if (info.isFile())
            entries.push({ rel, kind: "file", mode, size: info.size });
        else throw new Error("A staged tree changed after its check.");
    };
    await walk(root, ".");
    return entries;
}

/** A staged regular file's bytes, opened without following links and matched to its listed size. */
async function stagedBytes(path: string, size: number): Promise<Uint8Array> {
    const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    try {
        const bytes = await file.readFile();
        if (bytes.byteLength !== size)
            throw new Error("A staged file changed after its check.");
        return bytes;
    } finally {
        await file.close();
    }
}

/**
 * Delivers host stage roots into a remote house under missing names only. Nothing in the house changes until the
 * stage passes its caps and the targets are chosen; afterwards each claimed path is recorded, and a definite failure
 * removes exactly those paths again (folders only while empty). An unconfirmed step or incomplete cleanup is reported
 * as uncertain or partial, never as nothing handed over.
 */
async function remoteDeliver(
    house: RemoteHouse,
    target: { into: string; collision?: "number"; caps: Readonly<TreeCaps> },
    staged: string,
    names: string[],
    context: Context,
): Promise<Result<string[], TreeFailure>> {
    const operation = "deliver-tree";
    const trees: StagedEntry[][] = [];
    try {
        let entries = 0,
            bytes = 0;
        for (const name of names) {
            const tree = await checkTree(join(staged, name), operation);
            if (!tree.ok) return tree;
            const listed = await stagedEntries(join(staged, name));
            for (const entry of listed) {
                if (++entries > target.caps.entries)
                    return err({
                        kind: "entries-cap",
                        cap: target.caps.entries,
                        operation,
                        retrySafe: false,
                    });
                bytes += entry.size;
                if (bytes > target.caps.bytes)
                    return err({
                        kind: "bytes-cap",
                        cap: target.caps.bytes,
                        operation,
                        retrySafe: false,
                    });
                if (entry.size > REMOTE_FILE_BYTES)
                    return err({
                        kind: "bytes-cap",
                        cap: REMOTE_FILE_BYTES,
                        scope: "file",
                        operation,
                        retrySafe: false,
                    });
            }
            trees.push(listed);
        }
    } catch {
        return failure(operation, "refused");
    }
    const into =
        target.into === "." ? house.path : posix.join(house.path, target.into);
    const env = house.open();
    try {
        // Folders from `/house` down to the receiving shelf; the first missing one and those below it are made.
        const missing: string[] = [];
        let current = REMOTE_HOUSE_ROOT;
        for (const part of posix
            .relative(REMOTE_HOUSE_ROOT, into)
            .split("/")
            .filter((part) => part !== "")) {
            current = posix.join(current, part);
            if (missing.length) {
                missing.push(current);
                continue;
            }
            const info = await env.fileInfo(current, context);
            if (info.ok) {
                if (info.value.kind !== "directory")
                    return failure(operation, "shelf-linked");
            } else if (info.error.code === "not_found") missing.push(current);
            else return failure(operation, "refused");
        }
        const targets: string[] = [];
        const free = async (
            name: string,
        ): Promise<Result<boolean, TreeFailure>> => {
            if (targets.includes(name)) return ok(false);
            if (missing.length) return ok(true);
            const info = await env.fileInfo(posix.join(into, name), context);
            if (info.ok) return ok(false);
            return info.error.code === "not_found"
                ? ok(true)
                : failure(operation, "refused");
        };
        for (const name of names) {
            let chosen = name,
                number = 1;
            for (;;) {
                const available = await free(chosen);
                if (!available.ok) return available;
                if (available.value) break;
                if (target.collision !== "number")
                    return err({
                        operation,
                        kind: "clobber",
                        name,
                        retrySafe: false,
                    });
                const ext = extname(name);
                chosen = `${name.slice(0, name.length - ext.length)}-${++number}${ext}`;
            }
            targets.push(chosen);
        }
        return await claimRemote(
            env,
            { into, missing },
            staged,
            names,
            targets,
            trees,
            context,
        );
    } finally {
        await env.cleanup(context);
    }
}

/** Writes the chosen targets, verifies them through the house's own tree and settles a failure honestly. */
async function claimRemote(
    env: WorkspaceEnv,
    shelf: { into: string; missing: string[] },
    staged: string,
    names: string[],
    targets: string[],
    trees: StagedEntry[][],
    context: Context,
): Promise<Result<string[], TreeFailure>> {
    const operation = "deliver-tree";
    const claimed: string[] = [];
    const directories: { path: string; mode: number }[] = [];
    const settle = async (
        unsure: boolean,
    ): Promise<Result<never, TreeFailure>> => {
        let complete = true;
        for (const path of claimed.reverse()) {
            const undone = await env.remove(path, undefined, context);
            if (!undone.ok) complete = false;
        }
        if (unsure) return remoteStop(operation, "uncertain");
        return complete
            ? failure(operation, "refused")
            : remoteStop(operation, "partial");
    };
    for (const folder of shelf.missing) {
        const made = await env.createDir(folder, undefined, context);
        if (!made.ok) return settle(unconfirmed(made.error));
        claimed.push(folder);
    }
    for (const [index, name] of names.entries()) {
        const chosen = targets[index] ?? "";
        for (const entry of trees[index] ?? []) {
            const to =
                entry.rel === "."
                    ? posix.join(shelf.into, chosen)
                    : posix.join(shelf.into, chosen, entry.rel);
            if (entry.kind === "directory") {
                const made = await env.createDir(to, undefined, context);
                if (!made.ok) return settle(unconfirmed(made.error));
                claimed.push(to);
                directories.push({ path: to, mode: entry.mode });
                continue;
            }
            let content: Uint8Array;
            try {
                content = await stagedBytes(
                    entry.rel === "."
                        ? join(staged, name)
                        : join(staged, name, ...entry.rel.split("/")),
                    entry.size,
                );
            } catch {
                return settle(false);
            }
            entry.sha256 = digest(content);
            const written = await env.writeExclusive(
                to,
                content,
                entry.mode,
                context,
            );
            if (!written.ok) return settle(unconfirmed(written.error));
            claimed.push(to);
        }
    }
    // Apply restrictive directory modes only after their children have been delivered.
    for (const directory of directories.reverse()) {
        const changed = await env.exec(
            [
                "/usr/bin/chmod",
                "--",
                directory.mode.toString(8),
                directory.path,
            ],
            undefined,
            context,
        );
        if (!changed.ok || changed.value.exitCode !== 0)
            return remoteStop(operation, "uncertain");
    }
    for (const [index, chosen] of targets.entries()) {
        const listed = await env.tree(
            posix.join(shelf.into, chosen),
            MERGE_CAPS,
            context,
        );
        const expected = trees[index] ?? [];
        // A mismatch may hold someone else's change, so nothing is undone.
        if (!listed.ok) return remoteStop(operation, "uncertain");
        if (listed.value.length !== expected.length)
            return remoteStop(operation, "partial");
        const found = new Map(listed.value.map((entry) => [entry.path, entry]));
        for (const entry of expected) {
            const actual = found.get(entry.rel);
            if (
                actual?.kind !== entry.kind ||
                (actual.mode & 0o777) !== entry.mode ||
                (entry.kind === "file" &&
                    (actual.size !== entry.size ||
                        actual.sha256 !== entry.sha256))
            )
                return remoteStop(operation, "partial");
        }
    }
    return ok(targets);
}

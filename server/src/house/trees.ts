import { lstat, mkdir, mkdtemp, readdir, rename, rm } from "node:fs/promises";
import {
    basename,
    extname,
    isAbsolute,
    join,
    relative,
    resolve,
    sep,
    win32,
} from "node:path";
import { fileURLToPath } from "node:url";
import type { Context } from "@earendil-works/chord";
import { type Static, Type } from "typebox";
import { Value } from "typebox/value";
import { treeWorker } from "../environment.ts";
import type { Home } from "../home.ts";
import { MAX_GIFT_BYTES } from "../protocol.ts";
import { err, ok, type Result } from "../result.ts";
import { canon, within } from "../sandbox/policy.ts";
import type { SandboxEnv } from "../sandbox.ts";

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
/** Regular files with their no-follow size and digest. */
export type TreeFile = TreeReport["files"][number];
/** Unsafe entries are reported without reading their contents. */
export type TreeListing = Pick<TreeReport, "files" | "skipped">;
type Operation =
    | "stage-tree"
    | "deliver-tree"
    | "list-tree"
    | "remove-tree"
    | "erase-house";
/** A tree refusal may follow partial work; inspect its target before retrying. */
export type TreeFailure = { operation: Operation; retrySafe: false } & (
    | { kind: "entries-cap" | "bytes-cap"; cap: number }
    | { kind: "unsafe-entry"; skipped: TreeListing["skipped"] }
    | {
          kind:
              | "stage-not-empty"
              | "stage-overlaps-source"
              | "shelf-linked"
              | "refused";
      }
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
            return "The tree exceeds its bytes cap.";
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

/** A no-follow copy into a separate empty stage, keeping regular bytes and modes. */
export async function stage(
    home: Home,
    source: { root: string; rel: string },
    staged: string,
    caps: Readonly<TreeCaps>,
    context: Context,
): Promise<Result<void, TreeFailure>> {
    const operation = "stage-tree";
    source = { ...source, root: resolve(source.root) };
    staged = resolve(staged);
    const path = treeRel(source.rel, operation, true);
    if (!path.ok) return path;
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

/** Named stage roots delivered without clobbering; failed claims are rolled back. */
export async function deliver(
    home: Home,
    target: { root: string; into: string; collision?: "number" },
    staged: string,
    names: string[],
    context: Context,
): Promise<Result<string[], TreeFailure>> {
    const operation = "deliver-tree";
    target = { ...target, root: resolve(target.root) };
    staged = resolve(staged);
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
    context: Context,
): Promise<Result<string[], TreeFailure>> {
    const operation = "deliver-tree";
    const env = treeWorker(home, {
        root: staged,
        stage: root,
        caps: MERGE_CAPS,
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

/** Regular files and digests, with unsafe entries left unread. */
export async function list(
    home: Home,
    root: string,
    rel: string,
    context: Context,
): Promise<Result<TreeListing, TreeFailure>> {
    const path = treeRel(rel, "list-tree", true);
    if (!path.ok) return path;
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

/** Removes paths under their sole root's write policy, removing links as links. */
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

/** Detaches only server-owned house folders before freeing their regular bytes. */
export async function erase(
    home: Home,
    dir: string,
): Promise<Result<number, TreeFailure>> {
    const operation = "erase-house";
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

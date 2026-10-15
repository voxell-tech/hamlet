import { lstat, readdir } from "node:fs/promises";
import { dirname, extname, isAbsolute, join, win32 } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { Home } from "./home.ts";
import { MAX_GIFT_BYTES } from "./protocol.ts";
import { SandboxEnv } from "./sandbox/env.ts";
import { closedPolicy } from "./sandbox/reach.ts";

const deliveries = new WeakMap<Home, Promise<unknown>>();
const REFUSED =
    "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.";

const MAX_GIFT_FILES = 4096;

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

/** A no-link skill tree of regular files with bounded bytes and entry count. */
export async function checkSkill(dir: string): Promise<void> {
    let bytes = 0;
    let entries = 0;
    const walk = async (path: string): Promise<void> => {
        if (++entries > MAX_GIFT_FILES)
            throw new Error(
                `A skill gift takes at most ${MAX_GIFT_FILES} files and folders at once.`,
            );
        const info = await lstat(path);
        if (info.isDirectory()) {
            for (const name of await readdir(path))
                await walk(join(path, name));
        } else if (info.isFile()) {
            bytes += info.size;
            if (bytes > MAX_GIFT_BYTES)
                throw new Error("The skill gift is larger than 32 MB.");
        } else
            throw new Error(
                "A skill gift cannot include links or special files.",
            );
    };
    await walk(dir);
}

/** A giver's skill copied into a server-owned stage under the giver's house read policy. */
export async function stageSkill(
    home: Home,
    house: string,
    skill: string,
    stage: string,
    context: Context,
): Promise<void> {
    const env = giftEnv(home, stage, [house]);
    try {
        const copied = await env.exec(
            [
                "/bin/cp",
                "-RP",
                "--",
                join(house, "skills", skill),
                join(stage, skill),
            ],
            undefined,
            context,
        );
        if (!copied.ok || copied.value.exitCode !== 0) throw new Error(REFUSED);
        await checkSkill(join(stage, skill));
    } finally {
        await env.cleanup(context);
    }
}

function giftEnv(home: Home, writable: string, readable: string[]): SandboxEnv {
    const policy = closedPolicy(writable, readable);
    policy.env = { PATH: "/usr/bin:/bin", HOME: writable, LANG: "C.UTF-8" };
    // Linux cp probes past EOF and hits RLIMIT_FSIZE on an exactly full file.
    policy.limits = {
        maxFileMb: MAX_GIFT_BYTES / (1024 * 1024) + 1,
        wallSecs: 10,
    };
    return new SandboxEnv({
        cwd: writable,
        policy,
        sandbox: home.sandbox,
        tmpScope: "gifts",
    });
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
    const before = deliveries.get(home) ?? Promise.resolve();
    const next = before
        .catch(() => {})
        .then(async () => {
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
            const env = giftEnv(home, house, [source]);
            const paths = targets.map((name) => join(house, shelf, name));
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
  cp -RP -- "$copy" "$target"
done`,
                        "gift",
                        claimed,
                        join(house, shelf),
                        ...roots.flatMap((root, i) => [
                            join(source, root),
                            paths[i] ?? "",
                        ]),
                    ],
                    undefined,
                    context,
                );
                if (!copied.ok || copied.value.exitCode !== 0)
                    throw new Error(REFUSED);
                return targets;
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
        });
    deliveries.set(home, next);
    try {
        return await next;
    } finally {
        if (deliveries.get(home) === next) deliveries.delete(home);
    }
}

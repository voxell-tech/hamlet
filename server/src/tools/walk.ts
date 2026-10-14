import { isAbsolute, join, relative, sep } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { ExecutionEnv, FileInfo } from "@earendil-works/pi-durable/env";

/** Folders a walk does not go into. */
const SKIPPED = new Set([
    ".git",
    ".hamlet",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    ".next",
]);

/** The most files a walk looks at. */
const VISITED = 20_000;

/** The environment a tool's call runs in. */
export function envOf(api: {
    readonly env: ExecutionEnv | undefined;
}): ExecutionEnv {
    if (api.env === undefined) {
        throw new Error("No execution environment is configured");
    }
    return api.env;
}

/** The most hits or names a search gives. */
export const FOUND = 100;

/**
 * Visits each file under `root` through `env`: a folder's files in name order, then its folders, the first first (reading.rs:122-210).
 * Links are not followed, and a folder that cannot be listed, such as Hamlet's home, is gone around. It stops when
 * `visit` gives false, or after {@link VISITED} files, and says whether it stopped at that cap.
 */
export async function walk(
    env: ExecutionEnv,
    root: string,
    visit: (path: string, file: FileInfo) => Promise<boolean>,
    context: Context,
): Promise<boolean> {
    let seen = 0;
    const todo = [root];
    for (let dir = todo.pop(); dir !== undefined; dir = todo.pop()) {
        const listed = await env.listDir(dir, context);
        if (!listed.ok) continue;
        const below: string[] = [];
        for (const entry of [...listed.value].sort(byName)) {
            const path = join(dir, entry.name);
            if (entry.kind === "directory" && !SKIPPED.has(entry.name)) {
                below.push(path);
            } else if (entry.kind === "file") {
                seen += 1;
                if (seen > VISITED) return true;
                if (!(await visit(path, entry))) return false;
            }
        }
        todo.push(...below.reverse());
    }
    return false;
}

/** The full path `path` names, from the folder commands run in, and what is there; it fails when that cannot be read. */
export async function reached(
    env: ExecutionEnv,
    path: string | undefined,
    context: Context,
): Promise<{ full: string; info: FileInfo }> {
    const given = (path ?? "").trim();
    const full = isAbsolute(given) ? given : join(env.cwd, given);
    const info = await env.fileInfo(full, context);
    if (!info.ok) throw new Error(info.error.message);
    // A folder's reach is its listing's, and a file's its opening's.
    if (info.value.kind === "directory") {
        const listed = await env.listDir(full, context);
        if (!listed.ok) throw new Error(listed.error.message);
    } else {
        const opened = await env.openBinaryReader(full, undefined, context);
        if (!opened.ok) throw new Error(opened.error.message);
        await opened.value.close(context);
    }
    return { full, info: info.value };
}

/** `path` as a search shows it: under the folder commands run in where it lies there. */
export function shown(env: ExecutionEnv, path: string): string {
    const under = relative(env.cwd, path);
    return under === "" || under.startsWith("..") || isAbsolute(under)
        ? path
        : under;
}

/** `path` under `root`, with `/` between its parts. */
export function under(root: string, path: string): string {
    return relative(root, path).split(sep).join("/");
}

/** The findings of a search, or `none`, with a note where it was cut short. */
export function report(
    found: readonly string[],
    capped: boolean,
    none: string,
): string {
    const lines = found.slice(0, FOUND);
    if (lines.length === 0) lines.push(none);
    if (found.length > FOUND) {
        lines.push(`[more than ${FOUND}. Narrow it with \`path\` or \`name\`]`);
    }
    if (capped) {
        lines.push(`[stopped after ${VISITED} files. Narrow it with \`path\`]`);
    }
    return lines.join("\n");
}

/** True when the file at `path` under the searched folder is one `pattern` asks for: by its name, or by its path when `pattern` has a `/`. */
export function wanted(pattern: string, path: string): boolean {
    return globMatches(
        pattern,
        pattern.includes("/") ? path : (path.split("/").at(-1) ?? ""),
    );
}

/** True when `name` matches `pattern`, where `*` stands for any run of characters and `?` for one. */
export function globMatches(pattern: string, name: string): boolean {
    const want = [...pattern];
    const have = [...name];
    let p = 0;
    let n = 0;
    // The place of the last `*`, and how much of the name it took.
    let star: { at: number; took: number } | undefined;
    while (n < have.length) {
        if (p < want.length && (want[p] === "?" || want[p] === have[n])) {
            p += 1;
            n += 1;
        } else if (p < want.length && want[p] === "*") {
            star = { at: p, took: n };
            p += 1;
        } else if (star !== undefined) {
            star.took += 1;
            p = star.at + 1;
            n = star.took;
        } else {
            return false;
        }
    }
    return want.slice(p).every((c) => c === "*");
}

function byName(a: FileInfo, b: FileInfo): number {
    return a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
}

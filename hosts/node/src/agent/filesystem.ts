import { join } from "node:path";
import type { Context } from "@earendil-works/chord/context";
import {
    err,
    FileError,
    type FileErrorCode,
    type FileInfo,
    ok,
    type Result,
} from "@earendil-works/pi-durable/env";
import { AgentFolder } from "@hamlet/actor-node";

/** The folders an agent's filesystem holds from its first start. */
const FOLDERS = ["memory", "skills", "inbox"];

/** The most names a search or a find looks at. */
const LOOKED = 20_000;

/** The most matches or files a search or a find gives. */
const FOUND = 100;

/** A name in a folder of the agent's filesystem, a link itself rather than what it names. */
export type FileEntry = Omit<FileInfo, "mtimeMs">;

/** What a search or a find gave, and whether it stopped at a bound. */
export interface Found<T> {
    readonly found: T[];
    readonly capped: boolean;
}

/** A line that matched a search, numbered from 1. */
export interface Match {
    readonly path: string;
    readonly line: number;
    readonly text: string;
}

/** One replacement in a file: `old` occurs once in it, and becomes `new`. */
export interface Edit {
    readonly old: string;
    readonly new: string;
}

/**
 * The agent's filesystem: its own folder, which no other agent and no
 * project shares. Each path is relative to the folder or absolute below it.
 * No operation follows a link, nor a path out of the folder, also while a
 * command of the agent swaps one of its folders for a link. The operations
 * keep the shapes of pi-durable's `ExecutionEnv`: an expected failure is
 * returned as a `FileError`, and each takes the call's `Context`.
 */
export interface AgentFilesystem {
    /** As pi-durable's `FileSystem.id`: equal ids see the same files. */
    readonly id: string;
    /** The folder's absolute path, with `/` between names. */
    readonly root: string;
    read(
        path: string,
        context: Context,
    ): Promise<Result<Uint8Array, FileError>>;
    /** Writes the file whole, making its folders. A link there is replaced, not written through. */
    write(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>>;
    /** Makes every edit of `edits` in the UTF-8 text of the file at once. */
    edit(
        path: string,
        edits: readonly Edit[],
        context: Context,
    ): Promise<Result<void, FileError>>;
    /** The names in one folder, sorted. */
    list(
        path: string,
        context: Context,
    ): Promise<Result<FileEntry[], FileError>>;
    /**
     * The lines that match `pattern`, a regular expression that runs in
     * linear time, in the text files at or below `path`.
     */
    search(
        pattern: string,
        path: string,
        options: { ignoreCase?: boolean } | undefined,
        context: Context,
    ): Promise<Result<Found<Match>, FileError>>;
    /**
     * The files at or below `path` whose name `glob` matches, or whose path
     * from `path` it matches when it holds a `/`. In `glob`, `*` stands for
     * any run of characters and `?` for one.
     */
    find(
        glob: string,
        path: string,
        context: Context,
    ): Promise<Result<Found<string>, FileError>>;
    /** Copies a file to another place in the agent's filesystem. */
    copy(
        from: string,
        to: string,
        context: Context,
    ): Promise<Result<void, FileError>>;
}

/** What a native refusal's leading name is, as a `FileError` code. */
const CODES: Record<string, FileErrorCode> = {
    ENOENT: "not_found",
    EACCES: "permission_denied",
    EPERM: "permission_denied",
    OUTSIDE: "permission_denied",
    ENOTDIR: "not_directory",
    EISDIR: "is_directory",
    ELOOP: "invalid",
    EINVAL: "invalid",
};

function fileError(error: unknown, path: string): FileError {
    if (error instanceof FileError) {
        return error;
    }
    const cause = error instanceof Error ? error : undefined;
    const message = cause?.message ?? String(error);
    const code = CODES[message.slice(0, message.indexOf(":"))] ?? "unknown";
    return new FileError(code, `${message}: ${path}`, path, cause);
}

/**
 * Opens the filesystem of the agent numbered `agent` in the realm folder
 * `realm`, at `actors/agent/<agent>.fs`, beside the agent's database, and
 * makes its folders where missing.
 */
export function openAgentFilesystem(
    realm: string,
    agent: number,
): AgentFilesystem {
    const folder = AgentFolder.open(
        join(realm, "actors", "agent", `${agent}.fs`),
        FOLDERS,
    );
    const root = folder.path;
    const absolute = (path: string) => (path === "" ? root : `${root}/${path}`);
    const call = async <T>(
        path: string,
        context: Context,
        work: () => Promise<T>,
    ): Promise<Result<T, FileError>> => {
        if (context.abortSignal?.aborted) {
            return err(new FileError("aborted", "the call was aborted", path));
        }
        try {
            return ok(await work());
        } catch (error) {
            return err(fileError(error, path));
        }
    };
    return {
        id: `hamlet-agent-filesystem:${root}`,
        root,
        read: (path, context) =>
            call(
                path,
                context,
                async () => new Uint8Array(await folder.read(path)),
            ),
        write: (path, content, context) =>
            call(path, context, () => folder.write(path, Buffer.from(content))),
        edit: (path, edits, context) =>
            call(path, context, async () => {
                const text = utf8(await folder.read(path), path);
                await folder.write(
                    path,
                    Buffer.from(edited(text, edits, path)),
                );
            }),
        list: (path, context) =>
            call(path, context, async () =>
                (await folder.list(path)).map((entry) => ({
                    name: entry.name,
                    path: absolute(entry.path),
                    kind: entry.kind as FileEntry["kind"],
                    size: entry.size,
                })),
            ),
        search: (pattern, path, options, context) =>
            call(path, context, async () => {
                const searched = await folder.search(
                    path,
                    pattern,
                    options?.ignoreCase ?? false,
                    FOUND,
                    LOOKED,
                );
                return {
                    found: searched.matches.map((match) => ({
                        path: absolute(match.path),
                        line: match.line,
                        text: match.text,
                    })),
                    capped: searched.capped,
                };
            }),
        find: (glob, path, context) =>
            call(path, context, async () => {
                const walked = await folder.files(path, LOOKED);
                const found: string[] = [];
                for (const file of walked.files) {
                    const under =
                        file.path === walked.start
                            ? (file.path.split("/").pop() ?? "")
                            : file.path.slice(
                                  walked.start === ""
                                      ? 0
                                      : walked.start.length + 1,
                              );
                    const tested = glob.includes("/")
                        ? under
                        : (under.split("/").pop() ?? "");
                    if (!matchesGlob(glob, tested)) {
                        continue;
                    }
                    if (found.length === FOUND) {
                        return { found, capped: true };
                    }
                    found.push(absolute(file.path));
                }
                return { found, capped: walked.capped };
            }),
        copy: (from, to, context) =>
            call(from, context, async () =>
                folder.write(to, await folder.read(from)),
            ),
    };
}

function utf8(bytes: Uint8Array, path: string): string {
    try {
        return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
        throw new FileError("invalid", `not UTF-8 text: ${path}`, path);
    }
}

/** `text` with each of `edits` made in it, each found once in `text` as it was. */
function edited(text: string, edits: readonly Edit[], path: string): string {
    const spans = edits
        .map((edit) => {
            const at = text.indexOf(edit.old);
            if (edit.old === "" || at < 0) {
                throw new FileError(
                    "invalid",
                    `the text to replace is not in ${path}`,
                    path,
                );
            }
            if (text.indexOf(edit.old, at + 1) >= 0) {
                throw new FileError(
                    "invalid",
                    `the text to replace occurs more than once in ${path}`,
                    path,
                );
            }
            return { at, end: at + edit.old.length, replacement: edit.new };
        })
        .sort((a, b) => a.at - b.at);
    let out = "";
    let from = 0;
    for (const span of spans) {
        if (span.at < from) {
            throw new FileError(
                "invalid",
                `two edits replace the same text in ${path}`,
                path,
            );
        }
        out += text.slice(from, span.at) + span.replacement;
        from = span.end;
    }
    return out + text.slice(from);
}

/**
 * Whether `glob` matches the whole of `name`, where `*` stands for any run
 * of characters and `?` for one. Time grows with the product of the two
 * lengths at most.
 */
function matchesGlob(glob: string, name: string): boolean {
    const want = [...glob];
    const have = [...name];
    let at = 0;
    let index = 0;
    // The place of the last `*`, and how much of the name it took.
    let star = -1;
    let took = 0;
    while (index < have.length) {
        if (
            at < want.length &&
            (want[at] === "?" || want[at] === have[index])
        ) {
            at++;
            index++;
        } else if (at < want.length && want[at] === "*") {
            star = at;
            took = index;
            at++;
        } else if (star >= 0) {
            took++;
            at = star + 1;
            index = took;
        } else {
            return false;
        }
    }
    return want.slice(at).every((char) => char === "*");
}

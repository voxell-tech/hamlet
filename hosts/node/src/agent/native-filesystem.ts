import { join, resolve } from "node:path";
import type { Context } from "@earendil-works/chord/context";
import {
    err,
    FileError,
    type FileErrorCode,
    ok,
    type Result,
} from "@earendil-works/pi-durable/env";
import { AgentFolder } from "@hamlet/actor-node";
import {
    type AgentFilesystem,
    type Edit,
    type FileEntry,
    FOUND,
    type Found,
    LOOKED,
} from "./filesystem.ts";

/** The folders an agent's filesystem holds from its first start. */
const FOLDERS = ["memory", "skills", "inbox"];

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

/**
 * A native refusal as a `FileError`. Given the system call that `opened` the
 * file at that absolute path, its words are Node's own.
 */
function fileError(
    error: unknown,
    path: string,
    opened?: { readonly call: string; readonly at: string },
): FileError {
    if (error instanceof FileError) {
        return error;
    }
    const cause = error instanceof Error ? error : undefined;
    const message = cause?.message ?? String(error);
    const code = CODES[message.slice(0, message.indexOf(":"))] ?? "unknown";
    const words =
        opened === undefined
            ? `${message}: ${path}`
            : `${message}, ${opened.call} '${opened.at}'`;
    return new FileError(code, words, path, cause);
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
        syscall?: string,
    ): Promise<Result<T, FileError>> => {
        if (context.abortSignal?.aborted) {
            return err(new FileError("aborted", "the call was aborted", path));
        }
        try {
            return ok(await work());
        } catch (error) {
            return err(
                fileError(
                    error,
                    path,
                    syscall === undefined
                        ? undefined
                        : { call: syscall, at: resolve(root, path) },
                ),
            );
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
                "open",
            ),
        write: (path, content, context, executable) =>
            call(path, context, () =>
                folder.write(path, Buffer.from(content), executable),
            ),
        edit: (path, edits, context) =>
            call(path, context, async () => {
                // Today refuses an empty `edits` before it looks for the file.
                if (edits.length === 0) {
                    throw new FileError(
                        "invalid",
                        "Edit tool input is invalid. edits must contain at least one replacement.",
                        path,
                    );
                }
                const before = utf8(await folder.read(path), path);
                const after = edited(before, edits, path);
                await folder.write(path, Buffer.from(after));
                return { before, after };
            }),
        list: (path, context) =>
            call(path, context, async () =>
                (await folder.list(path)).map((entry) => ({
                    name: entry.name,
                    path: absolute(entry.path),
                    kind: entry.kind as FileEntry["kind"],
                    size: entry.size,
                    executable: entry.executable,
                })),
            ),
        search: (pattern, path, options, context) =>
            call(path, context, async () => {
                const searched = await folder
                    .search(
                        path,
                        pattern,
                        options?.ignoreCase ?? false,
                        options?.name,
                        FOUND + 1,
                        LOOKED,
                    )
                    .catch((error: unknown) => {
                        throw patternRefusal(error, path) ?? error;
                    });
                return bounded(
                    searched.matches.map((match) => ({
                        path: absolute(match.path),
                        line: match.line,
                        text: match.text,
                    })),
                    searched.capped,
                );
            }),
        find: (glob, path, context) =>
            call(path, context, async () => {
                const walked = await folder.files(path, LOOKED, glob);
                return bounded(
                    walked.files.map((file) => absolute(file.path)),
                    walked.capped,
                );
            }),
        walk: (path, context) =>
            call(path, context, async () => {
                const walked = await folder.files(path, LOOKED);
                return {
                    files: walked.files.map((file) => file.path),
                    capped: walked.capped,
                    executable: walked.files
                        .filter((file) => file.executable)
                        .map((file) => file.path),
                };
            }),
        remove: (path, context) =>
            call(path, context, () => folder.remove(path)),
        copy: (from, to, context) =>
            call(from, context, async () => {
                await folder.write(to, await folder.read(from));
            }),
    };
}

/**
 * A native refusal of a search's pattern by the regex crate, in today's
 * words (`crates/hamlet_server/src/work/tools/walk.rs`).
 */
function patternRefusal(error: unknown, path: string): FileError | undefined {
    const message = error instanceof Error ? error.message : "";
    if (!/^EINVAL: (regex parse error:|Compiled regex )/.test(message)) {
        return undefined;
    }
    const words = message.slice("EINVAL: ".length);
    const last = words.lastIndexOf("error: ");
    const reason = (
        last < 0 ? words : words.slice(last + "error: ".length)
    ).trim();
    return new FileError(
        "invalid",
        reason.includes("look-around") || reason.includes("backreferences")
            ? "`pattern` holds a backreference, a lookaround or another part a search in linear time cannot take. Use a linear-time pattern."
            : `\`pattern\` is not a regular expression: ${reason}. Correct the pattern.`,
        path,
    );
}

/** As today's, a cut walk that also found more says only the more. */
function bounded<T>(items: T[], capped: boolean): Found<T> {
    const more = items.length > FOUND;
    return { found: items.slice(0, FOUND), more, capped: capped && !more };
}

function utf8(bytes: Uint8Array, path: string): string {
    try {
        return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
        throw new FileError("invalid", `not UTF-8 text: ${path}`, path);
    }
}

/**
 * `text` with each of `edits` made in it, each found once in `text` as it
 * was. A refusal is in today's words (`crates/hamlet_server/src/work/tools/files.rs`).
 */
function edited(text: string, edits: readonly Edit[], path: string): string {
    const refuse = (words: string): never => {
        throw new FileError("invalid", words, path);
    };
    const many = edits.length > 1;
    const empty = edits.findIndex((edit) => edit.old === "");
    if (empty >= 0) {
        refuse(
            many
                ? `edits[${empty}].oldText must not be empty in ${path}.`
                : `oldText must not be empty in ${path}.`,
        );
    }
    const spans = edits
        .map((edit, index) => {
            const at = text.indexOf(edit.old);
            if (at < 0) {
                refuse(
                    many
                        ? `Could not find edits[${index}] in ${path}. The oldText must match exactly including all whitespace and newlines.`
                        : `Could not find the exact text in ${path}. The old text must match exactly including all whitespace and newlines.`,
                );
            }
            const count = text.split(edit.old).length - 1;
            if (count > 1) {
                refuse(
                    many
                        ? `Found ${count} occurrences of edits[${index}] in ${path}. Each oldText must be unique. Please provide more context to make it unique.`
                        : `Found ${count} occurrences of the text in ${path}. The text must be unique. Please provide more context to make it unique.`,
                );
            }
            return {
                index,
                at,
                end: at + edit.old.length,
                replacement: edit.new,
            };
        })
        .sort((a, b) => a.at - b.at);
    let out = "";
    let from = 0;
    let last: (typeof spans)[number] | undefined;
    for (const span of spans) {
        if (last !== undefined && span.at < last.end) {
            refuse(
                `edits[${last.index}] and edits[${span.index}] overlap in ${path}. Merge them into one edit or target disjoint regions.`,
            );
        }
        out += text.slice(from, span.at) + span.replacement;
        from = span.end;
        last = span;
    }
    out += text.slice(from);
    if (out === text) {
        refuse(
            many
                ? `No changes made to ${path}. The replacements produced identical content.`
                : `No changes made to ${path}. The replacement produced identical content. This might indicate an issue with special characters or the text not existing as expected.`,
        );
    }
    return out;
}

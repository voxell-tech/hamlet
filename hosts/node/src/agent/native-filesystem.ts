import { join } from "node:path";
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
            call(path, context, async () => ({
                created: await folder.write(path, Buffer.from(content)),
            })),
        edit: (path, edits, context) =>
            call(path, context, async () => {
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
                })),
            ),
        search: (pattern, path, options, context) =>
            call(path, context, async () => {
                const searched = await folder.search(
                    path,
                    pattern,
                    options?.ignoreCase ?? false,
                    options?.name,
                    FOUND + 1,
                    LOOKED,
                );
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
        copy: (from, to, context) =>
            call(from, context, async () => {
                await folder.write(to, await folder.read(from));
            }),
    };
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

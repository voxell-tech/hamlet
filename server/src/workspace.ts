import { randomUUID } from "node:crypto";
import { posix } from "node:path";
import type { Context } from "@earendil-works/chord";
import {
    type BinaryReader,
    type DirReader,
    type ExecutionEnv,
    ExecutionError,
    err,
    FileError,
    type FileInfo,
    type FileWatcher,
    LineScanner,
    ok,
    type Result,
    type ShellExecOptions,
    type ShellExecResult,
    type TextLineReader,
    type WatchChange,
    type WatchTarget,
} from "@earendil-works/pi-durable/env";
import { type Static, type TSchema, Type } from "typebox";
import { Value } from "typebox/value";
import type { WorkspaceConnection } from "./settings.ts";

/** A house-domain operation whose remote implementation is unavailable. */
export type WorkspaceUnavailable = {
    kind: "workspace-unavailable";
    operation: string;
    retrySafe: true;
};

/** Refuses house-domain work before opening any local copy. */
export function workspaceUnavailable(operation: string): {
    ok: false;
    error: WorkspaceUnavailable;
} {
    return {
        ok: false,
        error: { kind: "workspace-unavailable", operation, retrySafe: true },
    };
}

/** Recovery words for remote house-domain operations. */
export function workspaceUnavailableText(
    failure: WorkspaceUnavailable,
): string {
    return `Cannot ${failure.operation}: this operation is not supported by remote workspaces. No local copy was opened; keep the remote files in place and ask the chief before changing workspace backends.`;
}

const information = Type.Object({
    name: Type.String(),
    path: Type.String(),
    kind: Type.Union([
        Type.Literal("file"),
        Type.Literal("directory"),
        Type.Literal("symlink"),
    ]),
    size: Type.Number(),
    mtimeMs: Type.Number(),
});
const readReply = Type.Object({ data: Type.String(), info: information });
const treeEntry = Type.Object({
    path: Type.String(),
    kind: Type.Union([
        Type.Literal("file"),
        Type.Literal("directory"),
        Type.Literal("link"),
        Type.Literal("special"),
    ]),
    mode: Type.Integer(),
    size: Type.Integer(),
    sha256: Type.Optional(Type.String()),
});
/** Bounded tree metadata from the confined file helper. */
export type WorkspaceTreeEntry = Static<typeof treeEntry>;
const commandReply = Type.Object({
    exitCode: Type.Integer(),
    stdout: Type.String(),
    stderr: Type.String(),
});
const response = Type.Union([
    Type.Object({ kind: Type.Literal("ok"), value: Type.Unknown() }),
    Type.Object({
        kind: Type.Literal("error"),
        code: Type.String(),
        message: Type.String(),
        uncertain: Type.Boolean(),
    }),
]);
type Failure = Extract<Static<typeof response>, { kind: "error" }>;
type LibraryFiles = readonly { path: string; data: string; mode: number }[];
type Operation =
    | { kind: "init_house" }
    | { kind: "tree_list"; path: string; entries: number; bytes: number }
    | { kind: "tree_write"; path: string; data: string; mode: number }
    | { kind: "exec"; argv: readonly string[]; cwd: string; timeout_ms: number }
    | { kind: "read"; path: string; no_follow: boolean }
    | { kind: "info" | "list" | "canonical" | "flush"; path: string }
    | { kind: "write"; path: string; data: string; append: boolean }
    | { kind: "mkdir"; path: string; recursive: boolean }
    | { kind: "remove"; path: string; recursive: boolean; force: boolean }
    | { kind: "rename"; path: string; destination: string }
    | { kind: "truncate"; path: string; size: number };

/** Ordinary tool execution through the manager, with no local file or process fallback. */
export class WorkspaceEnv implements ExecutionEnv {
    /** Includes the agent and reach because virtual paths name different private houses. */
    readonly id: string;
    /** Virtual working directory in the worker's namespace. */
    cwd: string;
    /** Stable receipt label for this supported reach. */
    readonly policyHash: string;
    readonly #connection: WorkspaceConnection;
    readonly #identity: {
        workspace: string;
        agent: number;
        project: boolean;
        network: boolean;
        read_only?: boolean;
    };
    readonly #active = new Set<string>();
    readonly #library:
        | ((context: Context) => Promise<Result<LibraryFiles, FileError>>)
        | undefined;

    /** The manager connection belongs to the home; identity comes from its conversation records. */
    constructor(
        connection: WorkspaceConnection,
        identity: {
            workspace: string;
            agent: number;
            project: boolean;
            network: boolean;
            read_only?: boolean;
        },
        library?: (
            context: Context,
        ) => Promise<Result<LibraryFiles, FileError>>,
    ) {
        this.#connection = connection;
        this.#identity = identity;
        this.#library = library;
        this.id = `workspace:${connection.url}:${identity.workspace}:${identity.agent}:${identity.project}:${identity.read_only === true}`;
        this.cwd = identity.project ? "/project" : "/house";
        this.policyHash = `workspace-v1:${identity.project ? "project" : "house"}:${identity.network ? "public-http-proxy" : "netless"}:${identity.read_only ? "read-only" : "writable"}`;
    }

    async #post(message: unknown): Promise<Static<typeof response>> {
        try {
            const reply = await fetch(
                `${this.#connection.url.replace(/\/$/, "")}/v1`,
                {
                    method: "POST",
                    redirect: "error",
                    headers: {
                        authorization: `Bearer ${this.#connection.token}`,
                        "content-type": "application/json",
                    },
                    body: JSON.stringify(message),
                    signal: AbortSignal.timeout(165_000),
                },
            );
            if (!reply.ok || reply.body === null) return uncertain();
            const chunks: Uint8Array[] = [];
            let size = 0;
            for await (const chunk of reply.body) {
                size += chunk.byteLength;
                if (size > 6 * 1024 * 1024) return uncertain();
                chunks.push(chunk);
            }
            const value: unknown = JSON.parse(Buffer.concat(chunks).toString());
            return Value.Check(response, value) ? value : uncertain();
        } catch {
            return uncertain();
        }
    }

    async #call<S extends TSchema>(
        operation: Operation,
        schema: S,
        context: Context,
    ): Promise<Result<Static<S>, Failure>> {
        let library: LibraryFiles | undefined;
        try {
            const snapshot = await this.#library?.(context);
            if (snapshot !== undefined && !snapshot.ok)
                return err({
                    kind: "error",
                    code: "unavailable",
                    message:
                        "The shared library could not be read safely within 1024 entries and 4 MiB. Ask the chief to inspect the shelf; no workspace operation was sent.",
                    uncertain: false,
                });
            library = snapshot?.value;
        } catch {
            return err({
                kind: "error",
                code: "unavailable",
                message:
                    "The shared library could not be read safely within 1024 entries and 4 MiB. Ask the chief to inspect the shelf; no workspace operation was sent.",
                uncertain: false,
            });
        }
        if (context.abortSignal?.aborted)
            return err({
                kind: "error",
                code: "aborted",
                message: "Workspace operation cancelled before sending.",
                uncertain: false,
            });
        const id = randomUUID();
        this.#active.add(id);
        const cancel = () => {
            void this.#post({ kind: "cancel", id });
        };
        context.abortSignal?.addEventListener("abort", cancel, { once: true });
        try {
            const reply = await this.#post({
                kind: "execute",
                request: {
                    version: 1,
                    id,
                    ...this.#identity,
                    operation,
                    ...(library === undefined ? {} : { library }),
                },
            });
            if (reply.kind === "error")
                return err({
                    ...reply,
                    message: `${reply.message} Request: ${id}.`,
                });
            if (!Value.Check(schema, reply.value)) return err(uncertain());
            return ok(reply.value);
        } finally {
            context.abortSignal?.removeEventListener("abort", cancel);
            this.#active.delete(id);
        }
    }

    async #file<S extends TSchema>(
        operation: Operation,
        schema: S,
        context: Context,
    ): Promise<Result<Static<S>, FileError>> {
        const result = await this.#call(operation, schema, context);
        return result.ok ? result : err(fileFailure(result.error));
    }

    #path(path: string): string {
        return posix.resolve(this.cwd, path);
    }

    /** Creates missing initial house files without replacing notes or following shelf links. */
    async initialize(context: Context): Promise<Result<void, FileError>> {
        return this.#void({ kind: "init_house" }, context);
    }

    /** Lists regular-file digests and modes without reading symlinks or special files. */
    async tree(
        path: string,
        caps: { entries: number; bytes: number },
        context: Context,
    ): Promise<Result<WorkspaceTreeEntry[], FileError>> {
        return this.#file(
            { kind: "tree_list", path: this.#path(path), ...caps },
            Type.Array(treeEntry),
            context,
        );
    }

    /** Claims a new tree file without replacing existing bytes, preserving ordinary permission bits. */
    async writeExclusive(
        path: string,
        content: Uint8Array,
        mode: number,
        context: Context,
    ): Promise<Result<void, FileError>> {
        if (content.byteLength > 4 * 1024 * 1024)
            return err(
                new FileError(
                    "not_supported",
                    "Remote file transfers are limited to 4 MiB.",
                ),
            );
        return this.#void(
            {
                kind: "tree_write",
                path: this.#path(path),
                data: Buffer.from(content).toString("base64"),
                mode,
            },
            context,
        );
    }

    /** Resolves only within the worker's POSIX namespace. */
    async absolutePath(
        path: string,
        _context: Context,
    ): Promise<Result<string, FileError>> {
        return ok(this.#path(path));
    }
    /** Joins virtual paths without consulting the host. */
    async joinPath(
        parts: string[],
        _context: Context,
    ): Promise<Result<string, FileError>> {
        return ok(posix.join(...parts));
    }
    /** Reads a regular file snapshot of at most 4 MiB. */
    async readBinaryFile(
        path: string,
        context: Context,
    ): Promise<Result<Uint8Array, FileError>> {
        const result = await this.#file(
            { kind: "read", path: this.#path(path), no_follow: false },
            readReply,
            context,
        );
        return result.ok
            ? ok(Buffer.from(result.value.data, "base64"))
            : result;
    }
    /** Decodes the bounded remote snapshot as UTF-8. */
    async readTextFile(
        path: string,
        context: Context,
    ): Promise<Result<string, FileError>> {
        const result = await this.readBinaryFile(path, context);
        return result.ok ? ok(new TextDecoder().decode(result.value)) : result;
    }
    /** Pins bytes and metadata together so a rename cannot switch the open file. */
    async openBinaryReader(
        path: string,
        options: { noFollow?: boolean } | undefined,
        context: Context,
    ): Promise<Result<BinaryReader, FileError>> {
        const result = await this.#file(
            {
                kind: "read",
                path: this.#path(path),
                no_follow: options?.noFollow === true,
            },
            readReply,
            context,
        );
        if (!result.ok) return result;
        const bytes = Buffer.from(result.value.data, "base64");
        let closed = false;
        const unavailable = (context: Context) =>
            closed || context.abortSignal?.aborted;
        const refused = () =>
            err<never, FileError>(
                new FileError(
                    "aborted",
                    "The remote file reader is closed or cancelled.",
                ),
            );
        return ok({
            info: async (context) =>
                unavailable(context) ? refused() : ok(result.value.info),
            read: async (offset, length, context) => {
                if (unavailable(context)) return refused();
                if (
                    !Number.isSafeInteger(offset) ||
                    offset < 0 ||
                    !Number.isSafeInteger(length) ||
                    length < 0
                )
                    return err(
                        new FileError(
                            "invalid",
                            "Choose non-negative integer byte offsets.",
                        ),
                    );
                return ok(bytes.subarray(offset, offset + length));
            },
            scanLines: async (options, context) => {
                if (unavailable(context)) return refused();
                const scanner = new LineScanner(
                    options.startLine,
                    options.endLine,
                );
                scanner.push(bytes);
                return ok(scanner.finish());
            },
            close: async () => {
                closed = true;
            },
        });
    }
    /** Reads lines from the same bounded snapshot until closed. */
    async openTextLineReader(
        path: string,
        context: Context,
    ): Promise<Result<TextLineReader, FileError>> {
        const result = await this.readTextFile(path, context);
        if (!result.ok) return result;
        let at = 0;
        let closed = false;
        return ok({
            readLine: async (context) => {
                if (closed || context.abortSignal?.aborted)
                    return err(
                        new FileError(
                            "aborted",
                            "The remote line reader is closed or cancelled.",
                        ),
                    );
                if (at >= result.value.length) return ok(undefined);
                const newline = result.value.indexOf("\n", at);
                const end = newline < 0 ? result.value.length : newline;
                const text = result.value.slice(at, end).replace(/\r$/, "");
                at = end + 1;
                return ok({ text, terminated: newline >= 0 });
            },
            close: async () => {
                closed = true;
            },
        });
    }
    /** Returns up to the requested number of snapshot lines. */
    async readTextLines(
        path: string,
        options: { maxLines?: number } | undefined,
        context: Context,
    ): Promise<Result<string[], FileError>> {
        const opened = await this.openTextLineReader(path, context);
        if (!opened.ok) return opened;
        const lines: string[] = [];
        try {
            while (
                lines.length < (options?.maxLines ?? Number.POSITIVE_INFINITY)
            ) {
                const line = await opened.value.readLine(context);
                if (!line.ok) return line;
                if (line.value === undefined) break;
                lines.push(line.value.text);
            }
            return ok(lines);
        } finally {
            await opened.value.close(context);
        }
    }
    /** Replaces a file remotely; an uncertain response is never replayed. */
    async writeFile(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#write(path, content, false, context);
    }
    /** Appends remotely without replaying uncertain mutations. */
    async appendFile(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#write(path, content, true, context);
    }
    async #write(
        path: string,
        content: string | Uint8Array,
        append: boolean,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const bytes =
            typeof content === "string"
                ? Buffer.from(content)
                : Buffer.from(content);
        if (bytes.length > 4 * 1024 * 1024)
            return err(
                new FileError(
                    "not_supported",
                    "Remote files are limited to 4 MiB per transfer.",
                ),
            );
        return this.#void(
            {
                kind: "write",
                path: this.#path(path),
                data: bytes.toString("base64"),
                append,
            },
            context,
        );
    }
    async #void(
        operation: Operation,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const result = await this.#file(operation, Type.Null(), context);
        return result.ok ? ok(undefined) : result;
    }
    /** Changes a remote file's length, up to 4 MiB. */
    async truncateFile(
        path: string,
        size: number,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#void(
            { kind: "truncate", path: this.#path(path), size },
            context,
        );
    }
    /** Flushes the remote file before returning. */
    async flushFile(
        path: string,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#void({ kind: "flush", path: this.#path(path) }, context);
    }
    /** Renames within the confined namespace. */
    async renameFile(
        path: string,
        destination: string,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#void(
            {
                kind: "rename",
                path: this.#path(path),
                destination: this.#path(destination),
            },
            context,
        );
    }
    /** Inspects a path without following its final symlink. */
    async fileInfo(
        path: string,
        context: Context,
    ): Promise<Result<FileInfo, FileError>> {
        return this.#file(
            { kind: "info", path: this.#path(path) },
            information,
            context,
        );
    }
    /** Lists at most 10,000 entries in a remote directory. */
    async listDir(
        path: string,
        context: Context,
    ): Promise<Result<FileInfo[], FileError>> {
        return this.#file(
            { kind: "list", path: this.#path(path) },
            Type.Array(information),
            context,
        );
    }
    /** Pages one bounded directory snapshot. */
    async openDirReader(
        path: string,
        context: Context,
    ): Promise<Result<DirReader, FileError>> {
        const listed = await this.listDir(path, context);
        if (!listed.ok) return listed;
        let at = 0;
        let closed = false;
        return ok({
            next: async (maxEntries, context) => {
                if (closed || context.abortSignal?.aborted)
                    return err(
                        new FileError(
                            "aborted",
                            "The remote directory reader is closed or cancelled.",
                        ),
                    );
                if (!Number.isSafeInteger(maxEntries) || maxEntries <= 0)
                    return err(
                        new FileError(
                            "invalid",
                            "Choose a positive page size.",
                        ),
                    );
                const entries = listed.value.slice(at, at + maxEntries);
                at += entries.length;
                return ok({ entries, done: at >= listed.value.length });
            },
            close: async () => {
                closed = true;
            },
        });
    }
    /** Remote watching is unavailable; callers must not watch local copies. */
    async watch(
        _targets: readonly WatchTarget[],
        _onChange: (change: WatchChange) => void,
        _context: Context,
    ): Promise<Result<FileWatcher, FileError>> {
        return err(
            new FileError(
                "not_supported",
                "Remote file watching is unavailable.",
            ),
        );
    }
    /** Resolves symlinks inside the confined namespace. */
    async canonicalPath(
        path: string,
        context: Context,
    ): Promise<Result<string, FileError>> {
        return this.#file(
            { kind: "canonical", path: this.#path(path) },
            Type.String(),
            context,
        );
    }
    /** Only a missing remote path counts as absent. */
    async exists(
        path: string,
        context: Context,
    ): Promise<Result<boolean, FileError>> {
        const result = await this.fileInfo(path, context);
        return result.ok
            ? ok(true)
            : result.error.code === "not_found"
              ? ok(false)
              : result;
    }
    /** Creates a directory remotely. */
    async createDir(
        path: string,
        options: { recursive?: boolean } | undefined,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#void(
            {
                kind: "mkdir",
                path: this.#path(path),
                recursive: options?.recursive === true,
            },
            context,
        );
    }
    /** Removes only paths visible to this operation. */
    async remove(
        path: string,
        options: { recursive?: boolean; force?: boolean } | undefined,
        context: Context,
    ): Promise<Result<void, FileError>> {
        return this.#void(
            {
                kind: "remove",
                path: this.#path(path),
                recursive: options?.recursive === true,
                force: options?.force === true,
            },
            context,
        );
    }
    /** Allocates spill storage in the agent's remote house. */
    async createTempDir(
        prefix: string | undefined,
        context: Context,
    ): Promise<Result<string, FileError>> {
        const path = `/house/.tmp/${posix.basename(prefix ?? "tmp")}-${randomUUID()}`;
        const made = await this.createDir(path, { recursive: true }, context);
        return made.ok ? ok(path) : made;
    }
    /** Allocates an empty file inside a fresh remote directory. */
    async createTempFile(
        options: { prefix?: string; suffix?: string } | undefined,
        context: Context,
    ): Promise<Result<string, FileError>> {
        const dir = await this.createTempDir(options?.prefix, context);
        if (!dir.ok) return dir;
        const path = posix.join(
            dir.value,
            `output${posix.basename(options?.suffix ?? ".tmp")}`,
        );
        const written = await this.writeFile(path, "", context);
        return written.ok ? ok(path) : written;
    }
    /** Buffers bounded output until the whole process tree exits; shell scrubbing remains caller-owned. */
    async exec(
        command: string | readonly string[],
        options: ShellExecOptions | undefined,
        context: Context,
    ): Promise<Result<ShellExecResult, ExecutionError>> {
        if (
            Object.keys(options?.env ?? {}).length !== 0 ||
            options?.inheritEnv === false ||
            options?.spill !== undefined
        )
            return err(
                new ExecutionError(
                    "unknown",
                    "Remote environment overrides and raw spill are unsupported; use the scrubbed tool adapter.",
                ),
            );
        const timeout = options?.timeout ?? 120;
        if (!Number.isFinite(timeout) || timeout <= 0 || timeout > 120)
            return err(
                new ExecutionError(
                    "unknown",
                    "Choose a timeout above zero and at most 120 seconds.",
                ),
            );
        const result = await this.#call(
            {
                kind: "exec",
                argv:
                    typeof command === "string"
                        ? ["/bin/bash", "-c", command]
                        : command,
                cwd: this.#path(options?.cwd ?? this.cwd),
                timeout_ms: Math.ceil(timeout * 1000),
            },
            commandReply,
            context,
        );
        if (!result.ok) {
            const code =
                result.error.code === "timeout"
                    ? "timeout"
                    : result.error.code === "aborted"
                      ? "aborted"
                      : "unknown";
            return err(new ExecutionError(code, result.error.message));
        }
        try {
            options?.onOutput?.(result.value.stdout, context, {
                stream: "stdout",
            });
            options?.onOutput?.(result.value.stderr, context, {
                stream: "stderr",
            });
        } catch (cause) {
            return err(
                new ExecutionError(
                    "callback_error",
                    "Remote output callback failed.",
                    cause instanceof Error ? cause : undefined,
                ),
            );
        }
        return ok({ exitCode: result.value.exitCode });
    }
    /** Cancels outstanding operations without deleting workspace files. */
    async cleanup(_context: Context): Promise<void> {
        await Promise.all(
            [...this.#active].map((id) => this.#post({ kind: "cancel", id })),
        );
    }
}

function uncertain(): Failure {
    return {
        kind: "error",
        code: "uncertain",
        message:
            "The workspace result is unknown. Inspect its files before repeating the operation.",
        uncertain: true,
    };
}
function fileFailure(failure: Failure): FileError {
    switch (failure.code) {
        case "aborted":
        case "not_found":
        case "permission_denied":
        case "not_directory":
        case "is_directory":
        case "invalid":
        case "not_supported":
            return new FileError(failure.code, failure.message);
        default:
            return new FileError("unknown", failure.message);
    }
}

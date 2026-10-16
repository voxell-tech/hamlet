import { constants, lstatSync, mkdirSync, type Stats } from "node:fs";
import { access, rmdir } from "node:fs/promises";
import { delimiter, isAbsolute, join, resolve } from "node:path";
import type { Context } from "@earendil-works/chord";
import {
    type BinaryReader,
    type DirReader,
    ExecutionError,
    err,
    FileError,
    type FileInfo,
    type FileWatcher,
    ok,
    type Result,
    type ShellExecOptions,
    type ShellExecResult,
    type TextLineReader,
    type WatchChange,
    type WatchTarget,
} from "@earendil-works/pi-durable/env";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { shellOutput } from "../output.ts";
import { FILE_READS, Files } from "./files.ts";
import {
    canon,
    directoryRoot,
    fixedRoots,
    type Limits,
    limitsOf,
    type Policy,
    policyHash,
    privateTmp,
    readable,
    tempRoot,
    writable,
} from "./policy.ts";
import { limited, platformSandbox, type Sandbox } from "./sandbox.ts";

type Reach = "read" | "write" | "move";

// A byte-order mark is kept, as the node environment keeps it.
const STRICT_UTF8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

function rootAt(path: string) {
    let info: Stats | undefined;
    try {
        info = lstatSync(resolve(path));
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    if (info && !info.isDirectory() && !info.isFile() && !info.isSocket())
        throw new Error(
            `The sandbox root must be a real file, directory or socket: ${path}`,
        );
    return { path, real: canon(path), dev: info?.dev, ino: info?.ino };
}

/** An `ExecutionEnv` whose commands and file operations keep to a policy (S12, C18). */
export class SandboxEnv extends NodeExecutionEnv {
    /** The {@link policyHash} of the policy and the sandbox the environment runs commands under. */
    readonly policyHash: string;
    readonly #policy: Policy;
    readonly #limits: Limits;
    readonly #sandbox: Sandbox;
    readonly #tmp: string;
    readonly #output: string;
    readonly #scoped: boolean;
    readonly #files: Files;
    readonly #roots: ReturnType<typeof rootAt>[];

    constructor(options: {
        cwd: string;
        policy: Policy;
        sandbox?: Sandbox;
        /** A name for a private temp folder of the environment's own, apart from every other scope of the same writable paths. `cleanup` removes the folder. */
        tmpScope?: string;
    }) {
        super({ cwd: options.cwd });
        for (const path of options.policy.rw) directoryRoot(path);
        for (const { socket } of options.policy.services)
            if (!lstatSync(socket).isSocket())
                throw new Error(
                    `The service endpoint must be a real socket: ${socket}`,
                );
        this.#limits = limitsOf(options.policy);
        this.#sandbox = options.sandbox ?? platformSandbox();
        this.policyHash = policyHash(options.policy, this.#sandbox.name);
        this.#tmp = privateTmp(options.policy, options.tmpScope);
        this.#output = join(this.#tmp, "tmp-files");
        directoryRoot(this.#output);
        mkdirSync(this.#output, { recursive: true, mode: 0o700 });
        this.#policy = {
            ...options.policy,
            fixed: [
                ...(options.policy.fixed ?? []),
                tempRoot(),
                this.#output,
                ...FILE_READS,
            ],
        };
        this.#roots = fixedRoots(this.#policy, this.#tmp).map(rootAt);
        this.#scoped = options.tmpScope !== undefined;
        this.#files = new Files(
            this.#policy,
            this.#sandbox,
            this.cwd,
            this.#tmp,
            this.#output,
        );
    }

    override async exec(
        command: string | readonly string[],
        options: ShellExecOptions | undefined,
        context: Context,
    ): Promise<Result<ShellExecResult, ExecutionError>> {
        const refused = this.#rootRefusal();
        if (refused) return err(new ExecutionError("spawn_error", refused));
        if (options?.spill !== undefined) {
            try {
                return await shellOutput(this, []).exec(
                    command,
                    options,
                    context,
                );
            } catch (error) {
                return err(new ExecutionError("unknown", String(error)));
            }
        }
        const argv =
            typeof command === "string" ? ["bash", "-c", command] : command;
        const [program] = argv;
        if (program === undefined) return super.exec(argv, options, context);
        const cwd = options?.cwd ? resolve(this.cwd, options.cwd) : this.cwd;
        // The sandbox's own program always starts, so a missing one is found
        // here, as the interface reports it.
        if (!(await this.#found(program, cwd))) {
            return err(
                new ExecutionError(
                    "spawn_error",
                    `${program}: command not found`,
                ),
            );
        }
        const confined = this.#sandbox.command(
            this.#policy,
            limited(this.#limits, argv),
            cwd,
            this.#tmp,
        );
        const { wallSecs } = this.#limits;
        return super.exec(
            confined.argv,
            {
                ...options,
                env: { ...confined.env, ...options?.env },
                inheritEnv: false,
                timeout: Math.min(options?.timeout ?? wallSecs, wallSecs),
            },
            context,
        );
    }

    override async readTextFile(
        path: string,
        context: Context,
    ): Promise<Result<string, FileError>> {
        const refused = await this.#refused(path, "read", context);
        if (refused) return err(refused);
        // A lossy read would let an edit write the replacement characters
        // back over the file.
        const bytes = await this.#call<Uint8Array>(
            "readBinaryFile",
            [path],
            context,
        );
        if (!bytes.ok) return bytes;
        try {
            return ok(STRICT_UTF8.decode(bytes.value));
        } catch {
            return err(
                new FileError("invalid", `${path} is not UTF-8 text`, path),
            );
        }
    }

    override async openTextLineReader(
        path: string,
        context: Context,
    ): Promise<Result<TextLineReader, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : this.#lines(path, context);
    }

    override async readTextLines(
        path: string,
        options: { maxLines?: number } | undefined,
        context: Context,
    ): Promise<Result<string[], FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused
            ? err(refused)
            : this.#call("readTextLines", [path, options], context);
    }

    override async readBinaryFile(
        path: string,
        context: Context,
    ): Promise<Result<Uint8Array, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused
            ? err(refused)
            : this.#call("readBinaryFile", [path], context);
    }

    override async openBinaryReader(
        path: string,
        options: { noFollow?: boolean } | undefined,
        context: Context,
    ): Promise<Result<BinaryReader, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : this.#binary(path, options, context);
    }

    override async listDir(
        path: string,
        context: Context,
    ): Promise<Result<FileInfo[], FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : this.#call("listDir", [path], context);
    }

    override async openDirReader(
        path: string,
        context: Context,
    ): Promise<Result<DirReader, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : this.#dir(path, context);
    }

    override async watch(
        targets: readonly WatchTarget[],
        onChange: (change: WatchChange) => void,
        context: Context,
    ): Promise<Result<FileWatcher, FileError>> {
        for (const { path } of targets) {
            const refused = await this.#refused(path, "read", context);
            if (refused) return err(refused);
        }
        return this.#watch(targets, onChange, context);
    }

    override async writeFile(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused
            ? err(refused)
            : this.#call("writeFile", [path, content], context);
    }

    override async appendFile(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused
            ? err(refused)
            : this.#call("appendFile", [path, content], context);
    }

    override async truncateFile(
        path: string,
        size: number,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused
            ? err(refused)
            : this.#call("truncateFile", [path, size], context);
    }

    override async flushFile(
        path: string,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused
            ? err(refused)
            : this.#call("flushFile", [path], context);
    }

    override async renameFile(
        sourcePath: string,
        destinationPath: string,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused =
            (await this.#refused(sourcePath, "move", context)) ??
            (await this.#refused(destinationPath, "move", context));
        return refused
            ? err(refused)
            : this.#call("renameFile", [sourcePath, destinationPath], context);
    }

    override async createDir(
        path: string,
        options: { recursive?: boolean } | undefined,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused
            ? err(refused)
            : this.#call("createDir", [path, options], context);
    }

    override async remove(
        path: string,
        options: { recursive?: boolean; force?: boolean } | undefined,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "move", context);
        return refused
            ? err(refused)
            : this.#call("remove", [path, options], context);
    }

    override async createTempDir(
        prefix: string | undefined,
        context: Context,
    ): Promise<Result<string, FileError>> {
        return this.#call("createTempDir", [prefix], context);
    }

    override async createTempFile(
        options: { prefix?: string; suffix?: string } | undefined,
        context: Context,
    ): Promise<Result<string, FileError>> {
        return this.#call("createTempFile", [options], context);
    }

    override async fileInfo(
        path: string,
        context: Context,
    ): Promise<Result<FileInfo, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : this.#call("fileInfo", [path], context);
    }

    override async canonicalPath(
        path: string,
        context: Context,
    ): Promise<Result<string, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused
            ? err(refused)
            : this.#call("canonicalPath", [path], context);
    }

    /** A house shelf listed and read in one sandboxed call. */
    async skillShelf(
        path: string,
        contents: boolean,
        context: Context,
    ): Promise<Result<import("../skill-paper.ts").SkillPaper[], FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused
            ? err(refused)
            : this.#call("skills", [path, contents], context);
    }

    override async cleanup(context: Context): Promise<void> {
        await super.cleanup(context);
        await this.#files.close();
        if (this.#scoped) {
            await this.exec(
                [
                    "/bin/bash",
                    "-c",
                    'shopt -s dotglob nullglob; rm -rf -- "$TMPDIR"/*',
                ],
                undefined,
                context,
            );
            // Mounted roots can only be removed after their sandbox ends.
            if (this.#rootRefusal() === undefined) {
                await rmdir(this.#output).catch(() => undefined);
                await rmdir(this.#tmp).catch(() => undefined);
            }
        }
    }

    #call<T>(
        method: string,
        args: unknown[],
        context: Context,
    ): Promise<Result<T, FileError>> {
        const refused = this.#rootRefusal();
        return refused
            ? Promise.resolve(err(new FileError("permission_denied", refused)))
            : this.#files.call(method, args, context, this.cwd);
    }

    async #lines(
        path: string,
        context: Context,
    ): Promise<Result<TextLineReader, FileError>> {
        const opened = await this.#call<number>(
            "openTextLineReader",
            [path],
            context,
        );
        if (!opened.ok) return opened;
        const id = opened.value;
        return ok({
            readLine: (context) =>
                this.#call("handle", [id, "readLine"], context),
            close: async (context) => {
                await this.#call("handle", [id, "close"], context);
            },
        });
    }

    async #binary(
        path: string,
        options: { noFollow?: boolean } | undefined,
        context: Context,
    ): Promise<Result<BinaryReader, FileError>> {
        const opened = await this.#call<number>(
            "openBinaryReader",
            [path, options],
            context,
        );
        if (!opened.ok) return opened;
        const id = opened.value;
        return ok({
            info: (context) => this.#call("handle", [id, "info"], context),
            read: (offset, length, context) =>
                this.#call("handle", [id, "read", offset, length], context),
            scanLines: (options, context) =>
                this.#call("handle", [id, "scanLines", options], context),
            close: async (context) => {
                await this.#call("handle", [id, "close"], context);
            },
        });
    }

    async #dir(
        path: string,
        context: Context,
    ): Promise<Result<DirReader, FileError>> {
        const opened = await this.#call<number>(
            "openDirReader",
            [path],
            context,
        );
        if (!opened.ok) return opened;
        const id = opened.value;
        return ok({
            next: (maxEntries, context) =>
                this.#call("handle", [id, "next", maxEntries], context),
            close: async (context) => {
                await this.#call("handle", [id, "close"], context);
            },
        });
    }

    async #watch(
        targets: readonly WatchTarget[],
        onChange: (change: WatchChange) => void,
        context: Context,
    ): Promise<Result<FileWatcher, FileError>> {
        const refused = this.#rootRefusal();
        if (refused) return err(new FileError("permission_denied", refused));
        const opened = await this.#files.call<{
            handle: number;
            event: number;
            mode: "native" | "polling";
        }>("watch", [targets], context, this.cwd, onChange);
        if (!opened.ok) return opened;
        const { handle, event, mode } = opened.value;
        return ok({
            mode,
            close: async (context) => {
                this.#files.unlisten(event);
                await this.#call("handle", [handle, "close"], context);
            },
        });
    }

    /** The reason the policy refuses `path` to `reach`, or `undefined` when it allows it. */
    async #refused(
        path: string,
        reach: Reach,
        context: Context,
    ): Promise<FileError | undefined> {
        const refused = this.#rootRefusal();
        if (refused) return new FileError("permission_denied", refused, path);
        const absolute = await super.absolutePath(path, context);
        if (!absolute.ok) return absolute.error;
        const real = absolute.value;
        const allowed =
            reach === "read"
                ? readable(this.#policy, this.#tmp, real)
                : writable(this.#policy, this.#tmp, real, reach === "move");
        return allowed
            ? undefined
            : new FileError(
                  "permission_denied",
                  `${path} is out of this agent's reach to ${reach === "read" ? "read" : "change"}`,
                  path,
              );
    }

    #rootRefusal(): string | undefined {
        try {
            for (const root of this.#roots) {
                const current = rootAt(root.path);
                if (
                    current.real !== root.real ||
                    current.dev !== root.dev ||
                    current.ino !== root.ino ||
                    (root.dev === undefined &&
                        this.#policy.rw.includes(root.path))
                )
                    throw new Error();
            }
        } catch {
            return "The sandbox root is no longer its real directory. The operation was refused.";
        }
        return undefined;
    }

    /** True when `program` names an executable file, on the policy's `PATH` when it is a bare name. */
    async #found(program: string, cwd: string): Promise<boolean> {
        const candidates = program.includes("/")
            ? [isAbsolute(program) ? program : resolve(cwd, program)]
            : (this.#policy.env.PATH ?? "")
                  .split(delimiter)
                  .filter((dir) => dir !== "")
                  .map((dir) => resolve(dir, program));
        for (const candidate of candidates) {
            try {
                await access(candidate, constants.X_OK);
                return true;
            } catch {}
        }
        return false;
    }
}

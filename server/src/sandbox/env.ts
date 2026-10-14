import { constants } from "node:fs";
import { access, mkdtemp, rm } from "node:fs/promises";
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
import {
    type Limits,
    limitsOf,
    type Policy,
    policyHash,
    privateTmp,
    readable,
    writable,
} from "./policy.ts";
import { platformSandbox, type Sandbox } from "./sandbox.ts";

type Reach = "read" | "write" | "move";

// A byte-order mark is kept, as the node environment keeps it.
const STRICT_UTF8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** `argv`, run by bash once `limits` hold for it and for every process it starts. */
function limited(limits: Limits, argv: readonly string[]): string[] {
    // bash counts a file's size in blocks of 1024 bytes. A limit is only
    // lowered, so where the machine allows less, its own limit stays.
    const script = (
        [
            ["t", limits.cpuSecs],
            ["u", limits.maxProcs],
            ["f", limits.maxFileMb * 1024],
            ["c", 0],
        ] as const
    )
        .map(
            ([flag, value]) =>
                `ulimit -${flag} ${value} 2>/dev/null || ulimit -${flag} hard`,
        )
        .join("\n");
    return ["bash", "-c", `${script}\nexec "$@"`, "bash", ...argv];
}

/** An `ExecutionEnv` whose commands and file operations keep to a policy (S12, C18). */
export class SandboxEnv extends NodeExecutionEnv {
    /** The {@link policyHash} of the policy and the sandbox the environment runs commands under. */
    readonly policyHash: string;
    readonly #policy: Policy;
    readonly #limits: Limits;
    readonly #sandbox: Sandbox;
    readonly #tmp: string;
    readonly #scoped: boolean;

    constructor(options: {
        cwd: string;
        policy: Policy;
        sandbox?: Sandbox;
        /** A name for a private temp folder of the environment's own, apart from every other scope of the same writable paths. `cleanup` removes the folder. */
        tmpScope?: string;
    }) {
        super({ cwd: options.cwd });
        this.#policy = options.policy;
        this.#limits = limitsOf(options.policy);
        this.#sandbox = options.sandbox ?? platformSandbox();
        this.policyHash = policyHash(options.policy, this.#sandbox.name);
        this.#tmp = privateTmp(options.policy, options.tmpScope);
        this.#scoped = options.tmpScope !== undefined;
    }

    override async exec(
        command: string | readonly string[],
        options: ShellExecOptions | undefined,
        context: Context,
    ): Promise<Result<ShellExecResult, ExecutionError>> {
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
        const bytes = await super.readBinaryFile(path, context);
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
        return refused ? err(refused) : super.openTextLineReader(path, context);
    }

    override async readTextLines(
        path: string,
        options: { maxLines?: number } | undefined,
        context: Context,
    ): Promise<Result<string[], FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused
            ? err(refused)
            : super.readTextLines(path, options, context);
    }

    override async readBinaryFile(
        path: string,
        context: Context,
    ): Promise<Result<Uint8Array, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : super.readBinaryFile(path, context);
    }

    override async openBinaryReader(
        path: string,
        options: { noFollow?: boolean } | undefined,
        context: Context,
    ): Promise<Result<BinaryReader, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused
            ? err(refused)
            : super.openBinaryReader(path, options, context);
    }

    override async listDir(
        path: string,
        context: Context,
    ): Promise<Result<FileInfo[], FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : super.listDir(path, context);
    }

    override async openDirReader(
        path: string,
        context: Context,
    ): Promise<Result<DirReader, FileError>> {
        const refused = await this.#refused(path, "read", context);
        return refused ? err(refused) : super.openDirReader(path, context);
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
        return super.watch(targets, onChange, context);
    }

    override async writeFile(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused ? err(refused) : super.writeFile(path, content, context);
    }

    override async appendFile(
        path: string,
        content: string | Uint8Array,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused
            ? err(refused)
            : super.appendFile(path, content, context);
    }

    override async truncateFile(
        path: string,
        size: number,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused ? err(refused) : super.truncateFile(path, size, context);
    }

    override async flushFile(
        path: string,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused ? err(refused) : super.flushFile(path, context);
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
            : super.renameFile(sourcePath, destinationPath, context);
    }

    override async createDir(
        path: string,
        options: { recursive?: boolean } | undefined,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "write", context);
        return refused ? err(refused) : super.createDir(path, options, context);
    }

    override async remove(
        path: string,
        options: { recursive?: boolean; force?: boolean } | undefined,
        context: Context,
    ): Promise<Result<void, FileError>> {
        const refused = await this.#refused(path, "move", context);
        return refused ? err(refused) : super.remove(path, options, context);
    }

    override async createTempDir(
        prefix: string | undefined,
        context: Context,
    ): Promise<Result<string, FileError>> {
        if (context.abortSignal?.aborted) {
            return err(new FileError("aborted", "aborted"));
        }
        try {
            return ok(await mkdtemp(join(this.#tmp, prefix ?? "tmp-")));
        } catch (error) {
            return err(new FileError("unknown", String(error), this.#tmp));
        }
    }

    override async cleanup(context: Context): Promise<void> {
        await super.cleanup(context);
        if (this.#scoped) await rm(this.#tmp, { recursive: true, force: true });
    }

    /** The reason the policy refuses `path` to `reach`, or `undefined` when it allows it. */
    async #refused(
        path: string,
        reach: Reach,
        context: Context,
    ): Promise<FileError | undefined> {
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

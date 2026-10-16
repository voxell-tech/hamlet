import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { dirname } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import type { Context } from "@earendil-works/chord";
import {
    err,
    FileError,
    type Result,
    type WatchChange,
} from "@earendil-works/pi-durable/env";
import { fileLimited, type Sandbox } from "./platform.ts";
import { limitsOf, type Policy } from "./policy.ts";
import { decode, encode, frameId } from "./wire.ts";

const FILE_HELPER = fileURLToPath(new URL("./file-helper.ts", import.meta.url));

/** The file helper's runtime and source files. */
export const FILE_READS = [
    process.execPath,
    dirname(
        fileURLToPath(
            import.meta.resolve("@earendil-works/pi-durable/env/node"),
        ),
    ),
    fileURLToPath(
        new URL(
            "../../package.json",
            import.meta.resolve("@earendil-works/pi-durable/env/node"),
        ),
    ),
    fileURLToPath(new URL("../../package.json", import.meta.url)),
    FILE_HELPER,
    fileURLToPath(new URL("../house/skill-paper.ts", import.meta.url)),
    fileURLToPath(new URL("./wire.ts", import.meta.url)),
];

/** File operations in one policy-confined process, started on its first request. */
export class Files {
    readonly #start: () => ChildProcessWithoutNullStreams;
    #child: ChildProcessWithoutNullStreams | undefined;
    readonly #ending = new Set<Promise<void>>();
    readonly #requestMs: number;
    #idle: NodeJS.Timeout | undefined;
    #closed = false;
    #next = 0;
    readonly #pending = new Map<
        number,
        (result: Result<unknown, FileError>) => void
    >();
    readonly #events = new Map<number, (change: WatchChange) => void>();

    /** Prepares the policy-confined helper, which starts on its first call. */
    constructor(
        policy: Policy,
        sandbox: Sandbox,
        cwd: string,
        tmp: string,
        output: string,
    ) {
        this.#requestMs = Math.min(30, limitsOf(policy).wallSecs) * 1000;
        this.#start = () => {
            const confined = sandbox.command(
                {
                    ...policy,
                    ro: [...policy.ro, ...FILE_READS],
                },
                fileLimited(limitsOf(policy), [
                    process.execPath,
                    "--experimental-strip-types",
                    FILE_HELPER,
                    tmp,
                    output,
                ]),
                cwd,
                tmp,
            );
            const [program, ...args] = confined.argv;
            if (program === undefined) throw new Error("No sandbox program");
            return spawn(program, args, {
                cwd,
                env: confined.env,
                stdio: "pipe",
            });
        };
    }

    /** Starts the helper on demand and correlates its reply with this request. */
    async call<T>(
        method: string,
        args: unknown[],
        context: Context,
        cwd: string,
        onChange?: (change: WatchChange) => void,
    ): Promise<Result<T, FileError>> {
        if (context.abortSignal?.aborted)
            return err(new FileError("aborted", "aborted"));
        if (this.#closed)
            return err(new FileError("invalid", "File environment is closed"));
        if (this.#child === undefined) {
            try {
                this.#child = this.#start();
            } catch (error) {
                return err(new FileError("unknown", String(error)));
            }
            const child = this.#child;
            const failed = () =>
                this.#stop(
                    child,
                    new FileError(
                        "unknown",
                        "The sandbox file helper stopped.",
                    ),
                );
            child.stdin.on("error", failed);
            child.stderr.resume();
            child.on("error", failed);
            const ended = new Promise<void>((resolve) =>
                child.once("close", () => {
                    failed();
                    resolve();
                }),
            );
            this.#ending.add(ended);
            void ended.then(() => this.#ending.delete(ended));
            createInterface({ input: child.stdout }).on("line", (line) => {
                const id = frameId(line);
                if (id === undefined) return;
                let reply: {
                    result: Result<unknown, FileError>;
                    event?: WatchChange;
                };
                try {
                    reply = decode(line) as typeof reply;
                    if (reply.event !== undefined) {
                        if (
                            typeof reply.event !== "object" ||
                            reply.event === null
                        )
                            throw new Error("Invalid watch event");
                    } else if (
                        typeof reply.result?.ok !== "boolean" ||
                        (!reply.result.ok &&
                            !(reply.result.error instanceof FileError))
                    )
                        throw new Error("Invalid file result");
                } catch {
                    const error = new FileError(
                        "invalid",
                        "Invalid sandbox file reply",
                    );
                    this.#pending.get(id)?.(err(error));
                    const listener = this.#events.get(id);
                    this.#events.delete(id);
                    listener?.({ error });
                    this.#rest();
                    return;
                }
                if (reply.event !== undefined)
                    this.#events.get(id)?.(reply.event);
                else this.#pending.get(id)?.(reply.result);
            });
        }
        clearTimeout(this.#idle);
        const child = this.#child;
        const id = this.#next++;
        if (onChange !== undefined)
            this.#events.set(id, (change) => {
                if (!context.abortSignal?.aborted) onChange(change);
            });
        return new Promise((resolve) => {
            const aborted = () => {
                // The response stays bounded and drained after its caller leaves.
                this.#events.delete(id);
                resolve(err(new FileError("aborted", "aborted")));
            };
            context.abortSignal?.addEventListener("abort", aborted, {
                once: true,
            });
            const timer = setTimeout(
                () =>
                    this.#stop(
                        child,
                        new FileError(
                            "unknown",
                            "The sandbox file request timed out.",
                        ),
                    ),
                this.#requestMs,
            );
            this.#pending.set(id, (result) => {
                clearTimeout(timer);
                this.#pending.delete(id);
                context.abortSignal?.removeEventListener("abort", aborted);
                if (!result.ok || context.abortSignal?.aborted)
                    this.#events.delete(id);
                resolve(result as Result<T, FileError>);
                this.#rest();
            });
            try {
                child.stdin.write(`${encode({ id, method, args, cwd })}\n`);
            } catch (error) {
                this.#pending.get(id)?.(
                    err(new FileError("invalid", String(error))),
                );
            }
        });
    }

    /** Drops a watch listener and lets an unused helper become idle. */
    unlisten(id: number): void {
        this.#events.delete(id);
        this.#rest();
    }

    #rest(): void {
        clearTimeout(this.#idle);
        if (
            this.#child === undefined ||
            this.#pending.size > 0 ||
            this.#events.size > 0
        )
            return;
        const child = this.#child;
        this.#idle = setTimeout(
            () =>
                this.#stop(
                    child,
                    new FileError(
                        "unknown",
                        "The sandbox file helper is idle.",
                    ),
                ),
            30_000,
        );
        this.#idle.unref();
    }

    #stop(child: ChildProcessWithoutNullStreams, error: FileError): void {
        if (this.#child !== child) return;
        this.#child = undefined;
        clearTimeout(this.#idle);
        child.kill("SIGKILL");
        for (const finish of this.#pending.values()) finish(err(error));
        const listeners = [...this.#events.values()];
        this.#events.clear();
        for (const listener of listeners) listener({ error });
    }

    /** Refuses new calls, stops the helper and waits for its processes to end. */
    async close(): Promise<void> {
        this.#closed = true;
        if (this.#child !== undefined)
            this.#stop(
                this.#child,
                new FileError("invalid", "File environment is closed"),
            );
        await Promise.all(this.#ending);
    }
}

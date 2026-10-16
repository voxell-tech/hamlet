import type { Context } from "@earendil-works/chord";
import {
    type ExecutionEnv,
    getOrThrow,
    type ShellExecOptions,
    type ShellOutputInfo,
} from "@earendil-works/pi-durable/env";
import { Scrubber } from "./scrub.ts";

/** The shell environment whose streamed and spilled output is scrubbed before either leaves memory (C32). */
export function shellOutput(
    env: ExecutionEnv,
    secrets: readonly string[],
): ExecutionEnv {
    const exec: ExecutionEnv["exec"] = async (command, options, context) => {
        const scrubber = new Scrubber(secrets);
        const spill = new Spill(env, options?.spill, context);
        let last: ShellOutputInfo = { stream: "stdout" };
        const output = (text: string, info: ShellOutputInfo) => {
            if (text === "") return;
            spill.push(text);
            options?.onOutput?.(text, context, info);
        };
        const { spill: _spill, window: _window, ...rest } = options ?? {};
        const result = await env.exec(
            command,
            {
                ...rest,
                onOutput: (text, _context, info) => {
                    last = info;
                    output(scrubber.push(text), info);
                },
            },
            context,
        );
        output(scrubber.flush(), last);
        const path = await spill.finish();
        if (path !== undefined) {
            if (result.ok) result.value.spillPath = path;
            else result.error.spillPath = path;
        }
        return result;
    };
    return new Proxy(env, {
        get(target, property) {
            if (property === "exec") return exec;
            const value: unknown = Reflect.get(target, property, target);
            return typeof value === "function" ? value.bind(target) : value;
        },
    });
}

/** The complete scrubbed output, buffered only up to the spill threshold. */
class Spill {
    readonly #env: ExecutionEnv;
    readonly #limits: ShellExecOptions["spill"];
    readonly #context: Context;
    #buffer = "";
    #bytes = 0;
    #newlines = 0;
    #spilling = false;
    #path: string | undefined;
    #written = Promise.resolve();
    #error: unknown;

    constructor(
        env: ExecutionEnv,
        limits: ShellExecOptions["spill"],
        context: Context,
    ) {
        this.#env = env;
        this.#limits = limits;
        this.#context = context;
    }

    push(text: string): void {
        const limits = this.#limits;
        if (limits === undefined) return;
        if (!this.#spilling) {
            this.#buffer += text;
            this.#bytes += Buffer.byteLength(text);
            this.#newlines += text.split("\n").length - 1;
            const lines = this.#newlines + (text.endsWith("\n") ? 0 : 1);
            if (this.#bytes <= limits.afterBytes && lines <= limits.afterLines)
                return;
            this.#spilling = true;
            text = this.#buffer;
            this.#buffer = "";
        }
        this.#written = this.#written.then(async () => {
            if (this.#error !== undefined) return;
            try {
                this.#path ??= getOrThrow(
                    await this.#env.createTempFile(
                        { prefix: "pi-output-", suffix: ".log" },
                        this.#context,
                    ),
                );
                getOrThrow(
                    await this.#env.appendFile(this.#path, text, this.#context),
                );
            } catch (error) {
                this.#error = error;
            }
        });
    }

    async finish(): Promise<string | undefined> {
        await this.#written;
        if (this.#error !== undefined) throw this.#error;
        return this.#path;
    }
}

import type { Context } from "@earendil-works/chord";
import type {
    ToolExecutionApi,
    ToolExecutionResult,
} from "@earendil-works/pi-durable";
import { ExecutionError } from "@earendil-works/pi-durable/env";
import { createBashTool } from "@earendil-works/pi-durable/tools";
import { type Static, Type } from "typebox";
import { Value } from "typebox/value";
import { LEAVE, type Leave } from "../environment.ts";
import type { SandboxEnv } from "../sandbox.ts";
import type { Secrets } from "../scrub.ts";
import { toolFailureText } from "./failure.ts";

/** A confined command's ending, including the tool receipt and any retained shell output. */
export type ConfinedRun = {
    exitCode: number;
    timedOut: boolean;
    policy: string;
    result: ToolExecutionResult;
    output?: Static<typeof shellOutput>;
};

const shellOutput = Type.Object({
    output: Type.String(),
    truncated: Type.Boolean(),
    exitCode: Type.Number(),
    fullOutputPath: Type.Optional(Type.String()),
});

/** The gate's memoized leave, shared by the built-in shell and hand tools. */
export async function leaveFor(
    api: Pick<ToolExecutionApi, "memo">,
    context: Context,
): Promise<Leave> {
    return (await api.memo<Leave>(LEAVE, context)) ?? { net: false };
}

/** Runs argv directly, or a bash command through pi's spill-aware adapter, scrubbing both before output is kept. */
export async function runConfined(
    env: SandboxEnv,
    argv: readonly string[] | string,
    options: {
        secrets: Secrets;
        wallSecs: number;
        onOutput(text: string): void;
        api?: ToolExecutionApi;
    },
    context: Context,
): Promise<ConfinedRun> {
    const scrubbed = options.secrets.shell(env);
    let exitCode = -1;
    let timedOut = false;
    let result: ToolExecutionResult = {};
    let output: Static<typeof shellOutput> | undefined;
    const fail = (reason: string) =>
        options.onOutput(
            options.secrets.words(
                toolFailureText({
                    kind: "run-refused",
                    operation: "run-tool",
                    retrySafe: false,
                    reason,
                }),
            ),
        );
    if (!Number.isFinite(options.wallSecs) || options.wallSecs <= 0) {
        fail("Invalid timeout: must be a finite number of seconds");
    } else if (typeof argv === "string") {
        const api = options.api;
        if (api === undefined)
            throw new Error("A bash command requires the tool output adapter");
        try {
            result = await createBashTool().execute(
                { command: argv, timeout: options.wallSecs },
                {
                    ...api,
                    env: scrubbed,
                    output: options.onOutput,
                    outputWindow: undefined,
                },
                context,
            );
            if (!Value.Check(shellOutput, result.structuredOutput))
                throw new Error("Invalid shell structured output");
            output = result.structuredOutput;
            exitCode = output.exitCode;
        } catch (cause) {
            // SAFETY: pi turns an environment timeout into an Error without a typed code; other exceptions remain defects.
            // Verified by: tools.test.ts the shell's default and shortened timeout receipts.
            if (String(cause).includes("Command timed out")) timedOut = true;
            else if (cause instanceof ExecutionError) {
                switch (cause.code) {
                    case "aborted":
                        if (context.abortSignal?.aborted) throw cause;
                        fail(cause.message);
                        break;
                    case "shell_unavailable":
                    case "spawn_error":
                    case "unknown":
                        fail(cause.message);
                        break;
                    case "timeout":
                        timedOut = true;
                        break;
                    case "callback_error":
                        throw cause;
                    default: {
                        const unhandled: never = cause.code;
                        throw new Error(
                            `Unclassified execution failure: ${unhandled}`,
                        );
                    }
                }
            } else throw cause;
        }
    } else {
        const ran = await scrubbed.exec(
            [...argv],
            {
                timeout: options.wallSecs,
                onOutput: (text) => options.onOutput(text),
            },
            context,
        );
        if (ran.ok) exitCode = ran.value.exitCode;
        else if (ran.error.code === "timeout") timedOut = true;
        else {
            if (ran.error.code === "aborted" && context.abortSignal?.aborted)
                throw ran.error;
            fail(ran.error.message);
        }
    }
    if (timedOut) options.onOutput(`\n[timed out after ${options.wallSecs}s]`);
    return {
        exitCode,
        timedOut,
        policy: env.policyHash,
        result: {
            ...result,
            ...(exitCode !== 0 && { isError: true }),
            details: { exitCode, policy: env.policyHash, degraded: false },
        },
        ...(output === undefined ? {} : { output }),
    };
}

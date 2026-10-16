import { Type } from "@earendil-works/pi-ai";
import { defineTool } from "@earendil-works/pi-durable";
import { createBashTool } from "@earendil-works/pi-durable/tools";
import { envFor, placeOf } from "../environment.ts";
import type { Home } from "../home.ts";
import { SandboxEnv } from "../sandbox.ts";
import { toolFailureText } from "./failure.ts";
import { leaveFor, runConfined } from "./run.ts";

/** Seconds a command may run, which a model can ask to shorten and never to lengthen. */
const WALL_SECS = 120;

/** Words a sandbox refuses a write with: Seatbelt's, then bubblewrap's (tools.rs:498-507). */
const REFUSED = [
    "Operation not permitted",
    "Read-only file system",
    "Permission denied",
];

/** Said after a command the sandbox stopped, while the hamlet works on no folder. */
const OUTSIDE =
    "\n[The sandbox lets a command write only inside the workspace, which is its working directory. Use a relative path there, not /tmp or another folder.]";

/** Said after a command the sandbox stopped, while the hamlet works on a folder. */
const OUTSIDE_PROJECT =
    "\n[The sandbox lets a command write only in the project folder and in your own workspace, and keeps Hamlet's own folder closed to it.]";

/** Arguments shared by the shell and the gate. */
export const bashParameters = Type.Object({
    command: Type.String({ description: "Bash command to execute" }),
    timeout: Type.Optional(
        Type.Number({
            description: `Timeout in seconds, at most ${WALL_SECS}`,
        }),
    ),
    network: Type.Optional(
        Type.Boolean({
            description: "True when the command needs the network.",
        }),
    ),
});

/**
 * Hamlet's `bash`: pi's, with a `network` flag, a time limit of `wallSecs` that a model may only shorten, and Rust's
 * hints (C16). The call reaches the network only when its `hamlet.leave` grants it (C19), and every credential is
 * taken out of its output before pi keeps or shows any of it (C32). Its details say how it ended and under which
 * sandbox (ADR 0002).
 */
export function bashTool(home: Home, wallSecs = WALL_SECS) {
    const pi = createBashTool();
    return defineTool({
        name: pi.name,
        description: `${pi.description} It stops after ${wallSecs} seconds, or sooner with \`timeout\`.`,
        parameters: bashParameters,
        ...(pi.structuredOutputSchema && {
            structuredOutputSchema: pi.structuredOutputSchema,
        }),
        ...(pi.outputLimits && { outputLimits: pi.outputLimits }),
        execute: async ({ command, timeout }, api, context) => {
            const leave = await leaveFor(api, context);
            const place = await placeOf(home, api, api.conversationId, context);
            const env =
                leave?.net === true
                    ? await envFor(
                          home,
                          { conversationId: api.conversationId, read: api },
                          { net: true },
                          context,
                      )
                    : api.env;
            if (!(env instanceof SandboxEnv)) {
                return {
                    output: [
                        {
                            type: "text",
                            text: toolFailureText({
                                kind: "no-environment",
                                operation: "run-tool",
                                retrySafe: true,
                            }),
                        },
                    ],
                    isError: true,
                };
            }
            const folder = await env.fileInfo(env.cwd, context);
            if (!folder.ok || folder.value.kind !== "directory") {
                const text = `Error: the folder ${env.cwd} is gone. Tell the chief.`;
                return { output: [{ type: "text", text }], isError: true };
            }
            const out = (text: string) => {
                if (text !== "") api.output(text);
            };
            const ran = await runConfined(
                env,
                command,
                {
                    secrets: home.secrets,
                    wallSecs: Math.min(timeout ?? wallSecs, wallSecs),
                    onOutput: out,
                    api,
                },
                context,
            );
            if (ran.output === undefined) return ran.result;
            if (
                ran.exitCode !== 0 &&
                REFUSED.some((words) =>
                    api.retainedOutput().text.includes(words),
                )
            ) {
                out(place?.folder == null ? OUTSIDE : OUTSIDE_PROJECT);
            }
            const kept = api.retainedOutput();
            return {
                ...ran.result,
                structuredOutput: {
                    ...ran.output,
                    output: kept.text,
                    truncated: kept.truncated,
                },
            };
        },
    });
}

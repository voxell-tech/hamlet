/**
 * All are `replay: "safe"`. A read is rerun as a read. A change carries
 * `t<task id>`, which a rerun sends again, so the workspace answers the call
 * it kept and nothing runs twice.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    defineExtension,
    defineTool,
    type Extension,
    type ToolExecutionApi,
    type ToolExecutionResult,
    type ToolRegistration,
} from "@earendil-works/pi-durable";
import { omitUndefined } from "../actor/common/utils.ts";
import { failureOf } from "./chat-items.ts";
import type { Outcome } from "./messages/agent.ts";
import { type CallId, type Refused, u32 } from "./messages/shared.ts";
import {
    type Change,
    type ChangeEnd,
    type Need,
    type NotRunWhy,
    type Program,
    type ReadAnswer,
    type RunEnd,
    type Text,
    type ToolList,
    WORKSPACE,
} from "./messages/workspace.ts";
import {
    type CallEnd,
    callOf,
    callOut,
    failed,
    interrupted,
    MAY_HAVE_RUN,
    type Peers,
    thrown,
    words,
} from "./turn.ts";

export interface FileToolArgs {
    read: { path: string; offset?: number; limit?: number };
    write: { path: string; content: string };
    edit: { path: string; edits: { oldText: string; newText: string }[] };
    search: {
        pattern: string;
        path?: string;
        name?: string;
        ignore_case?: boolean;
    };
    find: { path?: string; name?: string };
}

export type FileTool = keyof FileToolArgs;

export type AbsoluteFileCall<N extends FileTool> = FileToolArgs[N] & {
    readonly path: string;
};

export type Execute<Args> = (
    args: Args,
    api: ToolExecutionApi,
    context: Context,
) => Promise<ToolExecutionResult>;

/** The seconds a script may run, as the workspace's `LONGEST`. */
const LONGEST = 120;

/** Words in a command's output that tell the sandbox stopped it (`shell.rs`). */
const REFUSED = [
    "Operation not permitted",
    "Read-only file system",
    "Permission denied",
];

/** Today's note after a command the sandbox stopped, while the hamlet works on no folder. */
const OUTSIDE =
    "\n[The sandbox lets a command write only inside the workspace, which is its working directory. Use a relative path there, not /tmp or another folder.]";

function wallOf(script: Extract<Program, { type: "script" }>): number {
    return Math.min((script.timeout_ms ?? LONGEST * 1000) / 1000, LONGEST);
}

const text = (description: string) => ({ type: "string", description });
const number = (description: string) => ({ type: "number", description });

/** Today's definitions, word for word (`crates/hamlet_server/src/work/tools.rs`). */
const FILE_TOOLS: {
    readonly [N in FileTool]: {
        readonly description: string;
        readonly parameters: object;
    };
} = {
    read: {
        description:
            "Read the contents of a file. Supports text files and images (jpg, png, gif, webp). Images are sent as attachments. For text files, output is truncated to 2000 lines or 50KB (whichever is hit first). Use offset/limit for large files. When you need the full file, continue with offset until complete.",
        parameters: {
            type: "object",
            required: ["path"],
            properties: {
                path: text("Path to the file to read (relative or absolute)"),
                offset: number("Line number to start reading from (1-indexed)"),
                limit: number("Maximum number of lines to read"),
            },
        },
    },
    write: {
        description:
            "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Automatically creates parent directories.",
        parameters: {
            type: "object",
            required: ["path", "content"],
            properties: {
                path: text("Path to the file to write (relative or absolute)"),
                content: text("Content to write to the file"),
            },
        },
    },
    edit: {
        description:
            "Edit a single file using exact text replacement. Every edits[].oldText must match a unique, non-overlapping region of the original file. If two changes affect the same block or nearby lines, merge them into one edit instead of emitting overlapping edits. Do not include large unchanged regions just to connect distant changes.",
        parameters: {
            type: "object",
            required: ["path", "edits"],
            properties: {
                path: text("Path to the file to edit (relative or absolute)"),
                edits: {
                    type: "array",
                    items: {
                        type: "object",
                        required: ["oldText", "newText"],
                        properties: {
                            oldText: text(
                                "Exact text for one targeted replacement. It must be unique in the original file and must not overlap with any other edits[].oldText in the same call.",
                            ),
                            newText: text(
                                "Replacement text for this targeted edit.",
                            ),
                        },
                    },
                    description:
                        "One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits. If two changes touch the same block or nearby lines, merge them into one edit instead.",
                },
            },
        },
    },
    search: {
        description:
            "Search the contents of files under a folder for a regular expression. Gives each match as path:line: text, up to 100. Skips .git and build folders such as target and node_modules. Never waits for approval: use it rather than grep or rg.",
        parameters: {
            type: "object",
            required: ["pattern"],
            properties: {
                pattern: text(
                    "A regular expression, such as `fn main` or `TODO|FIXME`.",
                ),
                path: text(
                    "The folder or file to search. Leave out for the folder commands run in.",
                ),
                name: text("Only files whose name matches, such as `*.rs`."),
                ignore_case: { type: "boolean" },
            },
        },
    },
    find: {
        description:
            "List a folder, or find files by name under it. Without `name` it lists what is directly in the folder. With `name` it gives every file under the folder whose name matches, up to 100. Never waits for approval: use it rather than ls or find.",
        parameters: {
            type: "object",
            properties: {
                path: text(
                    "The folder. Leave out for the folder commands run in.",
                ),
                name: text(
                    "A file name with `*` and `?`, such as `*.toml` or `mod.rs`. With a `/` in it, it is matched against the path under the folder.",
                ),
            },
        },
    },
};

/** Today's shell definition, word for word (`crates/hamlet_server/src/work/tools/shell.rs`). */
const BASH = {
    description:
        "Execute a bash command in the current working directory. Returns combined stdout and stderr. Output is truncated to last 2000 lines or 50KB (whichever is hit first). If truncated, full output is saved to a temp file. Optionally provide a timeout in seconds. It stops after 120 seconds, or sooner with `timeout`.",
    parameters: {
        type: "object",
        required: ["command"],
        properties: {
            command: text("Bash command to execute"),
            timeout: number("Timeout in seconds, at most 120"),
            network: {
                type: "boolean",
                description: "True when the command needs the network.",
            },
        },
    },
};

function fileTool<N extends FileTool>(
    name: N,
    execute: Execute<AbsoluteFileCall<N>>,
): ToolRegistration {
    return defineTool({
        name,
        ...FILE_TOOLS[name],
        replay: "safe",
        execute: (args, api, context) =>
            execute(args as AbsoluteFileCall<N>, api, context),
    });
}

export function editsOf(
    edits: FileToolArgs["edit"]["edits"],
): { old: string; new: string }[] {
    return edits.map((one) => ({ old: one.oldText, new: one.newText }));
}

function shown(answer: Text): ToolExecutionResult {
    return {
        output: [{ type: "text", text: answer.text }],
        diagnostics: answer.notes.map((note) => ({
            severity: note.severity,
            message: note.message,
            ...(note.code === "" ? {} : { code: note.code }),
        })),
    };
}

function later(text: string): ToolExecutionResult {
    return { ...words(text), details: { endsLater: true } };
}

export function endsLater(details: unknown): boolean {
    return (
        typeof details === "object" &&
        details !== null &&
        "endsLater" in details &&
        details.endsLater === true
    );
}

/** Its item stays open until the end of `call` arrives. */
function waits(
    what: string,
    needs: readonly Need[],
    call: CallId,
): ToolExecutionResult {
    return {
        ...words(
            `${what} waits for a chief to allow it (${needs.join(", ")}). Its end arrives as a new input.`,
        ),
        details: { endsLater: true, waiting: call },
    };
}

/** The words of a command's end that arrives as an input. */
function ran(end: RunEnd): ToolExecutionResult {
    if (end.type === "not_run") return failed(end.words);
    const cut =
        end.cut === undefined
            ? ""
            : `\n[Output is cut to its end: ${end.cut.total_lines} lines in all. The whole is in ${end.cut.full_output}]`;
    const exit =
        end.exit.type === "code"
            ? `exit code ${end.exit.code}`
            : end.exit.type === "failed"
              ? `failed: ${end.exit.words}`
              : end.exit.type === "cut_off"
                ? MAY_HAVE_RUN
                : end.exit.type.replace("_", " ");
    const result = words(
        `${end.output}${cut}\n[Command ${end.command}: ${exit}]`,
    );
    return end.exit.type === "code" && end.exit.code === 0
        ? result
        : { ...result, isError: true };
}

/**
 * The words of a command that ended inside its call, as today's bash
 * (`crates/hamlet_server/src/work/tools/shell.rs`) and hand tools
 * (`work/added.rs`) give them.
 */
function ranNow(end: RunEnd, program: Program): ToolExecutionResult {
    if (end.type === "not_run") return failed(end.words);
    const script = program.type === "script";
    // Today's hand tools keep the head of their output and name no file.
    const fullOutput =
        end.cut === undefined || !script
            ? []
            : [
                  {
                      severity: "info" as const,
                      code: "full_output",
                      message: `Full output: ${end.cut.full_output}`,
                  },
              ];
    switch (end.exit.type) {
        case "code": {
            const { code } = end.exit;
            // No message tells the agent its project folder, so the note is
            // the one for a hamlet with none.
            const stopped =
                script &&
                code !== 0 &&
                REFUSED.some((refused) => end.output.includes(refused));
            return {
                ...words(stopped ? `${end.output}${OUTSIDE}` : end.output),
                isError: code !== 0,
                diagnostics:
                    code === 0
                        ? fullOutput
                        : [
                              ...fullOutput,
                              {
                                  severity: "error",
                                  code: "exit_code",
                                  message: `Command exited with code ${code}`,
                              },
                          ],
            };
        }
        case "timed_out":
            // A hand tool's limit is in its manifest, which the agent does
            // not see.
            if (!script) return ran(end);
            return {
                ...words(
                    `${end.output}\n[timed out after ${wallOf(program)}s]`,
                ),
                isError: true,
                diagnostics: fullOutput,
            };
        case "failed":
            return script
                ? scriptFailed(end.output, end.exit.words)
                : thrown("tool_error", end.exit.words);
        case "cut_off":
            return interrupted(script ? "bash" : program.name);
        case "cancelled":
            return ran(end);
    }
}

/** A write's words; one that `created` its file says so to the transcript. */
export function wrote(path: string, created = false): ToolExecutionResult {
    const result = words(`Successfully wrote to ${path}`);
    return created ? { ...result, details: { created } } : result;
}

/** Today's edit words (`crates/hamlet_server/src/work/tools/files.rs`). */
export function replaced(
    path: string,
    blocks: number,
    diff: NonNullable<Change["diff"]>,
): ToolExecutionResult {
    return {
        ...words(`Successfully replaced ${blocks} block(s) in ${path}.`),
        details: {
            diff: diff.diff,
            patch: diff.patch,
            ...(diff.first_changed_line === undefined
                ? {}
                : { firstChangedLine: diff.first_changed_line }),
        },
    };
}

export function readResult(
    answer: Exclude<ReadAnswer, Refused>,
): ToolExecutionResult {
    switch (answer.type) {
        case "text":
            return shown(answer);
        case "image":
            return {
                output: [
                    { type: "image", data: answer.data, mimeType: answer.mime },
                ],
            };
        case "failed":
            return failed(answer.words);
    }
}

/**
 * The words of a write's or an edit's end. A diff is set only for an edit,
 * and today's edit words count its `blocks`, which an end that arrives as
 * an input does not carry.
 */
function changed(end: ChangeEnd, blocks?: number): ToolExecutionResult {
    if (end.type !== "changed") return failed(end.words);
    if (end.diff === undefined) return wrote(end.path, end.created);
    return blocks === undefined
        ? words(`Changed ${end.path}.\n${end.diff.diff}`)
        : replaced(end.path, blocks, end.diff);
}

/** The words of a call's end that arrives as an input. */
export function endWords(outcome: Outcome): string {
    const result =
        outcome.type === "run" ? ran(outcome.end) : changed(outcome.end);
    return (result.output ?? [])
        .map((part) => (part.type === "text" ? part.text : ""))
        .join("");
}

/**
 * A call's end that arrives as an input, as today closes its item: a
 * command's exit and time, and today's words for a call not run
 * (`crates/hamlet_server/src/calls.rs`).
 */
export function endOf(outcome: Outcome): CallEnd {
    if (outcome.end.type === "not_run") {
        return notRun(outcome.end.why, outcome.end.words);
    }
    if (outcome.type === "change") {
        return outcome.end.type === "changed"
            ? { type: "ran", exit: 0, created: outcome.end.created }
            : { type: "ran", exit: 1, failure: failureOf(outcome.end.words) };
    }
    const { exit, output, took_ms } = outcome.end;
    if (exit.type !== "code") {
        return {
            type: "ran",
            exit: -1,
            ms: took_ms,
            failure: failureOf(endWords(outcome)),
        };
    }
    return {
        type: "ran",
        exit: exit.code,
        ms: took_ms,
        ...(exit.code === 0 ? {} : { failure: failureOf(output) }),
    };
}

function notRun(why: NotRunWhy, words: string): CallEnd {
    if (why.type === "chief_refused") {
        return {
            type: "blocked",
            why: `the chief refused it: ${why.reason ?? "no reason given"}`,
        };
    }
    return why.type === "lapsed" && why.lapse === "stopped"
        ? { type: "aborted" }
        : { type: "blocked", why: words };
}

function read(peers: Peers): ToolRegistration {
    return fileTool("read", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "read_file",
            {
                path: args.path,
                ...omitUndefined({
                    offset:
                        args.offset === undefined
                            ? undefined
                            : u32(args.offset),
                    limit:
                        args.limit === undefined ? undefined : u32(args.limit),
                }),
            },
            readResult,
            "read",
        ),
    );
}

async function run(
    peers: Peers,
    api: ToolExecutionApi,
    context: Context,
    program: Program,
): Promise<ToolExecutionResult> {
    return callOut(
        peers,
        api,
        context,
        WORKSPACE,
        "run",
        { program },
        (answer) => {
            switch (answer.type) {
                case "ended":
                    return ranNow(answer.end, program);
                case "running":
                    return later(
                        `Command ${answer.command} is still running. Its end arrives as a new input.`,
                    );
                case "waits_for_chief":
                    return waits("The command", answer.needs, callOf(api));
            }
        },
        program.type === "script" ? "bash" : program.name,
    );
}

/** A script that failed after `output`, in today's words (`shell.rs`, `fail`). */
function scriptFailed(output: string, reason: string): ToolExecutionResult {
    return failed(
        `${output}Error: ${reason}. Inspect the output before trying again, or tell the chief.`,
    );
}

function bash(peers: Peers): ToolRegistration {
    return defineTool({
        name: "bash",
        ...BASH,
        replay: "safe",
        outputLimits: { retain: "tail" },
        execute: async (
            args: { command: string; timeout?: number; network?: boolean },
            api,
            context,
        ) => {
            if (args.timeout !== undefined && args.timeout <= 0) {
                return scriptFailed(
                    "",
                    "Invalid timeout: must be a finite number of seconds",
                );
            }
            return run(peers, api, context, {
                type: "script",
                script: args.command,
                network: args.network ?? false,
                ...(args.timeout === undefined
                    ? {}
                    : { timeout_ms: u32(args.timeout * 1000) }),
            });
        },
    });
}

function write(peers: Peers): ToolRegistration {
    return fileTool("write", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "write_file",
            { path: args.path, content: args.content },
            (answer) =>
                answer.type === "ended"
                    ? changed(answer.end)
                    : waits("The write", answer.needs, callOf(api)),
            "write",
        ),
    );
}

function edit(peers: Peers): ToolRegistration {
    return fileTool("edit", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "edit_file",
            { path: args.path, edits: editsOf(args.edits) },
            (answer) =>
                answer.type === "ended"
                    ? changed(answer.end, args.edits.length)
                    : waits("The edit", answer.needs, callOf(api)),
            "edit",
        ),
    );
}

function search(peers: Peers): ToolRegistration {
    return fileTool("search", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "search",
            {
                under: args.path,
                pattern: args.pattern,
                ignore_case: args.ignore_case ?? false,
                ...omitUndefined({ name: args.name }),
            },
            (answer) =>
                answer.type === "text" ? shown(answer) : failed(answer.words),
            "search",
        ),
    );
}

function find(peers: Peers): ToolRegistration {
    return fileTool("find", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "find",
            { under: args.path, ...omitUndefined({ name: args.name }) },
            (answer) =>
                answer.type === "text" ? shown(answer) : failed(answer.words),
            "find",
        ),
    );
}

export function workspaceTools(peers: Peers): Extension {
    return defineExtension({
        name: "hamlet.workspace",
        tools: [
            read(peers),
            write(peers),
            edit(peers),
            bash(peers),
            search(peers),
            find(peers),
        ],
    });
}

export async function handTools(
    peers: Peers,
    tools: () => Promise<ToolList>,
): Promise<Extension> {
    const list = await tools();
    return defineExtension({
        name: "hamlet.hand-tools",
        tools: list.tools.map((tool) =>
            defineTool({
                name: tool.name,
                description: tool.description,
                parameters: JSON.parse(tool.parameters) as object,
                replay: "safe",
                outputLimits: { retain: "tail" },
                execute: (args, api, context) =>
                    run(peers, api, context, {
                        type: "tool",
                        name: tool.name,
                        arguments: JSON.stringify(args),
                    }),
            }),
        ),
    });
}

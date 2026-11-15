/**
 * The tools that reach the workspace: the project's files, commands, and
 * the tools a chief added by hand. Each is a pi-durable tool whose
 * `execute` makes one call out and words its answer.
 *
 * All are `replay: "safe"`. A read is rerun as a read. A change carries
 * `t<task id>`, which `callOut` stamps and a rerun sends again, so the
 * workspace answers the call it kept and nothing runs twice.
 *
 * No tool holds the turn: `run` answers `running` or `waits_for_chief`, the
 * tool ends, and the end arrives later as the input `finished` admits.
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
import {
    type ChangeEnd,
    type Need,
    type Outcome,
    type Program,
    type RunEnd,
    type Text,
    WORKSPACE,
} from "./messages.ts";
import { callOut, failed, listTools, type Peers, words } from "./turn.ts";

/** The arguments of each file tool, as today's definitions name them. */
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

const text = (description: string) => ({ type: "string", description });
const number = (description: string) => ({ type: "number", description });

/** Today's definitions, word for word (`crates/hamlet_server/src/work/tools.rs:152-305`). */
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

/** Today's shell definition, word for word (`crates/hamlet_server/src/work/tools/shell.rs:180-226`). */
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

type Execute<Args> = (
    args: Args,
    api: ToolExecutionApi,
    context: Context,
) => Promise<ToolExecutionResult>;

function fileTool<N extends FileTool>(
    name: N,
    execute: Execute<FileToolArgs[N]>,
): ToolRegistration {
    return defineTool({
        name,
        ...FILE_TOOLS[name],
        replay: "safe",
        execute: (args, api, context) =>
            execute(args as FileToolArgs[N], api, context),
    });
}

/** The fields of `given` that are set. A message never carries an unset field. */
function set<T extends object>(
    given: T,
): { [K in keyof T]?: NonNullable<T[K]> } {
    return Object.fromEntries(
        Object.entries(given).filter(([, value]) => value !== undefined),
    ) as { [K in keyof T]?: NonNullable<T[K]> };
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

function waits(what: string, needs: readonly Need[]): ToolExecutionResult {
    return words(
        `${what} waits for a chief to allow it (${needs.join(", ")}). Its end arrives as a new input.`,
    );
}

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
              : end.exit.type.replace("_", " ");
    const result = words(
        `${end.output}${cut}\n[Command ${end.command}: ${exit}]`,
    );
    return end.exit.type === "code" && end.exit.code === 0
        ? result
        : { ...result, isError: true };
}

function changed(end: ChangeEnd): ToolExecutionResult {
    if (end.type !== "changed") return failed(end.words);
    return words(
        `${end.created ? "Created" : "Changed"} ${end.path}.${end.diff === undefined ? "" : `\n${end.diff.diff}`}`,
    );
}

/**
 * The words of a call's end, as its tool would have given them had the call
 * ended at once. `finished` admits them as its input's content, so the
 * model reads one form whichever way an end reaches it.
 */
export function endWords(outcome: Outcome): string {
    const result =
        outcome.type === "run" ? ran(outcome.end) : changed(outcome.end);
    return (result.output ?? [])
        .map((part) => (part.type === "text" ? part.text : ""))
        .join("");
}

/**
 * `read`, for a path of the project. A read carries no id: a rerun reads
 * again. `own-files.ts` wraps this tool, so a path in the agent's own
 * filesystem never gets here.
 */
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
                ...set({ offset: args.offset, limit: args.limit }),
            },
            (answer) => {
                switch (answer.type) {
                    case "text":
                        return shown(answer);
                    case "image":
                        return {
                            output: [
                                {
                                    type: "image",
                                    data: answer.data,
                                    mimeType: answer.mime,
                                },
                            ],
                        };
                    case "failed":
                        return failed(answer.words);
                }
            },
        ),
    );
}

/**
 * Runs `program` under this call's id. `run` answers within the workspace's
 * 5 s: `ended` with the end, or `running` or `waits_for_chief`, and then
 * the tool ends without it. The end arrives later through the agent's
 * `finished`, as a new input. `bash` and every hand tool run through this.
 */
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
                    return ran(answer.end);
                case "running":
                    return words(
                        `Command ${answer.command} is still running. Its end arrives as a new input.`,
                    );
                case "waits_for_chief":
                    return waits("The command", answer.needs);
            }
        },
    );
}

function bash(peers: Peers): ToolRegistration {
    return defineTool({
        name: "bash",
        ...BASH,
        replay: "safe",
        outputLimits: { retain: "tail" },
        execute: (
            args: { command: string; timeout?: number; network?: boolean },
            api,
            context,
        ) =>
            run(peers, api, context, {
                type: "script",
                script: args.command,
                network: args.network ?? false,
                ...(args.timeout === undefined
                    ? {}
                    : { timeout_ms: Math.round(args.timeout * 1000) }),
            }),
    });
}

/** `write`: `write_file` under the call's id. */
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
                    : waits("The write", answer.needs),
        ),
    );
}

/** `edit`: `edit_file` under the call's id. */
function edit(peers: Peers): ToolRegistration {
    return fileTool("edit", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "edit_file",
            {
                path: args.path,
                edits: args.edits.map((one) => ({
                    old: one.oldText,
                    new: one.newText,
                })),
            },
            (answer) =>
                answer.type === "ended"
                    ? changed(answer.end)
                    : waits("The edit", answer.needs),
        ),
    );
}

/** `search`: a read, with no id. */
function search(peers: Peers): ToolRegistration {
    return fileTool("search", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "search",
            {
                under: args.path ?? ".",
                pattern: args.pattern,
                ignore_case: args.ignore_case ?? false,
                ...set({ name: args.name }),
            },
            (answer) =>
                answer.type === "text" ? shown(answer) : failed(answer.words),
        ),
    );
}

/** `find`: a read, with no id. */
function find(peers: Peers): ToolRegistration {
    return fileTool("find", (args, api, context) =>
        callOut(
            peers,
            api,
            context,
            WORKSPACE,
            "find",
            { under: args.path ?? ".", ...set({ name: args.name }) },
            (answer) =>
                answer.type === "text" ? shown(answer) : failed(answer.words),
        ),
    );
}

/**
 * The extension `hamlet.workspace`: the six tools, with today's
 * definitions. Its file tools are exactly the names of `FileToolArgs`,
 * which `own-files.ts` routes.
 */
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

/**
 * The extension `hamlet.hand-tools`: one tool for each that `list_tools`
 * gives, which runs as `run{program: {type: "tool", name, arguments}}` under
 * its call id. The actor installs it when the list arrives, without waiting
 * for it, so a slow workspace does not hold `submit`'s 4 s.
 */
export async function handTools(peers: Peers): Promise<Extension> {
    // P6's seam: `list.faults` names the tools that did not load, for the prompt (C15).
    const list = await listTools(peers);
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

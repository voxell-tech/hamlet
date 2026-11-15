/**
 * The agent's own files. This extension offers no tool of its own: it
 * wraps each file tool of `hamlet.workspace`, so one `read` serves a
 * project path and a path in the agent's filesystem alike.
 *
 * A path at or below the filesystem's root is own, and goes to the
 * adapter in this process. Every other path goes on to the wrapped tool,
 * which calls the workspace. A relative path, and a left-out one, start
 * from the filesystem's root.
 *
 * The wrapped tools are `replay: "safe"`. An own read, write, search or
 * find is safe too: a rerun gives the same. An own `edit` is not, because a
 * rerun of an edit that already ran would not find its old text. So its
 * route marks the call in `api.memo` before it edits, and a rerun that finds
 * the mark answers that the outcome is unknown, as pi-durable does for an
 * unsafe tool.
 */
import { isAbsolute } from "node:path";
import type { Context } from "@earendil-works/chord/context";
import {
    defineExtension,
    type Extension,
    type ToolExecutionApi,
    type ToolExecutionResult,
    type ToolRegistration,
    type Wrap,
} from "@earendil-works/pi-durable";
import { omitUndefined } from "../actor/common/utils.ts";
import {
    type AgentFilesystem,
    FOUND,
    type Found,
    LOOKED,
} from "./filesystem.ts";
import { failed, type Peers, unknownOutcome, words } from "./turn.ts";
import {
    type Execute,
    editsOf,
    type FileCall,
    type FileTool,
    type FileToolArgs,
} from "./workspace-tools.ts";

/**
 * Where a file tool's `path` leads: the absolute path, and whether it is
 * at or below the root of the agent's filesystem.
 *
 * A relative path is joined, not normalised, so one that climbs out of the
 * root stays own and the adapter refuses it.
 */
function reached(
    files: AgentFilesystem,
    path: string | undefined,
): { readonly own: boolean; readonly path: string } {
    // No message tells the agent its project folder, so paths start from
    // its own filesystem. When one does, this is the project folder.
    const start = files.root;
    const absolute =
        path === undefined
            ? start
            : isAbsolute(path)
              ? path
              : `${start}/${path}`;
    return {
        own: absolute === files.root || absolute.startsWith(`${files.root}/`),
        path: absolute,
    };
}

/**
 * The route of each file tool. The type names every file tool of
 * `workspace-tools.ts`, so a file tool with no route does not compile.
 */
function routes(peers: Peers): {
    readonly [N in FileTool]: Execute<FileCall<N>>;
} {
    const { files } = peers;
    return {
        read: async (args, _api, context) => {
            const file = await files.read(args.path, context);
            return file.ok
                ? words(linesOf(file.value, args.offset, args.limit))
                : failed(file.error.message);
        },
        edit: async (args, api, context) => {
            if ((await api.memo<boolean>(EDIT_BEGAN, context)) === true) {
                return unknownOutcome(
                    `The edit of ${args.path}`,
                    "the agent stopped while it ran",
                );
            }
            await api.memo(EDIT_BEGAN, true, context);
            const edited = await files.edit(
                args.path,
                editsOf(args.edits),
                context,
            );
            return edited.ok
                ? words(`Changed ${args.path}.`)
                : failed(edited.error.message);
        },
        write: (args, _api, context) => written(files, args, context),
        search: (args, _api, context) => searched(files, args, context),
        find: (args, _api, context) => found(files, args, context),
    };
}

/**
 * The extension `hamlet.own-files`. pi-durable applies every wrap after it
 * has gathered every tool, so the order of the two extensions does not
 * matter.
 */
export function ownFiles(peers: Peers): Extension {
    const own = routes(peers);
    const wrapOf = <N extends FileTool>(name: N): Wrap => ({
        tool: name,
        wrap: (tool: ToolRegistration): ToolRegistration => ({
            ...tool,
            execute: (
                args: FileToolArgs[N],
                api: ToolExecutionApi,
                context: Context,
            ) => {
                const at = reached(peers.files, args.path);
                const resolved = { ...args, path: at.path };
                return at.own
                    ? own[name](resolved, api, context)
                    : tool.execute(resolved, api, context);
            },
        }),
    });
    return defineExtension({
        name: "hamlet.own-files",
        wraps: (Object.keys(own) as FileTool[]).map(wrapOf),
    });
}

/** The memo an own edit leaves before it runs. */
const EDIT_BEGAN = "hamlet.own-edit.began";

/** The text of `bytes` from line `offset`, at most `limit` lines, cut as the workspace cuts a read. */
function linesOf(bytes: Uint8Array, offset?: number, limit?: number): string {
    const lines = new TextDecoder().decode(bytes).split("\n");
    const from = Math.max(0, (offset ?? 1) - 1);
    return lines.slice(from, from + (limit ?? 2000)).join("\n");
}

async function written(
    files: AgentFilesystem,
    args: FileCall<"write">,
    context: Context,
): Promise<ToolExecutionResult> {
    const done = await files.write(args.path, args.content, context);
    return done.ok ? words(`Wrote ${args.path}.`) : failed(done.error.message);
}

async function searched(
    files: AgentFilesystem,
    args: FileCall<"search">,
    context: Context,
): Promise<ToolExecutionResult> {
    const found = await files.search(
        args.pattern,
        args.path,
        omitUndefined({
            ignoreCase: args.ignore_case,
            // As today's: an empty name is no name.
            name: args.name || undefined,
        }),
        context,
    );
    if (!found.ok) return failed(found.error.message);
    return reported(
        {
            ...found.value,
            found: found.value.found.map(
                (match) => `${match.path}:${match.line}: ${match.text}`,
            ),
        },
        "no match",
    );
}

async function found(
    files: AgentFilesystem,
    args: FileCall<"find">,
    context: Context,
): Promise<ToolExecutionResult> {
    if (!args.name) {
        const listed = await files.list(args.path, context);
        if (!listed.ok) return failed(listed.error.message);
        const names = listed.value
            .map((entry) =>
                entry.kind === "directory" ? `${entry.name}/` : entry.name,
            )
            .sort();
        return words(names.length === 0 ? "(empty)" : names.join("\n"));
    }
    const matched = await files.find(args.name, args.path, context);
    return matched.ok
        ? reported(matched.value, "no file by that name")
        : failed(matched.error.message);
}

/**
 * What a search or a find found, in today's words
 * (`crates/hamlet_server/src/work/tools/walk.rs:121-141`).
 */
function reported(found: Found<string>, none: string): ToolExecutionResult {
    const lines = found.found.length === 0 ? [none] : [...found.found];
    if (found.more) {
        lines.push(`[more than ${FOUND}. Narrow it with \`path\` or \`name\`]`);
    }
    if (found.capped) {
        lines.push(`[stopped after ${LOOKED} files. Narrow it with \`path\`]`);
    }
    return words(lines.join("\n"));
}

import { isAbsolute, posix } from "node:path";
import type { Context } from "@earendil-works/chord/context";
import {
    generateDiffString,
    generateUnifiedPatch,
} from "@earendil-works/pi-coding-agent";
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
import type { Note, ReadAnswer, Refused } from "./messages.ts";
import { scrub } from "./models.ts";
import { failed, interrupted, type Peers, thrown, words } from "./turn.ts";
import {
    type AbsoluteFileCall,
    type Execute,
    editsOf,
    type FileTool,
    type FileToolArgs,
    readResult,
    replaced,
    wrote,
} from "./workspace-tools.ts";

function start(files: AgentFilesystem): string {
    // No message tells the agent its project folder, so paths start from
    // its own filesystem.
    return files.root;
}

/** A path is own when it normalises, as today's `footing` does, into the root. */
function reached(
    files: AgentFilesystem,
    path: string | undefined,
): { readonly own: boolean; readonly path: string } {
    const from = start(files);
    const normal = posix.normalize(
        path === undefined ? from : isAbsolute(path) ? path : `${from}/${path}`,
    );
    return {
        own: normal === files.root || normal.startsWith(`${files.root}/`),
        path: normal,
    };
}

function routes(peers: Peers): {
    readonly [N in FileTool]: Execute<AbsoluteFileCall<N>>;
} {
    const { files } = peers;
    return {
        read: async (args, _api, context) => {
            const file = await files.read(args.path, context);
            return file.ok
                ? readResult(
                      readOf(args.path, file.value, args.offset, args.limit),
                  )
                : failed(file.error.message);
        },
        edit: async (args, api, context) => {
            // A rerun of an edit that already ran would not find its old text,
            // so a rerun that finds the mark answers as pi-durable does for an
            // unsafe tool.
            if ((await api.memo<boolean>(EDIT_BEGAN, context)) === true) {
                return interrupted("edit");
            }
            await api.memo(EDIT_BEGAN, true, context);
            const edited = await files.edit(
                args.path,
                editsOf(args.edits),
                context,
            );
            if (!edited.ok) return thrown("tool_error", edited.error.message);
            const { before, after } = edited.value;
            const { diff, firstChangedLine } = generateDiffString(
                before,
                after,
            );
            return replaced(args.path, args.edits.length, {
                diff,
                patch: generateUnifiedPatch(args.path, before, after),
                ...(firstChangedLine === undefined
                    ? {}
                    : { first_changed_line: firstChangedLine }),
            });
        },
        write: (args, _api, context) => written(files, args, context),
        search: (args, _api, context) => searched(files, args, context),
        find: (args, _api, context) => found(files, args, context),
    };
}

/**
 * pi-durable applies every wrap after it has gathered every tool, so the
 * order of the two extensions does not matter.
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
                // The workspace scrubs its own results (C32).
                return at.own
                    ? own[name](resolved, api, context).then(scrub)
                    : tool.execute(resolved, api, context);
            },
        }),
    });
    return defineExtension({
        name: "hamlet.own-files",
        wraps: (Object.keys(own) as FileTool[]).map(wrapOf),
    });
}

const EDIT_BEGAN = "hamlet.own-edit.began";

/** As the workspace's (`crates/hamlet_actors/src/workspace.rs`). */
const MOST_LINES = 2000;
const MOST_BYTES = 50 * 1024;

/** Ported from `crates/hamlet_actors/src/workspace/files/read.rs`. */
function readOf(
    path: string,
    bytes: Uint8Array,
    offset?: number,
    limit?: number,
): Exclude<ReadAnswer, Refused> {
    const mime = imageOf(bytes);
    if (mime !== undefined) {
        return {
            type: "image",
            mime,
            data: Buffer.from(bytes).toString("base64"),
        };
    }
    // TextDecoder drops a leading BOM and replaces bytes that are not UTF-8.
    const lines = new TextDecoder().decode(bytes).split("\n");
    const total = lines.length;
    const start = Math.max(0, (offset ?? 0) - 1);
    if (start >= total) {
        return {
            type: "failed",
            words: `Offset ${offset ?? 0} is beyond end of file (${total} lines total)`,
        };
    }
    const end = limit === undefined ? total : Math.min(start + limit, total);
    const selected = lines.slice(start, end);
    const head = truncateHead(selected.join("\n"));
    const first = start + 1;
    const text = (shown: string, ...notes: Note[]) =>
        ({ type: "text", text: shown, notes }) as const;
    if (head.firstLineTooLong) {
        const line = Buffer.from(selected[0] ?? "");
        const cut = characterEnd(line, MOST_BYTES);
        return text(line.subarray(0, cut).toString(), {
            severity: "warn",
            code: "truncated",
            message: `Line ${first} is ${formatSize(line.length)}, exceeds the ${formatSize(MOST_BYTES)} limit; showing its first ${formatSize(cut)}. Use bash: sed -n '${first}p' ${path} | tail -c +${cut + 1}`,
        });
    }
    if (head.by !== undefined) {
        const last = first + head.lines - 1;
        const why =
            head.by === "lines" ? "" : ` (${formatSize(MOST_BYTES)} limit)`;
        return text(head.text, {
            severity: "info",
            code: "truncated",
            message: `Showing lines ${first}-${last} of ${total}${why}. Use offset=${last + 1} to continue.`,
        });
    }
    if (end < total) {
        return text(head.text, {
            severity: "info",
            code: "",
            message: `${total - end} more lines in file. Use offset=${end + 1} to continue.`,
        });
    }
    return text(head.text);
}

function truncateHead(content: string): {
    readonly text: string;
    readonly lines: number;
    readonly by?: "lines" | "bytes";
    readonly firstLineTooLong?: true;
} {
    const lines = content === "" ? [] : content.split("\n");
    if (content.endsWith("\n")) lines.pop();
    if (
        lines.length <= MOST_LINES &&
        Buffer.byteLength(content) <= MOST_BYTES
    ) {
        return { text: content, lines: lines.length };
    }
    if (Buffer.byteLength(lines[0] ?? "") > MOST_BYTES) {
        return { text: "", lines: 0, by: "bytes", firstLineTooLong: true };
    }
    const kept: string[] = [];
    let bytes = 0;
    let by: "lines" | "bytes" = "lines";
    for (const line of lines.slice(0, MOST_LINES)) {
        const size = Buffer.byteLength(line) + (kept.length > 0 ? 1 : 0);
        if (bytes + size > MOST_BYTES) {
            by = "bytes";
            break;
        }
        kept.push(line);
        bytes += size;
    }
    // Without a byte break, only omitted lines prove the line limit was
    // reached.
    if (by !== "bytes" && kept.length === lines.length) by = "bytes";
    return { text: kept.join("\n"), lines: kept.length, by };
}

/** `bytes` as pi writes a size, rounding a half up. */
function formatSize(bytes: number): string {
    const tenths = (unit: number) => {
        const count = Math.floor((bytes * 10 + unit / 2) / unit);
        return `${Math.floor(count / 10)}.${count % 10}`;
    };
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${tenths(1024)}KB`;
    return `${tenths(1024 * 1024)}MB`;
}

function characterEnd(bytes: Uint8Array, index: number): number {
    let end = index;
    while (end > 0 && ((bytes[end] ?? 0) & 0xc0) === 0x80) end--;
    return end;
}

function imageOf(bytes: Uint8Array): string | undefined {
    const at = (offset: number, ...want: number[]) =>
        want.every((byte, index) => bytes[offset + index] === byte);
    const ascii = (text: string) => [...text].map((c) => c.charCodeAt(0));
    if (at(0, 0xff, 0xd8, 0xff)) {
        return bytes[3] === 0xf7 ? undefined : "image/jpeg";
    }
    if (at(0, 0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)) {
        const header =
            bytes.length >= 16 &&
            at(8, 0, 0, 0, 13) &&
            at(12, ...ascii("IHDR"));
        return header ? "image/png" : undefined;
    }
    if (at(0, ...ascii("GIF87a")) || at(0, ...ascii("GIF89a"))) {
        return "image/gif";
    }
    if (at(0, ...ascii("RIFF")) && at(8, ...ascii("WEBP"))) {
        return "image/webp";
    }
    return undefined;
}

async function written(
    files: AgentFilesystem,
    args: AbsoluteFileCall<"write">,
    context: Context,
): Promise<ToolExecutionResult> {
    const done = await files.write(args.path, args.content, context);
    return done.ok ? wrote(args.path) : failed(done.error.message);
}

async function searched(
    files: AgentFilesystem,
    args: AbsoluteFileCall<"search">,
    context: Context,
): Promise<ToolExecutionResult> {
    if (args.pattern === "") {
        return failed("`pattern` is required. Supply a regular expression.");
    }
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
                (match) =>
                    `${shown(files, match.path)}:${match.line}: ${match.text}`,
            ),
        },
        "no match",
    );
}

function shown(files: AgentFilesystem, path: string): string {
    const from = `${start(files)}/`;
    const under = path.startsWith(from) ? path.slice(from.length) : "";
    // Today's `shown` (`walk.rs`) gives whole a name that begins with `..`.
    return under === "" || under.startsWith("..") ? path : under;
}

async function found(
    files: AgentFilesystem,
    args: AbsoluteFileCall<"find">,
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
    if (!matched.ok) return failed(matched.error.message);
    return reported(
        {
            ...matched.value,
            found: matched.value.found.map((path) => shown(files, path)),
        },
        "no file by that name",
    );
}

/** Today's words (`crates/hamlet_server/src/work/tools/walk.rs`). */
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

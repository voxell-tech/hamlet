import type { Context } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import { defineTool } from "@earendil-works/pi-durable";
import type { ExecutionEnv } from "@earendil-works/pi-durable/env";
import { linesOf } from "../change.ts";
import {
    envOf,
    FOUND,
    reached,
    report,
    shown,
    under,
    walk,
    wanted,
} from "./walk.ts";

/** The most characters of a line a search shows. */
const WIDTH = 300;

/** The largest file a search reads, in bytes. */
const LARGEST = 1024 * 1024;

const parameters = Type.Object({
    pattern: Type.String({
        description: "A regular expression, such as `fn main` or `TODO|FIXME`.",
    }),
    path: Type.Optional(
        Type.String({
            description:
                "The folder or file to search. Leave out for the folder commands run in.",
        }),
    ),
    name: Type.Optional(
        Type.String({
            description: "Only files whose name matches, such as `*.rs`.",
        }),
    ),
    ignore_case: Type.Optional(Type.Boolean()),
});

/** Hamlet's `search` (C16), which only reads and so never waits. */
export const Search = defineTool({
    name: "search",
    description:
        "Search the contents of files under a folder for a regular expression. Gives each match as path:line: text, up to 100. Skips .git and build folders such as target and node_modules. Never waits for approval: use it rather than grep or rg.",
    parameters,
    replay: "safe",
    execute: async (args, api, context) => ({
        output: [
            {
                type: "text",
                text: await search(envOf(api), args, context),
            },
        ],
    }),
});

/**
 * The lines matching a pattern in the text files under a folder, read through `env` (reading.rs:250-304). A model's
 * pattern runs in linear time, so none can stall the server; a file over 1 MiB, one that is not UTF-8 and one holding
 * NUL are passed over.
 */
export async function search(
    env: ExecutionEnv,
    args: {
        pattern: string;
        path?: string;
        name?: string;
        ignore_case?: boolean;
    },
    context: Context,
): Promise<string> {
    if (args.pattern === "") throw new Error("`pattern` is required");
    const matches = compile(args.pattern, args.ignore_case === true);
    const root = await reached(env, args.path, context);
    const found: string[] = [];
    const look = async (path: string, size: number) => {
        if (args.name && !wanted(args.name, under(root.full, path))) {
            return true;
        }
        if (size > LARGEST) return true;
        const read = await env.readTextFile(path, context);
        if (!read.ok || read.value.includes("\0")) return true;
        for (const [index, line] of linesOf(read.value).entries()) {
            if (!matches(line)) continue;
            found.push(
                `${shown(env, path)}:${index + 1}: ${clip(line.trimEnd())}`,
            );
            if (found.length > FOUND) return false;
        }
        return true;
    };
    let capped = false;
    if (root.info.kind === "directory") {
        capped = await walk(
            env,
            root.full,
            (path, file) => look(path, file.size),
            context,
        );
    } else {
        await look(root.full, root.info.size);
    }
    return report(found, capped, "no match");
}

/**
 * A test of a line against `pattern`, run by V8's linear engine. That engine takes no case-insensitive flag, so for
 * `ignoreCase` the pattern and the line are put in lower case, but for the letter that follows a backslash.
 */
function compile(
    pattern: string,
    ignoreCase: boolean,
): (line: string) => boolean {
    let source = pattern;
    if (ignoreCase) {
        source = "";
        for (let i = 0; i < pattern.length; i++) {
            const c = pattern[i] ?? "";
            const escaped = c === "\\" && i + 1 < pattern.length;
            source += escaped ? `${c}${pattern[++i]}` : c.toLowerCase();
        }
    }
    let regex: RegExp;
    try {
        regex = new RegExp(source, "l");
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (message.endsWith("linear time")) {
            throw new Error(
                "`pattern` holds a backreference, a lookaround or another part a search in linear time cannot take",
            );
        }
        const reason = message.slice(message.lastIndexOf("/l: ") + 4);
        throw new Error(`\`pattern\` is not a regular expression: ${reason}`);
    }
    return ignoreCase
        ? (line) => regex.test(line.toLowerCase())
        : (line) => regex.test(line);
}

function clip(line: string): string {
    const characters = [...line];
    return characters.length <= WIDTH
        ? line
        : `${characters.slice(0, WIDTH).join("")}...`;
}

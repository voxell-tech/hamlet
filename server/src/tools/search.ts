import type { Context } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import { defineTool } from "@earendil-works/pi-durable";
import type { ExecutionEnv } from "@earendil-works/pi-durable/env";
import { clip, linesOf } from "../change.ts";
import { err, ok, type Result } from "../result.ts";
import { type SearchFailure, toolFailureText } from "./failure.ts";
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
    execute: async (args, api, context) => {
        const env = envOf(api, "search");
        const result = env.ok ? await search(env.value, args, context) : env;
        return {
            output: [
                {
                    type: "text",
                    text: result.ok
                        ? result.value
                        : toolFailureText(result.error),
                },
            ],
            ...(!result.ok && { isError: true }),
        };
    },
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
): Promise<Result<string, SearchFailure>> {
    if (args.pattern === "")
        return err({
            kind: "pattern-empty",
            operation: "search",
            retrySafe: true,
        });
    const compiled = compile(args.pattern, args.ignore_case === true);
    if (!compiled.ok) return compiled;
    const matches = compiled.value;
    const reachedRoot = await reached(env, args.path, context, "search");
    if (!reachedRoot.ok) return reachedRoot;
    const root = reachedRoot.value;
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
                `${shown(env, path)}:${index + 1}: ${clip(line.trimEnd(), WIDTH, { flat: false })}`,
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
    return ok(report(found, capped, "no match"));
}

/**
 * A test of a line against `pattern`, run by V8's linear engine. That engine takes no case-insensitive flag, so for
 * `ignoreCase` the pattern and the line are put in lower case, but for the letter that follows a backslash.
 */
function compile(
    pattern: string,
    ignoreCase: boolean,
): Result<(line: string) => boolean, SearchFailure> {
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
        // SAFETY: V8's linear-regex rejection has no typed error code; only its known suffix classifies this case.
        // Verified by: search.test.ts rejects a backreference separately from an invalid pattern.
        if (message.endsWith("linear time"))
            return err({
                kind: "pattern-not-linear",
                operation: "search",
                retrySafe: true,
            });
        const reason = message.slice(message.lastIndexOf("/l: ") + 4);
        return err({
            kind: "pattern-invalid",
            operation: "search",
            retrySafe: true,
            reason,
        });
    }
    return ok(
        ignoreCase
            ? (line: string) => regex.test(line.toLowerCase())
            : (line: string) => regex.test(line),
    );
}

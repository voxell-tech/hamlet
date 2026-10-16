import type { Context } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import { defineTool } from "@earendil-works/pi-durable";
import type { ExecutionEnv } from "@earendil-works/pi-durable/env";
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

const parameters = Type.Object({
    path: Type.Optional(
        Type.String({
            description:
                "The folder. Leave out for the folder commands run in.",
        }),
    ),
    name: Type.Optional(
        Type.String({
            description:
                "A file name with `*` and `?`, such as `*.toml` or `mod.rs`. With a `/` in it, it is matched against the path under the folder.",
        }),
    ),
});

/** Hamlet's `find` (C16), which only reads and so never waits. */
export const Find = defineTool({
    name: "find",
    description:
        "List a folder, or find files by name under it. Without `name` it lists what is directly in the folder. With `name` it gives every file under the folder whose name matches, up to 100. Never waits for approval: use it rather than ls or find.",
    parameters,
    replay: "safe",
    execute: async (args, api, context) => {
        const env = envOf(api, "find");
        const result = env.ok ? await find(env.value, args, context) : env;
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

/** A folder's entries, sorted with `/` after each folder, or the files under it whose name `name` matches, read through `env` (reading.rs:306-345). */
export async function find(
    env: ExecutionEnv,
    args: { path?: string; name?: string },
    context: Context,
): Promise<Result<string, SearchFailure>> {
    const reachedRoot = await reached(env, args.path, context, "find");
    if (!reachedRoot.ok) return reachedRoot;
    const root = reachedRoot.value;
    if (root.info.kind !== "directory") {
        return err({
            kind: "not-a-folder",
            operation: "find",
            retrySafe: true,
            path: shown(env, root.full),
        });
    }
    const { name } = args;
    if (!name) {
        const listed = await env.listDir(root.full, context);
        if (!listed.ok)
            return err({
                kind: "unreadable",
                operation: "find",
                retrySafe: true,
                path: shown(env, root.full),
                reason: listed.error.code,
            });
        const entries = listed.value
            .map((entry) =>
                entry.kind === "directory" ? `${entry.name}/` : entry.name,
            )
            .sort();
        return ok(entries.length === 0 ? "(empty)" : entries.join("\n"));
    }
    const found: string[] = [];
    const capped = await walk(
        env,
        root.full,
        async (path) => {
            if (wanted(name, under(root.full, path)))
                found.push(shown(env, path));
            return found.length <= FOUND;
        },
        context,
    );
    return ok(report(found, capped, "no file by that name"));
}

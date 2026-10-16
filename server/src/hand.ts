import { realpath, stat } from "node:fs/promises";
import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { defineExtension, defineTool } from "@earendil-works/pi-durable";
import { type FileWatcher, getOrThrow } from "@earendil-works/pi-durable/env";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { toolRunner } from "./environment.ts";
import {
    type Manifest,
    type ManifestFault,
    ManifestsDoc,
    parseManifest,
    reservedNames,
} from "./hand/manifests.ts";
import type { Home, Part } from "./home.ts";
import { err, type Result } from "./result.ts";
import { safeValue } from "./scrub.ts";

import { leaveFor, runConfined, toolFailureText } from "./tools.ts";

const NAME = "hamlet.hand";
/** The tools a chief added by hand (C17). */
export const Hand: Part = (home) => {
    const env = new NodeExecutionEnv({ cwd: home.layout.tools });
    let watcher: FileWatcher | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let line = Promise.resolve();
    let stopped = false;
    const refresh = async (context: Context) => {
        const tools: Manifest[] = [];
        const faults: ManifestFault[] = [];
        const names = new Set<string>();
        const reserved = reservedNames(home.registry);
        const listed = await env.listDir(env.cwd, context);
        const entries = listed.ok ? listed.value : [];
        if (!listed.ok && listed.error.code !== "not_found")
            faults.push({
                operation: "load-tool",
                dir: env.cwd,
                retrySafe: true,
                kind: "manifest-unreadable",
                reason: listed.error.message,
            });
        for (const entry of entries.sort((a, b) =>
            a.name < b.name ? -1 : 1,
        )) {
            if (entry.kind !== "directory" && entry.kind !== "symlink")
                continue;
            let dir = entry.path;
            if (entry.kind === "symlink") {
                try {
                    if (!(await stat(entry.path)).isDirectory()) {
                        faults.push({
                            operation: "load-tool",
                            dir,
                            retrySafe: true,
                            kind: "not-a-folder",
                        });
                        continue;
                    }
                    dir = await realpath(entry.path);
                } catch {
                    faults.push({
                        operation: "load-tool",
                        dir,
                        retrySafe: true,
                        kind: "not-a-folder",
                    });
                    continue;
                }
            }
            const loaded = await load(env, dir, context);
            if (!loaded.ok) {
                faults.push({ ...loaded.error, dir: entry.path });
                continue;
            }
            const manifest = loaded.value;
            const facts = {
                operation: "load-tool" as const,
                dir: entry.path,
                retrySafe: true as const,
                name: manifest.name,
            };
            if (reserved.has(manifest.name)) {
                faults.push({ ...facts, kind: "name-reserved" });
                continue;
            }
            if (names.has(manifest.name)) {
                faults.push({ ...facts, kind: "name-taken" });
                continue;
            }
            names.add(manifest.name);
            tools.push(manifest);
        }
        await home.harness.commit(async (tx) => {
            Object.assign(await tx.doc(ManifestsDoc), {
                tools,
                faults: safeValue(home, faults),
            });
        }, context);
        home.registry.install(
            defineExtension({
                name: NAME,
                tools: tools.map((manifest) => handTool(home, manifest)),
            }),
        );
    };
    return {
        extension: defineExtension({ name: NAME }),
        async start(context) {
            watcher = getOrThrow(
                await env.watch(
                    [{ path: env.cwd, recursive: true }],
                    (change) => {
                        if (stopped) return;
                        if ("error" in change)
                            process.stderr.write(
                                `hamlet: tools watch failed: ${change.error.message}\n`,
                            );
                        clearTimeout(timer);
                        timer = setTimeout(() => {
                            line = line
                                .then(() => refresh(context))
                                .catch((error: unknown) => {
                                    process.stderr.write(
                                        `hamlet: could not load tools: ${String(error)}\n`,
                                    );
                                });
                        }, 100);
                    },
                    context,
                ),
            );
            try {
                line = refresh(context);
                await line;
            } catch (error) {
                stopped = true;
                clearTimeout(timer);
                await watcher.close(BACKGROUND_CONTEXT);
                throw error;
            }
        },
        async stop(context) {
            stopped = true;
            clearTimeout(timer);
            await watcher?.close(context);
            await line;
            await env.cleanup(context);
        },
    };
};

async function load(
    env: NodeExecutionEnv,
    dir: string,
    context: Context,
): Promise<Result<Manifest, ManifestFault>> {
    const read = await env.readTextFile(`${dir}/manifest.json`, context);
    return read.ok
        ? parseManifest(read.value, dir)
        : err({
              kind: "manifest-unreadable",
              operation: "load-tool",
              dir,
              retrySafe: true,
              reason: read.error.message,
          });
}

function handTool(home: Home, manifest: Manifest) {
    return defineTool({
        name: manifest.name,
        description: manifest.description,
        parameters: manifest.parameters,
        replay: "unsafe",
        outputLimits: { maxBytes: 64 * 1024 },
        execute: async (args, api, context) => {
            const leave = await leaveFor(api, context);
            const env = toolRunner(
                home,
                manifest,
                leave?.net === true,
                `${home.dir}:${api.conversationId}:${api.taskId}`,
            );
            try {
                const failed = (reason: string) => ({
                    isError: true,
                    output: [
                        {
                            type: "text" as const,
                            text: home.secrets.words(
                                toolFailureText({
                                    kind: "run-refused",
                                    operation: "run-tool",
                                    retrySafe: false,
                                    reason,
                                }),
                            ),
                        },
                    ],
                });
                const made = await env.createTempFile(
                    { prefix: "arguments-", suffix: ".json" },
                    context,
                );
                if (!made.ok) return failed(made.error.message);
                const file = made.value;
                const written = await env.writeFile(
                    file,
                    JSON.stringify(args),
                    context,
                );
                if (!written.ok) return failed(written.error.message);
                const ran = await runConfined(
                    env,
                    [
                        "bash",
                        "-c",
                        'file=$1; shift; exec "$@" < "$file"',
                        "bash",
                        file,
                        ...manifest.exec,
                    ],
                    {
                        secrets: home.secrets,
                        wallSecs: manifest.wallSecs ?? 120,
                        onOutput: (text) => api.output(text),
                    },
                    context,
                );
                return ran.result;
            } finally {
                await env.cleanup(BACKGROUND_CONTEXT);
            }
        },
    });
}

export {
    type Manifest,
    type ManifestFault,
    ManifestsDoc,
    manifestFaultText,
    parseManifest,
    reservedNames,
} from "./hand/manifests.ts";

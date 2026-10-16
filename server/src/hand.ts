import { realpath, stat } from "node:fs/promises";
import { resolve } from "node:path";
import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { TSchema } from "@earendil-works/pi-ai";
import {
    defineExtension,
    defineTool,
    type JsonObject,
} from "@earendil-works/pi-durable";
import { type FileWatcher, getOrThrow } from "@earendil-works/pi-durable/env";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { Compile } from "typebox/compile";
import { record } from "./catalogue.ts";
import { LEAVE, type Leave } from "./environment.ts";
import { type Manifest, ManifestsDoc } from "./hand/manifests.ts";
import type { Home, Part } from "./home.ts";
import { limitsOf, type Policy, SandboxEnv } from "./sandbox.ts";
import { shellOutput } from "./scrub.ts";

const NAME = "hamlet.hand";
const RESERVED = new Set([
    "bash",
    "powershell",
    "read",
    "write",
    "edit",
    "search",
    "find",
    "create_agent",
    "ask_agent",
    "tell_agent",
    "build_road",
    "request_road",
    "set_model",
    "remove_agent",
    "merge_agents",
    "call_vote",
    "cast_ballot",
    "agree_road",
    "agree_move",
    "ask_removal",
    "object",
    "ask_chief",
    "submit_skill",
    "ask_librarian",
]);

/** The tools a chief added by hand (C17). */
export const Hand: Part = (home) => {
    const env = new NodeExecutionEnv({ cwd: home.layout.tools });
    let watcher: FileWatcher | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let line = Promise.resolve();
    let stopped = false;
    const refresh = async (context: Context) => {
        const tools: Manifest[] = [];
        const faults: string[] = [];
        const names = new Set<string>();
        const installed = new Set(
            home.registry
                .snapshot()
                .tools()
                .filter(({ extension }) => extension.name !== NAME)
                .map(({ tool }) => tool.name),
        );
        const listed = await env.listDir(env.cwd, context);
        const entries = listed.ok ? listed.value : [];
        if (!listed.ok && listed.error.code !== "not_found")
            faults.push(`${env.cwd}: ${listed.error.message}`);
        for (const entry of entries.sort((a, b) =>
            a.name < b.name ? -1 : 1,
        )) {
            if (entry.kind !== "directory" && entry.kind !== "symlink")
                continue;
            try {
                if (
                    entry.kind === "symlink" &&
                    !(await stat(entry.path)).isDirectory()
                )
                    throw new Error("not a folder");
                const manifest = await load(
                    env,
                    entry.kind === "symlink"
                        ? await realpath(entry.path)
                        : entry.path,
                    context,
                );
                if (reserved(manifest.name) || installed.has(manifest.name))
                    throw new Error(`name '${manifest.name}' is reserved`);
                if (names.has(manifest.name))
                    throw new Error(
                        `name '${manifest.name}' is taken by another tool`,
                    );
                limitsOf(policyFor(home, manifest, false));
                names.add(manifest.name);
                tools.push(manifest);
            } catch (error) {
                faults.push(
                    `${entry.path}: ${error instanceof Error ? error.message : String(error)}`,
                );
            }
        }
        await home.harness.commit(async (tx) => {
            Object.assign(await tx.doc(ManifestsDoc), { tools, faults });
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

/** A name belonging to Hamlet's tools or pi's tools. */
export function reserved(name: string): boolean {
    return RESERVED.has(name);
}

async function load(
    env: NodeExecutionEnv,
    dir: string,
    context: Context,
): Promise<Manifest> {
    const text = getOrThrow(
        await env.readTextFile(`${dir}/manifest.json`, context),
    );
    let parsed: unknown;
    try {
        parsed = JSON.parse(text);
    } catch {
        throw new Error("manifest.json is not valid JSON");
    }
    const value = record(parsed);
    if (typeof value.name !== "string") throw new Error("missing name");
    if (!Array.isArray(value.exec)) throw new Error("exec must be an array");
    const exec = value.exec.filter(
        (part): part is string => typeof part === "string",
    );
    if (exec.length === 0) throw new Error("exec is empty");
    if (!/^[a-zA-Z0-9_-]{1,64}$/.test(value.name))
        throw new Error(`name '${value.name}' must match [a-zA-Z0-9_-]{1,64}`);
    const parameters =
        value.parameters === undefined
            ? { type: "object", properties: {} }
            : value.parameters;
    try {
        if (
            typeof parameters !== "object" ||
            parameters === null ||
            Array.isArray(parameters)
        )
            throw new Error("parameters must be a JSON Schema object");
        Compile(parameters as TSchema);
    } catch (error) {
        throw new Error(
            `parameters do not compile: ${error instanceof Error ? error.message : String(error)}`,
        );
    }
    const sandbox = record(value.sandbox);
    const limits = record(sandbox.limits);
    const seconds = (value: unknown): number | null =>
        typeof value === "number" && Number.isSafeInteger(value) && value >= 0
            ? value
            : null;
    const paths = (value: unknown): string[] =>
        Array.isArray(value)
            ? value
                  .filter((path): path is string => typeof path === "string")
                  .map((path) => resolve(dir, path))
            : [];
    return {
        name: value.name,
        description:
            typeof value.description === "string" ? value.description : "",
        dir,
        exec,
        parameters: parameters as JsonObject,
        net: sandbox.net === true,
        ro: [...paths(sandbox.fs_ro), dir],
        rw: paths(sandbox.fs_rw),
        env: Object.fromEntries(
            Object.entries(record(sandbox.env)).map(([name, value]) => [
                name,
                typeof value === "string" ? value : "",
            ]),
        ),
        wallSecs: seconds(limits.wall_secs),
        cpuSecs: seconds(limits.cpu_secs),
    };
}

function policyFor(home: Home, manifest: Manifest, net: boolean): Policy {
    return {
        ro: manifest.ro,
        rw: manifest.rw,
        open: false,
        hidden: [],
        locked: [
            home.layout.tools,
            manifest.dir,
            home.layout.gifts,
            home.layout.erasing,
        ],
        net: manifest.net && net,
        services: [],
        env: { ...home.sandbox.baseEnv(), ...manifest.env },
        limits: {
            ...(manifest.wallSecs !== null && { wallSecs: manifest.wallSecs }),
            ...(manifest.cpuSecs !== null && { cpuSecs: manifest.cpuSecs }),
        },
    };
}

function handTool(home: Home, manifest: Manifest) {
    return defineTool({
        name: manifest.name,
        description: manifest.description,
        parameters: manifest.parameters as TSchema,
        replay: "unsafe",
        outputLimits: { maxBytes: 64 * 1024 },
        execute: async (args, api, context) => {
            const leave = await api.memo<Leave>(LEAVE, context);
            const policy = policyFor(home, manifest, leave?.net === true);
            const env = new SandboxEnv({
                cwd: manifest.dir,
                policy,
                sandbox: home.sandbox,
                tmpScope: `${home.dir}:${api.conversationId}:${api.taskId}`,
            });
            try {
                const file = getOrThrow(
                    await env.createTempFile(
                        { prefix: "arguments-", suffix: ".json" },
                        context,
                    ),
                );
                getOrThrow(
                    await env.writeFile(file, JSON.stringify(args), context),
                );
                const result = await shellOutput(
                    env,
                    home.credentials.secrets(),
                ).exec(
                    [
                        "bash",
                        "-c",
                        'file=$1; shift; exec "$@" < "$file"',
                        "bash",
                        file,
                        ...manifest.exec,
                    ],
                    { onOutput: (text) => api.output(text) },
                    context,
                );
                const exitCode = result.ok ? result.value.exitCode : -1;
                if (!result.ok)
                    api.output(
                        result.error.code === "timeout"
                            ? `\n[timed out after ${limitsOf(policy).wallSecs}s]`
                            : `Error: ${result.error.message}`,
                    );
                return {
                    isError: exitCode !== 0,
                    details: {
                        exitCode,
                        policy: env.policyHash,
                        degraded: false,
                    },
                };
            } finally {
                await env.cleanup(BACKGROUND_CONTEXT);
            }
        },
    });
}

export { type Manifest, ManifestsDoc } from "./hand/manifests.ts";

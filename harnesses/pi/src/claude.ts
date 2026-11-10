// The provider `claude`: Claude models answered by Anthropic's own Claude program through its Agent SDK, on the
// chief's Claude login, which that program keeps in the runtime's folder (ADR 0014).
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
    chmodSync,
    existsSync,
    mkdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { delimiter, dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
    type Api,
    type AssistantMessage,
    createAssistantMessageEventStream,
    type Model,
    type ProviderAuthInteraction,
    type SimpleStreamOptions,
    type TranscriptContext,
} from "@earendil-works/pi-ai";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import { anthropicProvider } from "@earendil-works/pi-ai/providers/anthropic";

export const PROVIDER = "claude";

/** How the Claude program names Hamlet in the requests it makes. */
const APP = "hamlet/0.1.0";

/** How long `claude auth login` has to give its link. */
const LINK_WAIT_MS = 30_000;

/** The Claude program's folders: its own (`CLAUDE_CONFIG_DIR`) and the one it works in. */
export type Folders = { readonly config: string; readonly work: string };

/**
 * pi compacts and runs every tool, so the Claude program compacts
 * nothing, loads no MCP servers or tools of its own and adds no prompts pi did not write.
 */
const GUARDS = {
    DISABLE_AUTO_COMPACT: "1",
    ENABLE_CLAUDEAI_MCP_SERVERS: "0",
    ENABLE_TOOL_SEARCH: "false",
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
    CLAUDE_CODE_RESUME_INTERRUPTED_TURN_MAX_AGE_MS: "1",
    CLAUDE_CODE_SILENT_TURN_REMINDER: "0",
    CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH: "16384",
};

/** The packages `claude-query.ts` loads, optional dependencies of pi. */
const PACKAGES = [
    "@anthropic-ai/claude-agent-sdk",
    "@modelcontextprotocol/sdk/server/mcp.js",
    "cc-session-io",
];

/**
 * The provider, with its folders in `home`; none when the Claude program or
 * a package it is driven with is not installed.
 */
export function claudeProvider(home: string): Provider | undefined {
    if (!PACKAGES.every(installed)) return undefined;
    const program = executable();
    if (program === undefined) return undefined;
    const at = {
        config: join(home, "claude"),
        work: join(home, "claude-work"),
    };
    mkdirSync(at.config, { recursive: true });
    mkdirSync(at.work, { recursive: true });
    const streamed = (
        model: Model<Api>,
        context: TranscriptContext,
        options?: SimpleStreamOptions,
    ) => {
        const out = createAssistantMessageEventStream();
        // Loaded on first use, so pi runs without the optional Claude packages.
        import("./claude-query.ts").then(
            (loaded) =>
                loaded.answer({ at, program }, model, context, options, out),
            (error: unknown) => {
                const failed: AssistantMessage = {
                    ...emptyAnswer(model),
                    stopReason: "error",
                    errorMessage: String(error),
                };
                out.push({ type: "error", reason: "error", error: failed });
                out.end(failed);
            },
        );
        return out;
    };
    return createProvider({
        id: PROVIDER,
        name: "Claude",
        auth: {
            oauth: {
                name: "Claude",
                isSubscription: true,
                loginLabel: "Sign in to Claude",
                // The login stays in the Claude program; the server keeps only this marker that it was made.
                login: async (interaction) => {
                    await signIn(at, program, interaction);
                    return {
                        type: "oauth",
                        access: randomUUID(),
                        refresh: randomUUID(),
                        expires: Number.MAX_SAFE_INTEGER,
                    };
                },
                refresh: async (credential) => credential,
                toAuth: async (credential) => ({ apiKey: credential.access }),
            },
        },
        models: anthropicProvider()
            .getModels()
            .filter((m) => !/-20\d{6}$/.test(m.id))
            .map((m) => ({ ...m, provider: PROVIDER })),
        api: { stream: streamed, streamSimple: streamed },
    });
}

/** Whether `name` resolves from pi. */
function installed(name: string): boolean {
    try {
        import.meta.resolve(name);
        return true;
    } catch {
        return false;
    }
}

/** The Claude program: `HAMLET_CLAUDE_EXECUTABLE`, else the one the SDK's package ships for this machine. */
function executable(): string | undefined {
    const chosen = process.env.HAMLET_CLAUDE_EXECUTABLE;
    if (chosen !== undefined && chosen !== "") return chosen;
    const require = createRequire(import.meta.url);
    const file = process.platform === "win32" ? "claude.exe" : "claude";
    for (const libc of ["", "-musl"])
        try {
            return require.resolve(
                `@anthropic-ai/claude-agent-sdk-${process.platform}-${process.arch}${libc}/${file}`,
            );
        } catch {}
    return undefined;
}

/**
 * The Claude program's environment: this process's without the chief's own Anthropic and Claude variables, so the
 * realm's login is the one used, and with the runtime's Claude folder.
 */
export function environment(at: Folders): Record<string, string> {
    const env: Record<string, string> = {};
    for (const [name, value] of Object.entries(process.env))
        if (value !== undefined && !/^(ANTHROPIC_|CLAUDE|HAMLET_)/.test(name))
            env[name] = value;
    return {
        ...env,
        ...GUARDS,
        CLAUDE_CONFIG_DIR: at.config,
        CLAUDE_AGENT_SDK_CLIENT_APP: APP,
        // A script, as a test's fake program is, runs on the Node running pi.
        PATH: [dirname(process.execPath), env.PATH ?? ""].join(delimiter),
    };
}

/** An answer of `model` with nothing in it yet. */
export function emptyAnswer(model: Model<Api>): AssistantMessage {
    return {
        role: "assistant",
        content: [],
        api: model.api,
        provider: model.provider,
        model: model.id,
        usage: {
            input: 0,
            output: 0,
            cacheRead: 0,
            cacheWrite: 0,
            totalTokens: 0,
            cost: {
                input: 0,
                output: 0,
                cacheRead: 0,
                cacheWrite: 0,
                total: 0,
            },
        },
        stopReason: "stop",
        timestamp: Date.now(),
    };
}

/**
 * Runs `claude auth login` with `BROWSER` set to a script that writes the sign-in link to a file, relays that link,
 * and waits until the Claude program has the login. The code and the token stay with it.
 */
async function signIn(
    at: Folders,
    program: string,
    interaction: ProviderAuthInteraction,
): Promise<void> {
    const link = join(at.work, "sign-in-link");
    rmSync(link, { force: true });
    const script = /\.(mjs|cjs|js)$/.test(program);
    const args = ["auth", "login", "--claudeai"];
    const child = spawn(
        script ? process.execPath : program,
        script ? [program, ...args] : args,
        {
            cwd: at.work,
            env: { ...environment(at), BROWSER: opener(at, link) },
            stdio: ["ignore", "pipe", "pipe"],
            windowsHide: true,
        },
    );
    let said = "";
    child.stdout.on("data", (chunk) => {
        said += String(chunk);
    });
    child.stderr.on("data", (chunk) => {
        said += String(chunk);
    });
    const exited = new Promise<number | null>((resolve) =>
        child.once("close", resolve),
    );
    const { signal } = interaction;
    const stop = () => child.kill();
    signal.addEventListener("abort", stop, { once: true });
    try {
        const url = await Promise.race([
            linkIn(link, signal),
            exited.then(() => undefined),
        ]);
        if (url === undefined)
            throw new Error(
                signal.aborted
                    ? "Sign-in cancelled."
                    : `Claude's sign-in gave no link. ${lastLine(said)}`,
            );
        interaction.notify({
            type: "auth_url",
            url,
            instructions:
                "Sign in to Claude in a browser on the computer Hamlet's server runs on. The sign-in finishes in Claude's own program; Hamlet never sees your login.",
        });
        const code = await exited;
        if (signal.aborted) throw new Error("Sign-in cancelled.");
        if (code !== 0)
            throw new Error(`Claude's sign-in failed. ${lastLine(said)}`);
    } finally {
        signal.removeEventListener("abort", stop);
        child.kill();
        rmSync(link, { force: true });
    }
}

/** The program `BROWSER` names: it writes the link it is given to `link`. */
function opener(at: Folders, link: string): string {
    if (process.platform === "win32") {
        const script = join(at.work, "sign-in-link.mjs");
        writeFileSync(
            script,
            `import { writeFileSync } from "node:fs";\nwriteFileSync(${JSON.stringify(link)}, process.argv[2] ?? "");\n`,
        );
        const opener = join(at.work, "sign-in-link.cmd");
        writeFileSync(opener, `@"${process.execPath}" "${script}" %*\r\n`);
        return opener;
    }
    const opener = join(at.work, "sign-in-link.sh");
    writeFileSync(
        opener,
        `#!/bin/sh\nprintf '%s' "$1" > '${link.replaceAll("'", "'\\''")}'\n`,
    );
    chmodSync(opener, 0o700);
    return opener;
}

/** The link written to `link`, once it is there. */
async function linkIn(
    link: string,
    signal: AbortSignal,
): Promise<string | undefined> {
    const until = Date.now() + LINK_WAIT_MS;
    while (Date.now() < until && !signal.aborted) {
        if (existsSync(link)) {
            const url = readFileSync(link, "utf8").trim();
            if (url.startsWith("https://")) return url;
        }
        await sleep(100);
    }
    return undefined;
}

function lastLine(text: string): string {
    return (
        text
            .split("\n")
            .map((line) => line.trim())
            .filter((line) => line !== "")
            .at(-1) ?? ""
    );
}

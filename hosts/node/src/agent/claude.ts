/**
 * The provider `claude`: Claude models answered by Anthropic's own Claude
 * program through its Agent SDK, on the chief's Claude login, which that
 * program keeps in the realm's `claude/` folder (ADR 0014). Ported from
 * `harnesses/pi/src/claude.ts`; the calls are in `claude-query.ts`, loaded
 * at the first, so a host that never runs Claude never loads the SDK.
 */
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
    type MutableModels,
    type ProviderAuthInteraction,
    type SimpleStreamOptions,
    type TranscriptContext,
} from "@earendil-works/pi-ai";
import { createProvider } from "@earendil-works/pi-ai/models";
import { anthropicProvider } from "@earendil-works/pi-ai/providers/anthropic";
import { remember, scrub } from "./models.ts";
import { emptyAnswer } from "./spend.ts";
import { reasonOf } from "./turn.ts";

export const PROVIDER = "claude";

export interface ClaudeProgram {
    /** The program to run, from `claudeExecutable` or a test's fake. */
    readonly executable: string;
    /** The realm's `claude/`, which holds the program's own login. */
    readonly folder: string;
}

/** How the Claude program names Hamlet in the requests it makes. */
const APP = "hamlet/0.1.0";

/** How long `claude auth login` has to give its link. */
const LINK_WAIT_MS = 30_000;

/**
 * pi-durable compacts and runs every tool, so the Claude program compacts
 * nothing, loads no MCP servers or tools of its own and adds no prompts
 * Hamlet did not write.
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

/** The Claude program's folders: its own (`CLAUDE_CONFIG_DIR`) and the one it works in. */
export interface Folders {
    readonly config: string;
    readonly work: string;
}

export interface Program {
    readonly at: Folders;
    readonly executable: string;
}

/** The Claude program: `HAMLET_CLAUDE_EXECUTABLE`, else the one the SDK's package ships for this machine. */
export function claudeExecutable(): string | undefined {
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

/** `models` with the provider `claude`, run by `program`. */
export function withClaude(
    models: MutableModels,
    program: ClaudeProgram,
): MutableModels {
    const at = { config: program.folder, work: join(program.folder, "work") };
    mkdirSync(at.work, { recursive: true });
    // The program keeps the chief's login here, and `mkdir`'s mode yields
    // to the umask and leaves a folder already there as it was.
    chmodSync(at.config, 0o700);
    const claude: Program = { at, executable: program.executable };
    const streamed = (
        model: Model<Api>,
        context: TranscriptContext,
        options?: SimpleStreamOptions,
    ) => {
        const out = createAssistantMessageEventStream();
        void import("./claude-query.ts").then(
            (query) => query.answer(claude, model, context, options, out),
            (error: unknown) => {
                const failed: AssistantMessage = {
                    ...emptyAnswer(model),
                    stopReason: "error",
                    errorMessage: reasonOf(error),
                };
                out.push({ type: "error", reason: "error", error: failed });
                out.end(failed);
            },
        );
        return out;
    };
    models.setProvider(
        createProvider({
            id: PROVIDER,
            name: "Claude",
            auth: {
                // A request carries no key: the program uses its own login.
                apiKey: {
                    name: "Claude's own login",
                    resolve: async () => ({ auth: {} }),
                },
                oauth: {
                    name: "Claude",
                    isSubscription: true,
                    loginLabel: "Sign in to Claude",
                    // The login stays in the Claude program; the realm keeps only this marker that it was made.
                    login: async (interaction) => {
                        await signIn(claude, interaction);
                        return {
                            type: "oauth",
                            access: randomUUID(),
                            refresh: randomUUID(),
                            expires: Number.MAX_SAFE_INTEGER,
                        };
                    },
                    refresh: async (credential) => credential,
                    toAuth: async () => ({}),
                },
            },
            models: anthropicProvider()
                .getModels()
                .filter((m) => !/-20\d{6}$/.test(m.id))
                .map((m) => ({ ...m, provider: PROVIDER })),
            api: { stream: streamed, streamSimple: streamed },
        }),
    );
    return models;
}

/**
 * The Claude program's environment: this process's without the chief's own
 * Anthropic, Claude and Hamlet variables, so the realm's login is the one
 * used, and with the realm's Claude folder.
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
        // A script, as a test's fake program is, runs on the Node running the host.
        PATH: [dirname(process.execPath), env.PATH ?? ""].join(delimiter),
    };
}

/**
 * Adds the tokens of the login the Claude program keeps in its folder to
 * the host's scrub list, so the program's own words never show them.
 */
export function rememberLogin(at: Folders): void {
    try {
        const { claudeAiOauth: kept } = JSON.parse(
            readFileSync(join(at.config, ".credentials.json"), "utf8"),
        ) as { claudeAiOauth?: Record<string, unknown> };
        remember(
            ...[kept?.accessToken, kept?.refreshToken].filter(
                (token) => typeof token === "string",
            ),
        );
    } catch {
        // No login yet, or one kept otherwise; a parse error quotes the file, so it is not reported.
    }
}

/**
 * Runs `claude auth login` with `BROWSER` set to a script that writes the
 * sign-in link to a file, relays that link, and waits until the Claude
 * program has the login. The code and the token stay with it.
 */
async function signIn(
    claude: Program,
    interaction: ProviderAuthInteraction,
): Promise<void> {
    const { at, executable } = claude;
    const link = join(at.work, "sign-in-link");
    rmSync(link, { force: true });
    const script = /\.(mjs|cjs|js)$/.test(executable);
    const args = ["auth", "login", "--claudeai"];
    const child = spawn(
        script ? process.execPath : executable,
        script ? [executable, ...args] : args,
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
    const looked = new AbortController();
    const lastWords = () => {
        rememberLogin(at);
        return scrub(lastLine(said));
    };
    try {
        const url = await Promise.race([
            linkIn(link, AbortSignal.any([signal, looked.signal])),
            // A program that wrote its link and ended at once is not
            // waited on for the next look.
            exited.then(() => linkAt(link)),
        ]);
        if (url === undefined)
            throw new Error(
                signal.aborted
                    ? "Sign-in cancelled."
                    : `Claude's sign-in gave no link. ${lastWords()}`,
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
            throw new Error(`Claude's sign-in failed. ${lastWords()}`);
        rememberLogin(at);
    } finally {
        looked.abort();
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
        const url = linkAt(link);
        if (url !== undefined) return url;
        await sleep(100);
    }
    return undefined;
}

function linkAt(link: string): string | undefined {
    if (!existsSync(link)) return undefined;
    const url = readFileSync(link, "utf8").trim();
    return url.startsWith("https://") ? url : undefined;
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

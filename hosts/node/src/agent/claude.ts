/**
 * The provider `claude`: Claude models answered by Anthropic's own Claude
 * program through its Agent SDK, on the chief's Claude login, which that
 * program keeps in the realm's `claude/` folder (ADR 0014). Ported from
 * `harnesses/pi/src/claude.ts`, `claude-query.ts` and `claude-history.ts`.
 *
 * One call ends at Claude's tool calls and parks its query until
 * pi-durable brings their results in the next call; any other call starts
 * a query, which resumes a Claude Code session written from pi-durable's
 * history.
 */
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
    chmodSync,
    existsSync,
    mkdirSync,
    readFileSync,
    realpathSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { delimiter, dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
    type EffortLevel,
    type Options,
    type Query,
    query,
    type SDKAssistantMessage,
    type SDKMessage,
    type SDKPartialAssistantMessage,
    type SDKUserMessage,
} from "@anthropic-ai/claude-agent-sdk";
import {
    type Api,
    type AssistantMessage,
    type AssistantMessageEventStream,
    createAssistantMessageEventStream,
    type JsonObject,
    type Message,
    type Model,
    type MutableModels,
    type ProviderAuthInteraction,
    type SimpleStreamOptions,
    type Tool,
    type ToolResultMessage,
    type TranscriptContext,
} from "@earendil-works/pi-ai";
import { calculateCost, createProvider } from "@earendil-works/pi-ai/models";
import { anthropicProvider } from "@earendil-works/pi-ai/providers/anthropic";
import {
    getCurrentSystemPrompt,
    getCurrentTools,
} from "@earendil-works/pi-ai/utils/transcript";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import {
    CallToolRequestSchema,
    type CallToolResult,
    ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";

export const PROVIDER = "claude";

export interface ClaudeProgram {
    /** The program to run; tests name a fake that answers on loopback. */
    readonly executable: string;
    /** The realm's `claude/`, which holds the program's own login. */
    readonly folder: string;
}

/** How the Claude program names Hamlet in the requests it makes. */
const APP = "hamlet/0.1.0";

/** How long `claude auth login` has to give its link. */
const LINK_WAIT_MS = 30_000;

/** How long a query waits at its tool calls before it is stopped; their results then start a new one. */
const IDLE_MS = 5 * 60_000;

/** How Claude Code names a tool of the MCP server `hamlet`. */
const TOOL_PREFIX = "mcp__hamlet__";

/** The prompt of a query whose history ends at tool results its own query no longer waits for. */
const CONTINUE =
    "[Your session was interrupted and resumed. What precedes this ends with the tool result you were waiting for. Continue the task from there.]";

/** The words a turn fails with when Claude bills past the plan, which a subscription login is for. */
const OVERAGE =
    "Claude is billing this beyond your plan, to extra usage, so Hamlet stopped. It goes on once your plan's limit resets.";

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

/** pi's thinking levels as Claude's efforts, where a model's own map names none. */
const EFFORTS: Record<string, EffortLevel> = {
    minimal: "low",
    low: "low",
    medium: "medium",
    high: "high",
    xhigh: "max",
    max: "max",
};

/** The Claude program's folders: its own (`CLAUDE_CONFIG_DIR`) and the one it works in. */
interface Folders {
    readonly config: string;
    readonly work: string;
}

interface Program {
    readonly at: Folders;
    readonly executable: string;
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
        answer(claude, model, context, options, out);
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
function environment(at: Folders): Record<string, string> {
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

/** A query stopped at Claude's tool calls, by the pi-durable session it answers. */
interface Parked {
    readonly run: Run;
    /** Where pi-durable keeps the answer that made the calls. */
    readonly at: number;
    readonly calls: readonly string[];
    readonly timer: NodeJS.Timeout;
}

const parked = new Map<string, Parked>();

/** Answers one call of `model` into `out`. */
function answer(
    claude: Program,
    model: Model<Api>,
    context: TranscriptContext,
    options: SimpleStreamOptions | undefined,
    out: AssistantMessageEventStream,
): void {
    const messages = context.messages.filter((m) => m.role !== "system");
    const reply = new Reply(model, out, messages.length, options?.signal);
    // A summary pi-durable asks for runs apart, leaving the conversation's query be.
    const session =
        options?.cacheRetention === "none" ? undefined : options?.sessionId;
    const held = session === undefined ? undefined : parked.get(session);
    if (session !== undefined && held !== undefined) {
        parked.delete(session);
        clearTimeout(held.timer);
        if (held.run.refusal !== undefined) {
            reply.fail(held.run.refusal);
            return;
        }
        const results = answered(held, messages);
        if (!held.run.closed && results !== undefined) {
            held.run.resume(reply, results);
            return;
        }
        held.run.close();
    }
    new Run(claude, model, context, messages, options, reply, session);
}

/** The results this call brings for `held`'s calls; none when the history moved on otherwise. */
function answered(
    held: Parked,
    messages: readonly Message[],
): ToolResultMessage[] | undefined {
    const calls = messages[held.at];
    const after = messages.slice(held.at + 1);
    if (calls?.role !== "assistant") return undefined;
    const ids = calls.content.flatMap((b) =>
        b.type === "toolCall" ? [b.id] : [],
    );
    const results = after.filter((m) => m.role === "toolResult");
    return results.length === after.length &&
        ids.length === held.calls.length &&
        ids.every((id) => held.calls.includes(id)) &&
        results.every((r) => ids.includes(r.toolCallId))
        ? results
        : undefined;
}

/** One query of the Claude program, which may answer several calls of pi-durable. */
class Run {
    readonly #claude: Program;
    readonly #session: string | undefined;
    readonly #input = new Input();
    readonly #query: Query;
    /** MCP calls waiting for their results, and results that came before their call. */
    readonly #waiting = new Map<string, (result: CallToolResult) => void>();
    readonly #early = new Map<string, CallToolResult>();
    /** Messages already streamed as events, whose whole copies are passed over. */
    readonly #streamed = new Set<string>();
    /** The files of the Claude sessions this query wrote, removed once it ends. */
    readonly #sessions = new Set<string>();
    #reply: Reply | undefined;
    #closed = false;
    #refusal: string | undefined;

    constructor(
        claude: Program,
        model: Model<Api>,
        context: TranscriptContext,
        messages: readonly Message[],
        options: SimpleStreamOptions | undefined,
        reply: Reply,
        session: string | undefined,
    ) {
        this.#claude = claude;
        this.#session = session;
        const append = getCurrentSystemPrompt(context.messages);
        const turn = messages.findLastIndex((m) => m.role !== "user") + 1;
        const prompt = messages.slice(turn);
        const history =
            prompt.length === 0 ? messages : messages.slice(0, turn);
        const resume =
            history.length === 0
                ? undefined
                : importSession(history, claude.at, model.id);
        if (resume !== undefined)
            this.#sessions.add(sessionPath(resume, claude.at));
        this.#input.push(
            prompt.length === 0
                ? CONTINUE
                : // pi's images name their own media type, which Anthropic checks.
                  (prompt.flatMap((m) => {
                      if (m.role !== "user") return [];
                      const content = userContent(m);
                      return typeof content === "string"
                          ? [{ type: "text" as const, text: content }]
                          : content;
                  }) as Exclude<Content, string>),
        );
        this.#query = query({
            prompt: this.#input,
            options: {
                pathToClaudeCodeExecutable: claude.executable,
                cwd: claude.at.work,
                env: environment(claude.at),
                // Claude Code's own tools, settings, MCP servers and slash commands are off; the agent's tools run in pi-durable.
                tools: [],
                settingSources: [],
                strictMcpConfig: true,
                verbatimPrompts: true,
                includePartialMessages: true,
                systemPrompt: {
                    type: "preset",
                    preset: "claude_code",
                    ...(append === "" ? {} : { append }),
                },
                mcpServers: {
                    hamlet: this.#server(getCurrentTools(context.messages)),
                },
                canUseTool: async (name, input) =>
                    name.startsWith(TOOL_PREFIX)
                        ? { behavior: "allow", updatedInput: input }
                        : {
                              behavior: "deny",
                              message: `${name} is not one of this conversation's tools.`,
                          },
                model: model.id,
                ...thinking(model, options?.reasoning),
                ...(resume === undefined ? {} : { resume }),
                stderr: (data) => process.stderr.write(`claude: ${data}`),
            },
        });
        this.#attach(reply);
        void this.#consume();
    }

    get closed(): boolean {
        return this.#closed;
    }

    /** Why the query was stopped for good, when it was. */
    get refusal(): string | undefined {
        return this.#refusal;
    }

    /** Answers `reply` once `results` reach the calls they answer. */
    resume(reply: Reply, results: readonly ToolResultMessage[]): void {
        this.#attach(reply);
        for (const result of results) {
            const answer: CallToolResult = {
                content: result.content.map((block) =>
                    block.type === "text"
                        ? { type: "text", text: block.text }
                        : {
                              type: "image",
                              data: block.data,
                              mimeType: block.mimeType,
                          },
                ),
                isError: result.isError,
            };
            const waiting = this.#waiting.get(result.toolCallId);
            this.#waiting.delete(result.toolCallId);
            if (waiting === undefined)
                this.#early.set(result.toolCallId, answer);
            else waiting(answer);
        }
    }

    /** Stops the Claude program, failing the call it answers. */
    close(): void {
        if (this.#closed) return;
        this.#closed = true;
        this.#input.end();
        this.#query.close();
        for (const waiting of this.#waiting.values())
            waiting({
                content: [{ type: "text", text: "The query ended." }],
                isError: true,
            });
        this.#waiting.clear();
        this.#reply?.fail(
            this.#refusal ?? "The Claude program stopped before it answered.",
        );
        this.#reply = undefined;
    }

    #attach(reply: Reply): void {
        this.#reply = reply;
        reply.onAbort(() => this.close());
    }

    /** The MCP server `hamlet`, offering `tools` with their schemas as pi-durable has them. */
    #server(tools: readonly Tool[]) {
        const server = new McpServer(
            { name: "hamlet", version: "0.1.0" },
            { capabilities: { tools: {} } },
        );
        server.server.setRequestHandler(ListToolsRequestSchema, async () => ({
            tools: tools.map((tool) => ({
                name: tool.name,
                description: tool.description,
                inputSchema: { type: "object" as const, ...tool.parameters },
            })),
        }));
        server.server.setRequestHandler(CallToolRequestSchema, (request) => {
            // Claude Code names the tool_use each call is for, which pairs it with pi-durable's result.
            const id = request.params._meta?.["claudecode/toolUseId"];
            if (typeof id !== "string")
                throw new Error(`${request.params.name} came with no call id.`);
            const early = this.#early.get(id);
            this.#early.delete(id);
            return early === undefined
                ? new Promise<CallToolResult>((resolve) =>
                      this.#waiting.set(id, resolve),
                  )
                : early;
        });
        return { type: "sdk" as const, name: "hamlet", instance: server };
    }

    async #consume(): Promise<void> {
        try {
            for await (const message of this.#query) {
                this.#take(message);
                if (message.type === "result") break;
            }
        } catch (error) {
            this.#reply?.fail(
                error instanceof Error ? error.message : String(error),
            );
            this.#reply = undefined;
        } finally {
            this.close();
            for (const file of this.#sessions) deleteSession(file);
        }
    }

    #take(message: SDKMessage): void {
        const reply = this.#reply;
        switch (message.type) {
            case "system":
                if (message.subtype === "init")
                    this.#sessions.add(
                        sessionPath(message.session_id, this.#claude.at),
                    );
                return;
            case "rate_limit_event":
                if (message.rate_limit_info.isUsingOverage === true) {
                    this.#refusal = OVERAGE;
                    this.close();
                }
                return;
            case "stream_event": {
                const { event } = message;
                if (event.type === "message_start")
                    this.#streamed.add(event.message.id);
                reply?.event(event);
                if (event.type === "message_stop") this.#park();
                return;
            }
            case "assistant":
                // Claude Code's notices of its own failures, which the result reports.
                if (message.message.model === "<synthetic>") return;
                if (this.#streamed.has(message.message.id)) return;
                reply?.whole(message.message);
                this.#park();
                return;
            case "result":
                if (message.subtype !== "success")
                    reply?.fail(
                        message.errors.join("\n") ||
                            `Claude stopped: ${message.subtype}`,
                    );
                else if (message.is_error)
                    reply?.fail(message.result || "Claude reported an error.");
                else reply?.finish();
                this.#reply = undefined;
                return;
            default:
                return;
        }
    }

    /** Ends the call at Claude's tool calls, if it made any, and keeps the query for pi-durable's results. */
    #park(): void {
        const reply = this.#reply;
        if (reply === undefined || reply.calls.length === 0) return;
        this.#reply = undefined;
        reply.finish("toolUse");
        const session = this.#session;
        if (session === undefined) {
            this.close();
            return;
        }
        parked.set(session, {
            run: this,
            at: reply.at,
            calls: reply.calls,
            timer: setTimeout(() => {
                if (parked.get(session)?.run === this) parked.delete(session);
                this.close();
            }, IDLE_MS).unref(),
        });
    }
}

type StreamEvent = SDKPartialAssistantMessage["event"];
type Block = SDKAssistantMessage["message"]["content"][number];
type Delta = Extract<StreamEvent, { type: "content_block_delta" }>["delta"];
type Content = SDKUserMessage["message"]["content"];

/** Claude's token counts, as message events and whole messages carry them. */
interface Tokens {
    input_tokens?: number | null;
    output_tokens?: number | null;
    cache_read_input_tokens?: number | null;
    cache_creation_input_tokens?: number | null;
}

/** The answer to one call of pi-durable, written as Claude's message events come. */
class Reply {
    /** How many messages the call was given, which is where pi-durable keeps this answer. */
    readonly at: number;
    readonly #model: Model<Api>;
    readonly #out: AssistantMessageEventStream;
    readonly #signal: AbortSignal | undefined;
    readonly #message: AssistantMessage;
    /** The open blocks of the message being streamed, by Claude's index: where each is, and a call's input so far. */
    readonly #open = new Map<number, { at: number; json: string }>();
    #started = false;
    #ended = false;
    #stop: AssistantMessage["stopReason"] = "stop";
    #aborted: (() => void) | undefined;

    constructor(
        model: Model<Api>,
        out: AssistantMessageEventStream,
        at: number,
        signal: AbortSignal | undefined,
    ) {
        this.at = at;
        this.#model = model;
        this.#out = out;
        this.#signal = signal;
        this.#message = emptyAnswer(model);
    }

    get calls(): string[] {
        return this.#message.content.flatMap((b) =>
            b.type === "toolCall" ? [b.id] : [],
        );
    }

    /** Calls `stop` once if pi-durable stops this call while it runs. */
    onAbort(stop: () => void): void {
        this.#aborted = () => {
            this.fail("The call was stopped.", true);
            stop();
        };
        if (this.#signal?.aborted === true) this.#aborted();
        else
            this.#signal?.addEventListener("abort", this.#aborted, {
                once: true,
            });
    }

    /** Takes one event of the streamed message. */
    event(event: StreamEvent): void {
        switch (event.type) {
            case "message_start":
                // Blocks left open by a stream Claude Code gave up on are never finished.
                for (const { at } of [...this.#open.values()].sort(
                    (a, b) => b.at - a.at,
                ))
                    this.#message.content.splice(at, 1);
                this.#open.clear();
                this.#tokens(event.message.usage);
                return;
            case "content_block_start":
                this.#begin(event.index, event.content_block);
                return;
            case "content_block_delta":
                this.#delta(event.index, event.delta);
                return;
            case "content_block_stop":
                this.#end(event.index);
                return;
            case "message_delta": {
                const reason = event.delta.stop_reason;
                this.#stop =
                    reason === "tool_use"
                        ? "toolUse"
                        : reason === "max_tokens"
                          ? "length"
                          : "stop";
                this.#tokens(event.usage);
                return;
            }
            default:
                return;
        }
    }

    /** Takes a message Claude Code delivered whole, without streaming it. */
    whole(message: SDKAssistantMessage["message"]): void {
        for (const block of message.content) {
            const index = -1 - this.#message.content.length;
            this.#begin(index, block);
            if (block.type === "text")
                this.#delta(index, { type: "text_delta", text: block.text });
            else if (block.type === "thinking") {
                this.#delta(index, {
                    type: "thinking_delta",
                    thinking: block.thinking,
                    estimated_tokens: null,
                });
                this.#delta(index, {
                    type: "signature_delta",
                    signature: block.signature,
                });
            } else if (block.type === "tool_use")
                this.#delta(index, {
                    type: "input_json_delta",
                    partial_json: JSON.stringify(block.input ?? {}),
                });
            this.#end(index);
        }
        this.#tokens(message.usage);
    }

    finish(stop?: "toolUse"): void {
        if (this.#ended) return;
        this.#start();
        const reason = stop ?? (this.#stop === "length" ? "length" : "stop");
        this.#message.stopReason = reason;
        this.#close();
        this.#out.push({ type: "done", reason, message: this.#message });
        this.#out.end(this.#message);
    }

    fail(words: string, aborted = false): void {
        if (this.#ended) return;
        const reason = aborted ? "aborted" : "error";
        this.#message.stopReason = reason;
        this.#message.errorMessage = words;
        this.#close();
        this.#out.push({ type: "error", reason, error: this.#message });
        this.#out.end(this.#message);
    }

    #close(): void {
        this.#ended = true;
        if (this.#aborted !== undefined)
            this.#signal?.removeEventListener("abort", this.#aborted);
    }

    #start(): void {
        if (this.#started) return;
        this.#started = true;
        this.#out.push({ type: "start", partial: this.#message });
    }

    #begin(index: number, block: Block): void {
        if (this.#ended) return;
        const content = this.#message.content;
        if (block.type === "text") content.push({ type: "text", text: "" });
        else if (block.type === "thinking")
            content.push({
                type: "thinking",
                thinking: "",
                thinkingSignature: "",
            });
        else if (
            block.type === "tool_use" &&
            block.name.startsWith(TOOL_PREFIX)
        )
            content.push({
                type: "toolCall",
                id: block.id,
                name: block.name.slice(TOOL_PREFIX.length),
                arguments: {},
            });
        // Claude Code refuses a call of any other tool itself, and Claude tries again.
        else return;
        this.#start();
        const at = content.length - 1;
        this.#open.set(index, { at, json: "" });
        const type = KINDS[content[at]?.type ?? "text"];
        this.#out.push({
            type: `${type}_start`,
            contentIndex: at,
            partial: this.#message,
        });
    }

    #delta(index: number, delta: Delta): void {
        const open = this.#open.get(index);
        const block = open && this.#message.content[open.at];
        if (open === undefined || block === undefined) return;
        const partial = this.#message;
        const contentIndex = open.at;
        if (block.type === "text" && delta.type === "text_delta") {
            block.text += delta.text;
            this.#out.push({
                type: "text_delta",
                contentIndex,
                delta: delta.text,
                partial,
            });
        } else if (
            block.type === "thinking" &&
            delta.type === "signature_delta"
        )
            block.thinkingSignature =
                (block.thinkingSignature ?? "") + delta.signature;
        else if (block.type === "thinking" && delta.type === "thinking_delta") {
            block.thinking += delta.thinking;
            this.#out.push({
                type: "thinking_delta",
                contentIndex,
                delta: delta.thinking,
                partial,
            });
        } else if (
            block.type === "toolCall" &&
            delta.type === "input_json_delta"
        ) {
            open.json += delta.partial_json;
            this.#out.push({
                type: "toolcall_delta",
                contentIndex,
                delta: delta.partial_json,
                partial,
            });
        }
    }

    #end(index: number): void {
        const open = this.#open.get(index);
        const block = open && this.#message.content[open.at];
        if (open === undefined || block === undefined) return;
        this.#open.delete(index);
        const partial = this.#message;
        const contentIndex = open.at;
        if (block.type === "text")
            this.#out.push({
                type: "text_end",
                contentIndex,
                content: block.text,
                partial,
            });
        else if (block.type === "thinking")
            this.#out.push({
                type: "thinking_end",
                contentIndex,
                content: block.thinking,
                partial,
            });
        else {
            block.arguments = parsed(open.json);
            this.#out.push({
                type: "toolcall_end",
                contentIndex,
                toolCall: block,
                partial,
            });
        }
    }

    #tokens(tokens: Tokens | undefined): void {
        if (tokens === undefined) return;
        const usage = this.#message.usage;
        usage.input = tokens.input_tokens ?? usage.input;
        usage.output = tokens.output_tokens ?? usage.output;
        usage.cacheRead = tokens.cache_read_input_tokens ?? usage.cacheRead;
        usage.cacheWrite =
            tokens.cache_creation_input_tokens ?? usage.cacheWrite;
        usage.totalTokens =
            usage.input + usage.output + usage.cacheRead + usage.cacheWrite;
        usage.cost = calculateCost(this.#model, usage);
    }
}

/** An answer of `model` with nothing in it yet. */
function emptyAnswer(model: Model<Api>): AssistantMessage {
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

/** The events' name of each kind of content. */
const KINDS = {
    text: "text",
    thinking: "thinking",
    toolCall: "toolcall",
} as const;

function parsed(json: string): JsonObject {
    try {
        const value: unknown = JSON.parse(json || "{}");
        return typeof value === "object" &&
            value !== null &&
            !Array.isArray(value)
            ? (value as JsonObject)
            : {};
    } catch {
        return {};
    }
}

/** How hard `model` thinks at pi's `reasoning`: off when none is asked for and the model can stop thinking. */
function thinking(
    model: Model<Api>,
    reasoning: SimpleStreamOptions["reasoning"],
): Partial<Options> {
    const levels: Partial<Record<string, string | null>> =
        model.thinkingLevelMap ?? {};
    if (reasoning === undefined)
        return model.reasoning && levels.off !== null
            ? { thinking: { type: "disabled" } }
            : {};
    const mapped = levels[reasoning];
    const effort =
        mapped === undefined
            ? EFFORTS[reasoning]
            : Object.values(EFFORTS).includes(mapped as EffortLevel)
              ? (mapped as EffortLevel)
              : undefined;
    // Newer models omit their thinking's words unless asked for a summary of them.
    return effort === undefined
        ? {}
        : { effort, extraArgs: { "thinking-display": "summarized" } };
}

/** A query's streaming input: its prompt, open until the query ends. */
class Input implements AsyncIterable<SDKUserMessage> {
    readonly #queue: SDKUserMessage[] = [];
    #wake: (() => void) | undefined;
    #ended = false;

    push(content: Content): void {
        this.#queue.push({
            type: "user",
            message: { role: "user", content },
            parent_tool_use_id: null,
        });
        this.#wake?.();
    }

    end(): void {
        this.#ended = true;
        this.#wake?.();
    }

    async *[Symbol.asyncIterator](): AsyncIterator<SDKUserMessage> {
        for (;;) {
            const next = this.#queue.shift();
            if (next !== undefined) {
                yield next;
                continue;
            }
            if (this.#ended) return;
            await new Promise<void>((resolve) => {
                this.#wake = resolve;
            });
            this.#wake = undefined;
        }
    }
}

/** A message of a Claude Code session, as Anthropic's API takes it. */
type SessionMessage =
    | { readonly role: "user"; readonly content: Content }
    | { readonly role: "assistant"; readonly content: Block[] };

/** Claude Code's version a session written for it names, as `cc-session-io` 0.4.1 writes it. */
const SESSION_VERSION = "2.1.83";

/**
 * A new Claude Code session holding `messages`, for a query to resume; its
 * id. Its records are those `cc-session-io` 0.4.1 writes.
 */
function importSession(
    messages: readonly Message[],
    at: Folders,
    model: string,
): string {
    const sessionId = randomUUID();
    const cwd = projectOf(at);
    const start = Date.now();
    let parentUuid: string | null = null;
    const records = anthropicMessages(messages).map((message, i) => {
        const uuid = randomUUID();
        const record = {
            type: message.role,
            uuid,
            parentUuid,
            sessionId,
            timestamp: new Date(start + i).toISOString(),
            isSidechain: false,
            cwd,
            userType: "external",
            version: SESSION_VERSION,
            gitBranch: "HEAD",
            entrypoint: "cli",
            ...(message.role === "user"
                ? { message }
                : {
                      requestId: `req_${randomUUID().replaceAll("-", "")}`,
                      message: {
                          id: `msg_${randomUUID().replaceAll("-", "")}`,
                          type: "message",
                          role: "assistant",
                          model,
                          content: message.content,
                          stop_reason: message.content.some(
                              (block) => block.type === "tool_use",
                          )
                              ? "tool_use"
                              : "end_turn",
                          stop_sequence: null,
                          usage: { input_tokens: 0, output_tokens: 0 },
                      },
                  }),
        };
        parentUuid = uuid;
        return `${JSON.stringify(record)}\n`;
    });
    const file = sessionPath(sessionId, at);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, records.join(""));
    return sessionId;
}

/** Removes the session in `file` and the folder of its own files. */
function deleteSession(file: string): void {
    rmSync(file, { force: true });
    rmSync(file.slice(0, -".jsonl".length), { recursive: true, force: true });
}

/** Where Claude Code keeps the session `id` of a program working in `at.work`. */
function sessionPath(id: string, at: Folders): string {
    const project = projectOf(at);
    let name = project.replace(/[^a-zA-Z0-9]/g, "-");
    if (name.length > 200) {
        let hash = 0;
        for (let i = 0; i < project.length; i++)
            hash = ((hash << 5) - hash + project.charCodeAt(i)) | 0;
        name = `${name.slice(0, 200)}-${Math.abs(hash).toString(36)}`;
    }
    return join(at.config, "projects", name, `${id}.jsonl`);
}

function projectOf(at: Folders): string {
    return realpathSync(at.work).normalize("NFC");
}

/** `messages` as Anthropic messages, each turn's tool results in one user message after its call. */
function anthropicMessages(messages: readonly Message[]): SessionMessage[] {
    const converted: SessionMessage[] = [];
    const ids = new Map<string, string>();
    let results: ToolResultBlock[] | undefined;
    for (const message of messages) {
        if (message.role === "user") {
            converted.push({ role: "user", content: userContent(message) });
        } else if (message.role === "assistant") {
            // A turn stopped before anything was written answered nothing.
            if (message.content.length === 0) continue;
            const blocks = message.content.flatMap((block): Block[] => {
                if (block.type === "text")
                    return block.text === ""
                        ? []
                        : [{ type: "text", text: block.text, citations: null }];
                if (block.type === "thinking")
                    // Anthropic takes back only the signatures it gave Claude Code.
                    return message.provider === PROVIDER &&
                        block.thinkingSignature
                        ? [
                              {
                                  type: "thinking",
                                  thinking: block.thinking,
                                  signature: block.thinkingSignature,
                              },
                          ]
                        : [];
                return [
                    {
                        type: "tool_use",
                        id: toolId(block.id, ids),
                        name: `${TOOL_PREFIX}${block.name}`,
                        input: block.arguments,
                    },
                ];
            });
            converted.push({
                role: "assistant",
                content:
                    blocks.length > 0
                        ? blocks
                        : [
                              {
                                  type: "text",
                                  text: "[incompatible content omitted]",
                                  citations: null,
                              },
                          ],
            });
            results = undefined;
        } else if (message.role === "toolResult") {
            const block: ToolResultBlock = {
                type: "tool_result",
                tool_use_id: toolId(message.toolCallId, ids),
                content: resultContent(message),
                ...(message.isError ? { is_error: true } : {}),
            };
            if (results === undefined) {
                results = [block];
                converted.push({ role: "user", content: results });
            } else results.push(block);
        }
    }
    return converted;
}

type UserBlock = Exclude<Content, string>[number];
type ToolResultBlock = Extract<UserBlock, { type: "tool_result" }>;
type ImageBlock = Extract<UserBlock, { type: "image" }>;
type MediaType = Extract<
    ImageBlock["source"],
    { type: "base64" }
>["media_type"];

/** A user message as Anthropic content. */
function userContent(
    message: Extract<Message, { role: "user" }>,
): string | UserBlock[] {
    if (typeof message.content === "string")
        return message.content || "[empty]";
    const blocks = message.content.flatMap((block): UserBlock[] =>
        block.type === "text"
            ? block.text === ""
                ? []
                : [{ type: "text", text: block.text }]
            : [
                  {
                      type: "image",
                      source: {
                          type: "base64",
                          media_type: block.mimeType as MediaType,
                          data: block.data,
                      },
                  },
              ],
    );
    return blocks.length > 0 ? blocks : "[empty]";
}

/** A result as text, which Anthropic rejects empty. */
function resultContent(message: Extract<Message, { role: "toolResult" }>) {
    const text = message.content
        .flatMap((block) => (block.type === "text" ? [block.text] : []))
        .join("\n");
    if (text.trim() !== "") return text;
    return message.isError
        ? "The tool failed with no output."
        : `(${message.toolName} gave no output)`;
}

/** A call id as Anthropic takes it; another provider's may hold other characters. */
function toolId(id: string, ids: Map<string, string>): string {
    let clean = ids.get(id);
    if (clean === undefined) {
        clean = id.replace(/[^a-zA-Z0-9_-]/g, "_");
        ids.set(id, clean);
    }
    return clean;
}

// One call of the provider `claude`: Claude's tool calls end the call and park its query
// until pi brings their results in the next call; any other call starts a query resumed from pi's history.
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
import type {
    Api,
    AssistantMessage,
    AssistantMessageEventStream,
    JsonObject,
    Message,
    Model,
    SimpleStreamOptions,
    Tool,
    ToolResultMessage,
    TranscriptContext,
} from "@earendil-works/pi-ai";
import { calculateCost } from "@earendil-works/pi-ai/models";
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
import { deleteSession } from "cc-session-io";
import { emptyAnswer, environment, type Folders } from "./claude.ts";
import { importSession, TOOL_PREFIX, userContent } from "./claude-history.ts";

/** How long a query waits at its tool calls before it is stopped; their results then start a new one. */
const IDLE_MS = 5 * 60_000;

/** The prompt of a query whose history ends at tool results its own query no longer waits for. */
const CONTINUE =
    "[Your session was interrupted and resumed. What precedes this ends with the tool result you were waiting for. Continue the task from there.]";

/** The words a turn fails with when Claude bills past the plan, which a subscription login is for. */
const OVERAGE =
    "Claude is billing this beyond your plan, to extra usage, so Hamlet stopped. It goes on once your plan's limit resets.";

/** pi's thinking levels as Claude's efforts, where a model's own map names none. */
const EFFORTS: Record<string, EffortLevel> = {
    minimal: "low",
    low: "low",
    medium: "medium",
    high: "high",
    xhigh: "max",
    max: "max",
};

/** The Claude program and its folders. */
export type Program = { readonly at: Folders; readonly program: string };

/** A query stopped at Claude's tool calls, by the pi session it answers. */
type Parked = {
    readonly run: Run;
    /** Where pi keeps the answer that made the calls. */
    readonly at: number;
    readonly calls: readonly string[];
    readonly timer: NodeJS.Timeout;
};

const parked = new Map<string, Parked>();

/** Answers one call of `model` into `out`. */
export function answer(
    claude: Program,
    model: Model<Api>,
    context: TranscriptContext,
    options: SimpleStreamOptions | undefined,
    out: AssistantMessageEventStream,
): void {
    const messages = context.messages.filter((m) => m.role !== "system");
    const reply = new Reply(model, out, messages.length, options?.signal);
    // A summary pi asks for runs apart, leaving the conversation's query be.
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

/** One query of the Claude program, which may answer several calls of pi. */
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
    /** The Claude sessions this query wrote, removed once it ends. */
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
        const turn = messages.findLastIndex((m) => m.role !== "user") + 1;
        const prompt = messages.slice(turn);
        const history =
            prompt.length === 0 ? messages : messages.slice(0, turn);
        const resume =
            history.length === 0
                ? undefined
                : importSession(history, claude.at, model.id);
        if (resume !== undefined) this.#sessions.add(resume);
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
        const append = getCurrentSystemPrompt(context.messages);
        this.#query = query({
            prompt: this.#input,
            options: {
                pathToClaudeCodeExecutable: claude.program,
                cwd: claude.at.work,
                env: environment(claude.at),
                // Claude Code's own tools, settings, MCP servers and slash commands are off; pi's tools run in pi.
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

    /** The MCP server `hamlet`, offering `tools` with their schemas as pi has them. */
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
            // Claude Code names the tool_use each call is for, which pairs it with pi's result.
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
            const { at } = this.#claude;
            for (const id of this.#sessions)
                deleteSession(id, at.work, at.config);
        }
    }

    #take(message: SDKMessage): void {
        const reply = this.#reply;
        switch (message.type) {
            case "system":
                if (message.subtype === "init")
                    this.#sessions.add(message.session_id);
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

    /** Ends the call at Claude's tool calls, if it made any, and keeps the query for pi's results. */
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
type Tokens = {
    input_tokens?: number | null;
    output_tokens?: number | null;
    cache_read_input_tokens?: number | null;
    cache_creation_input_tokens?: number | null;
};

/** The answer to one call of pi, written as Claude's message events come. */
class Reply {
    /** How many messages the call was given, which is where pi keeps this answer. */
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

    /** Calls `stop` once if pi stops this call while it runs. */
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

/** How hard `model` thinks at pi's `reasoning`: off when pi asks for none and the model can stop thinking. */
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

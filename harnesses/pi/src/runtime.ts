import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import type { Context } from "@earendil-works/chord/context";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Message } from "@earendil-works/pi-ai";
import {
    createModels,
    type MutableModels,
    type Provider,
} from "@earendil-works/pi-ai/models";
import {
    type Conversation,
    type ConversationId,
    type Cursor,
    configure,
    createRegistry,
    defineDoc,
    defineExtension,
    defineTool,
    type EntryRecord,
    Harness,
    hook,
    LiveDoc,
    ProviderDoc,
    type Registry,
    section,
    ToolTask,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { Type } from "typebox";
import type { Channel, ServerReply, ServerRequest } from "./channel.ts";
import { fault } from "./faults.ts";
import { FAUX, readScript, scriptedProvider } from "./faux.ts";
import {
    type Capability,
    type Item,
    LINK_VERSION,
    type ModelRef,
    type ToolSpec,
    type Usage,
} from "./link.ts";
import { allowedStreams, type Gate, modelProvider } from "./models.ts";
import { providerWork } from "./providers.ts";

/** The adapter's name and version, which the server keeps in each binding. */
export const ADAPTER = "hamlet-pi 0.1.0";
/** The pinned harness this adapter drives. */
export const HARNESS = "pi-durable 1.2.0";

const CAPABILITIES: Capability[] = [
    "operations",
    "attempts",
    "admission",
    "cancel",
    "recovery",
    "updates",
    "history",
];

/** How long a held wait sleeps before asking again when no answer is announced. */
const ASK_AGAIN_MS = 2_000;

/** The runtime id the server gave this store when it made it. */
const RuntimeDoc = defineDoc<{ id: string }>({
    kind: "hamlet.runtime",
    version: 1,
    scope: "session",
    initial: () => ({ id: "" }),
});

/** A conversation the server created, by the key it created it under. */
type Bound = {
    conversation: ConversationId;
    agent: string;
    errand: boolean;
    model: ModelRef;
};

const BindingsDoc = defineDoc<{ byKey: Record<string, Bound> }>({
    kind: "hamlet.bindings",
    version: 1,
    scope: "session",
    initial: () => ({ byKey: {} }),
});

/** How a runtime is started: its id, its store, and a faux script in place of real models. */
export type RuntimeOptions = {
    readonly runtime: string;
    readonly store: string;
    /** The server has not seen this runtime's store made yet, so it may be made now. */
    readonly fresh: boolean;
    readonly faux: string | undefined;
};

/** Runs the runtime over `channel` until the server goes, which ends the process. */
export async function run(
    options: RuntimeOptions,
    channel: Channel,
): Promise<void> {
    const context = BACKGROUND_CONTEXT;
    const exists = existsSync(options.store);
    if (!exists && !options.fresh) {
        const reply = await hello(channel, null, "missing");
        refusedExit(reply);
        return;
    }
    mkdirSync(dirname(options.store), { recursive: true });
    const models = createModels();
    const registry = createRegistry();
    const harness = await Harness.open(
        await openNodeSqliteStorage(options.store),
        {
            models,
            registry,
            onReport: (error) => {
                process.stderr.write(`pi: ${String(error)}\n`);
            },
        },
        context,
    );
    let recorded: string | null;
    if (exists) {
        const stored = await harness.snapshot(RuntimeDoc, context);
        recorded = stored === undefined ? null : stored.id;
    } else {
        await harness.commit(async (tx) => {
            (await tx.doc(RuntimeDoc)).id = options.runtime;
        }, context);
        recorded = options.runtime;
    }
    const reply = await hello(channel, recorded, exists ? "opened" : "created");
    if (reply.type !== "welcome") {
        await harness.close(context);
        refusedExit(reply);
        return;
    }
    const runtime = new Runtime(harness, registry, models, channel);
    await runtime.start(reply.tools, options.faux, context);
}

async function hello(
    channel: Channel,
    runtime: string | null,
    store: "created" | "opened" | "missing",
): Promise<ServerReply> {
    return channel.request({
        type: "hello",
        versions: [LINK_VERSION],
        adapter: ADAPTER,
        harness: HARNESS,
        runtime,
        store,
        capabilities: CAPABILITIES,
    });
}

function refusedExit(reply: ServerReply): void {
    const words =
        reply.type === "refused"
            ? reply.words
            : `The server answered the hello with ${reply.type}.`;
    process.stderr.write(`hamlet-pi: ${words}\n`);
    process.exit(1);
}

/** A bound conversation as this process knows it. */
type Known = Bound & { key: string; sessionId: string };

/** The open harness serving the server's requests. */
class Runtime {
    readonly #harness: Harness;
    readonly #registry: Registry;
    readonly #models: MutableModels;
    readonly #channel: Channel;
    /** By the conversation's id as the link spells it. */
    readonly #known = new Map<string, Known>();
    readonly #bySession = new Map<string, Known>();
    /** The last state of each conversation's run, as told to the server. */
    readonly #running = new Map<string, boolean>();
    /** Each conversation's requests, handled in their order. */
    readonly #lines = new Map<string, Promise<void>>();
    /** Held waits by operation key, woken when the server says it was answered. */
    readonly #waits = new Map<string, () => void>();
    /** Cursors of history pages this process handed out. */
    readonly #cursors = new Map<string, Cursor>();
    readonly #gate: Gate;
    #providerFaux: Provider | undefined;

    constructor(
        harness: Harness,
        registry: Registry,
        models: MutableModels,
        channel: Channel,
    ) {
        this.#harness = harness;
        this.#registry = registry;
        this.#models = models;
        this.#channel = channel;
        this.#gate = {
            attempt: async (sessionId, model) => {
                const known =
                    sessionId === undefined
                        ? undefined
                        : this.#bySession.get(sessionId);
                if (known === undefined)
                    return {
                        ok: false,
                        words: "This model call belongs to no conversation the server bound.",
                    };
                const key = randomUUID();
                const live = await harness.snapshot(
                    LiveDoc,
                    known.conversation,
                    BACKGROUND_CONTEXT,
                );
                const input = live?.run?.inputs[0];
                const answer = await channel.request({
                    type: "attempt",
                    conversation: String(known.conversation),
                    key,
                    model,
                    ...(input === undefined ? {} : { task: String(input) }),
                });
                switch (answer.type) {
                    case "allowance":
                        return {
                            ok: true,
                            key,
                            ...("model" in answer
                                ? { model: answer.model }
                                : {}),
                            ...(answer.credential === undefined
                                ? {}
                                : { apiKey: answer.credential.key }),
                        };
                    case "stopped":
                        return { ok: false, words: answer.words };
                    default:
                        return { ok: false, words: unexpected(answer) };
                }
            },
            charge: async (key, usage: Usage) => {
                const answer = await channel.request({
                    type: "usage",
                    key,
                    usage,
                });
                if (answer.type !== "charged")
                    throw new Error(unexpected(answer));
            },
        };
    }

    async start(
        tools: readonly ToolSpec[],
        faux: string | undefined,
        context: Context,
    ): Promise<void> {
        if (faux !== undefined) {
            const script = await readScript(faux);
            this.#providerFaux = scriptedProvider(
                script,
                () => "",
                (streams) => streams,
            );
            this.#models.setProvider(
                scriptedProvider(
                    script,
                    (options) => {
                        const known =
                            options?.sessionId === undefined
                                ? undefined
                                : this.#bySession.get(options.sessionId);
                        if (known === undefined) return "";
                        return known.errand ? "errands" : known.agent;
                    },
                    (streams) => allowedStreams(streams, this.#gate),
                ),
            );
        }
        const bindings = await this.#harness.snapshot(BindingsDoc, context);
        for (const [key, bound] of Object.entries(bindings?.byKey ?? {}))
            await this.#remember(key, bound, context);
        this.#registry.install(this.#extension(tools));
        this.#harness.subscribeCommits(({ changes }) => {
            const requests = new Map<number, string>();
            for (const change of changes)
                if (
                    change.type === "submission" &&
                    change.value.requestId !== undefined &&
                    change.value.entry !== undefined
                )
                    requests.set(change.value.entry, change.value.requestId);
            for (const change of changes) {
                if (change.type === "entry") {
                    const conversation = String(change.value.conversationId);
                    if (!this.#known.has(conversation)) continue;
                    this.#channel.send({
                        type: "kept",
                        conversation,
                        item: itemOf(
                            change.value,
                            requests.get(change.value.id),
                        ),
                    });
                } else if (
                    change.type === "document" &&
                    change.record.kind === LiveDoc.definition.kind &&
                    change.conversationId !== undefined
                ) {
                    const conversation = String(change.conversationId);
                    if (!this.#known.has(conversation)) continue;
                    const running =
                        change.value !== null && change.value.run !== undefined;
                    if (this.#running.get(conversation) === running) continue;
                    this.#running.set(conversation, running);
                    this.#channel.send({
                        type: "running",
                        conversation,
                        running,
                    });
                }
            }
        });
        this.#channel.serve(
            (request) => this.#serve(request),
            // No agent runs without its server (S21); what was cut off carries on in the next runtime.
            () => process.exit(0),
        );
        this.#harness.resume();
    }

    async #remember(
        key: string,
        bound: Bound,
        context: Context,
    ): Promise<Known> {
        const provider = await this.#harness.snapshot(
            ProviderDoc,
            bound.conversation,
            context,
        );
        if (provider === undefined)
            throw new Error(
                `Conversation ${bound.conversation} has no provider session`,
            );
        const known = { ...bound, key, sessionId: provider.sessionId };
        this.#known.set(String(bound.conversation), known);
        this.#bySession.set(provider.sessionId, known);
        if (
            bound.model.provider !== FAUX.provider &&
            this.#models.getProvider(bound.model.provider) === undefined
        )
            this.#models.setProvider(modelProvider(bound.model, this.#gate));
        return known;
    }

    #extension(tools: readonly ToolSpec[]) {
        const channel = this.#channel;
        const names = new Set(tools.map((tool) => tool.name));
        return defineExtension({
            name: "hamlet",
            tools: tools.map((spec) =>
                defineTool({
                    name: spec.name,
                    description: spec.description,
                    parameters: Type.Unsafe<Record<string, unknown>>(
                        spec.parameters,
                    ),
                    replay: spec.replay,
                    execute: async (args, api, context) => {
                        const key = String(api.taskId);
                        await fault("before_operation");
                        for (;;) {
                            const answer = await channel.request({
                                type: "operation",
                                conversation: String(api.conversationId),
                                key,
                                tool: spec.name,
                                arguments: args,
                            });
                            switch (answer.type) {
                                case "result":
                                    return {
                                        output: [
                                            { type: "text", text: answer.text },
                                        ],
                                        isError: answer.error,
                                    };
                                case "refusal":
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: answer.words,
                                            },
                                        ],
                                        isError: true,
                                    };
                                case "waiting":
                                    await this.#held(key, context);
                                    break;
                                default:
                                    throw new Error(unexpected(answer));
                            }
                        }
                    },
                }),
            ),
            sections: [
                section(
                    "hamlet",
                    async (input) => {
                        const answer = await channel.request({
                            type: "prompt",
                            conversation: String(input.conversationId),
                        });
                        if (answer.type !== "sections")
                            throw new Error(unexpected(answer));
                        return answer.sections
                            .map((part) => part.text)
                            .join("\n\n");
                    },
                    { tag: false },
                ),
            ],
            hooks: [
                hook(ToolTask, {
                    // An early ask, so a wait shows before the call is kept as begun; the server decides again as the
                    // call runs, since a recovered call skips this hook.
                    beforeTool: async (call, api, context) => {
                        if (!names.has(call.name)) return undefined;
                        const key = String(api.taskId);
                        for (;;) {
                            const answer = await channel.request({
                                type: "allow",
                                conversation: String(api.conversationId),
                                key,
                                tool: call.name,
                                arguments: call.arguments,
                            });
                            switch (answer.type) {
                                case "allowed":
                                    return undefined;
                                case "refusal":
                                    return { block: answer.words };
                                case "waiting":
                                    await this.#held(key, context);
                                    break;
                                default:
                                    return { block: unexpected(answer) };
                            }
                        }
                    },
                    afterTool: async () => {
                        await fault("after_receive");
                        return undefined;
                    },
                }),
            ],
        });
    }

    /** Waits until the server says the wait `key` was answered, or a while has passed. */
    async #held(key: string, context: Context): Promise<void> {
        const woken = new Promise<void>((resolve) => {
            this.#waits.set(key, resolve);
        });
        await Promise.race([
            woken,
            sleep(
                ASK_AGAIN_MS,
                undefined,
                context.abortSignal === undefined
                    ? {}
                    : { signal: context.abortSignal },
            ).catch(() => {}),
        ]);
        this.#waits.delete(key);
    }

    /** Handles `request` after the earlier requests of its conversation. */
    #serve(request: ServerRequest): void {
        const line =
            request.type === "provider_request"
                ? request.id
                : request.type === "create"
                  ? `create:${request.key}`
                  : "conversation" in request
                    ? request.conversation
                    : "";
        const previous = this.#lines.get(line) ?? Promise.resolve();
        const next = previous.then(() =>
            this.#answer(request).catch((error: unknown) => {
                this.#channel.send({
                    type: "error",
                    re: request.id,
                    code: "failed",
                    message:
                        error instanceof Error ? error.message : String(error),
                });
            }),
        );
        this.#lines.set(line, next);
        void next.finally(() => {
            if (this.#lines.get(line) === next) this.#lines.delete(line);
        });
    }

    async #answer(request: ServerRequest): Promise<void> {
        const context = BACKGROUND_CONTEXT;
        const re = request.id;
        switch (request.type) {
            case "provider_request": {
                const result = await providerWork(request, this.#providerFaux);
                this.#channel.send({ type: "provider_result", re, ...result });
                return;
            }
            case "create": {
                const known = await this.#create(request, context);
                this.#channel.send({
                    type: "created",
                    re,
                    conversation: String(known.conversation),
                });
                return;
            }
            case "submit": {
                const conversation = await this.#conversation(
                    request.conversation,
                    re,
                    context,
                );
                if (conversation === undefined) return;
                await fault("before_admit");
                const submission = await conversation.submit(
                    {
                        type: "input",
                        content: request.content,
                        requestId: request.request,
                        whenBusy: request.steer ? "steer" : "followUp",
                    },
                    context,
                );
                const record = await submission.status(context);
                await fault("after_admit");
                this.#channel.send({
                    type: "admitted",
                    re,
                    submission: String(submission.id),
                    status: record.status,
                });
                return;
            }
            case "cancel": {
                const conversation = await this.#conversation(
                    request.conversation,
                    re,
                    context,
                );
                if (conversation === undefined) return;
                await conversation.abort(context);
                this.#channel.send({ type: "cancelled", re });
                return;
            }
            case "history": {
                const conversation = await this.#conversation(
                    request.conversation,
                    re,
                    context,
                );
                if (conversation === undefined) return;
                const cursor =
                    request.before === undefined
                        ? undefined
                        : this.#cursors.get(request.before);
                if (request.before !== undefined && cursor === undefined) {
                    this.#channel.send({
                        type: "error",
                        re,
                        code: "malformed",
                        message: `No page starts before ${request.before} in this runtime.`,
                    });
                    return;
                }
                const page = await conversation.entries(
                    { order: "descending" },
                    request.limit,
                    cursor,
                    context,
                );
                const items = page.items.map((entry) => itemOf(entry));
                const first = items.at(-1);
                if (page.next !== undefined && first !== undefined)
                    this.#cursors.set(first.entry, page.next);
                this.#channel.send({
                    type: "page",
                    re,
                    items: items.reverse(),
                    earlier: page.next !== undefined,
                });
                return;
            }
            case "status": {
                const conversations = [];
                for (const [conversation, known] of this.#known) {
                    const live = await this.#harness.snapshot(
                        LiveDoc,
                        known.conversation,
                        context,
                    );
                    conversations.push({
                        conversation,
                        running: live?.run !== undefined,
                    });
                }
                this.#channel.send({ type: "statuses", re, conversations });
                return;
            }
            case "answered": {
                this.#waits.get(request.key)?.();
                return;
            }
            default: {
                const unhandled: never = request;
                throw new Error(`Unhandled request ${String(unhandled)}`);
            }
        }
    }

    async #create(
        request: Extract<ServerRequest, { type: "create" }>,
        context: Context,
    ): Promise<Known> {
        const { key } = request;
        const already = [...this.#known.values()].find(
            (known) => known.key === key,
        );
        if (already !== undefined) return already;
        if (
            request.model.provider === FAUX.provider &&
            this.#models.getProvider(FAUX.provider) === undefined
        )
            throw new Error("This runtime was started without a faux script.");
        const bound = await this.#harness.commit(async (tx) => {
            const bindings = await tx.doc(BindingsDoc);
            const kept = bindings.byKey[key];
            if (kept !== undefined) return { ...kept };
            const record = await tx.createConversation({
                ownership: { kind: "ownerless" },
            });
            await configure(tx, record.id, {
                model: {
                    provider: request.model.provider,
                    modelId: request.model.id,
                },
            });
            // Made now, so each model call can be told apart by its session from the first.
            await tx.doc(ProviderDoc, record.id);
            const made: Bound = {
                conversation: record.id,
                agent: request.agent,
                errand: request.errand,
                model: request.model,
            };
            bindings.byKey[key] = made;
            return made;
        }, context);
        return this.#remember(key, bound, context);
    }

    async #conversation(
        id: string,
        re: string,
        context: Context,
    ): Promise<Conversation | undefined> {
        const known = this.#known.get(id);
        const conversation =
            known === undefined
                ? undefined
                : await this.#harness.conversation(known.conversation, context);
        if (conversation === undefined)
            this.#channel.send({
                type: "error",
                re,
                code: "unknown_conversation",
                message: `No conversation ${id} in this runtime.`,
            });
        return conversation;
    }
}

function unexpected(answer: ServerReply): string {
    return answer.type === "error"
        ? `The server could not answer: ${answer.message}`
        : `The server answered with ${answer.type}.`;
}

/** A kept entry as the link shows it. */
function itemOf(entry: EntryRecord, request?: string): Item {
    const id = String(entry.id);
    const messages: readonly Message[] = entry.model ?? [];
    const [message] = messages;
    if (message === undefined || messages.length > 1)
        return { entry: id, kind: "other", text: "" };
    switch (message.role) {
        case "user":
            return {
                entry: id,
                kind: "user",
                ...(request === undefined ? {} : { request }),
                text:
                    typeof message.content === "string"
                        ? message.content
                        : message.content
                              .map((part) =>
                                  part.type === "text" ? part.text : "",
                              )
                              .join(""),
            };
        case "assistant": {
            const text = message.content
                .map((part) => (part.type === "text" ? part.text : ""))
                .join("");
            const thinking = message.content
                .map((part) => (part.type === "thinking" ? part.thinking : ""))
                .join("");
            const calls = message.content.flatMap((part) =>
                part.type === "toolCall"
                    ? [
                          {
                              id: part.id,
                              name: part.name,
                              arguments: { ...part.arguments },
                          },
                      ]
                    : [],
            );
            const { usage } = message;
            return {
                entry: id,
                kind: "assistant",
                text,
                ...(thinking === "" ? {} : { thinking }),
                ...(calls.length === 0 ? {} : { calls }),
                stop: message.stopReason,
                ...(message.errorMessage === undefined ? {} : { error: true }),
                usage: {
                    input: usage.input,
                    output: usage.output,
                    cacheRead: usage.cacheRead,
                    cacheWrite: usage.cacheWrite,
                    cost: usage.cost.total,
                },
            };
        }
        case "toolResult":
            return {
                entry: id,
                kind: "tool_result",
                text: message.content
                    .map((part) => (part.type === "text" ? part.text : ""))
                    .join(""),
                call: message.toolCallId,
                error: message.isError,
            };
        case "system":
            return { entry: id, kind: "other", text: "" };
        default: {
            const unhandled: never = message;
            throw new Error(`Unhandled message ${String(unhandled)}`);
        }
    }
}

import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import type { Context } from "@earendil-works/chord/context";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { AssistantMessage, Message } from "@earendil-works/pi-ai";
import {
    createModels,
    type MutableModels,
    type Provider,
} from "@earendil-works/pi-ai/models";
import {
    type Conversation,
    type ConversationId,
    configure,
    createRegistry,
    defineDoc,
    defineExtension,
    defineTool,
    type EntryId,
    type EntryRecord,
    GenerationTask,
    Harness,
    type HarnessSettings,
    hook,
    InboxDoc,
    type InboxState,
    LiveDoc,
    type LiveState,
    ProviderDoc,
    type Registry,
    section,
    type TaskId,
    ToolTask,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { Type } from "typebox";
import type {
    Channel,
    Outgoing,
    ServerReply,
    ServerRequest,
} from "./channel.ts";
import { linkCredentials } from "./credentials.ts";
import { fault } from "./faults.ts";
import { FAUX, readScript, scriptedProvider } from "./faux.ts";
import {
    type Block,
    type Capability,
    type Item,
    LINK_VERSION,
    type Mark,
    type ModelRef,
    type Partial,
    type ToolSpec,
    type Usage,
} from "./link.ts";
import { allowedStreams, type Gate, modelProvider } from "./models.ts";
import { providers } from "./provider-set.ts";
import { providerWork } from "./providers.ts";
import { RecoveryDoc, recoveryTask } from "./recovery.ts";
import { signIn } from "./sign-in.ts";

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

/** How every conversation runs: tools one at a time, the server's retry waits, and no retry inside a stream. */
const SETTINGS = {
    toolExecution: "sequential",
    retry: { maxRetries: 3, baseDelayMs: 1000, maxAgentDelayMs: 30_000 },
    stream: { maxRetries: 0 },
    compaction: {
        reserveTokens: Math.round(32768 * 0.2),
        backgroundTokens: Math.round(32768 * 0.1),
    },
} satisfies HarnessSettings;

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
    available: Provider[] = providers(),
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
            settings: SETTINGS,
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
    const runtime = new Runtime(harness, registry, models, channel, available);
    await runtime.start(
        reply.tools,
        options.faux,
        reply.windows ?? {},
        context,
    );
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

/** What a conversation was last told to be doing. */
type Doing = {
    live?: LiveState | undefined;
    inbox?: InboxState | undefined;
    told?: string;
};

/** The open harness serving the server's requests. */
class Runtime {
    readonly #harness: Harness;
    readonly #registry: Registry;
    readonly #models: MutableModels;
    readonly #channel: Channel;
    /** By the conversation's id as the link spells it. */
    readonly #known = new Map<string, Known>();
    readonly #bySession = new Map<string, Known>();
    /** What each conversation does, as last told to the server. */
    readonly #doing = new Map<string, Doing>();
    /** The generation task of each conversation's model call in flight, which a stopped call ends. */
    readonly #generations = new Map<string, TaskId>();
    /** Each conversation's requests, handled in their order. */
    readonly #lines = new Map<string, Promise<void>>();
    /** Held waits by operation key, woken when the server says it was answered. */
    readonly #waits = new Map<string, () => void>();
    readonly #gate: Gate;
    readonly #recovery: ReturnType<typeof recoveryTask>;
    readonly #signins = new Map<string, ReturnType<typeof signIn>>();
    readonly #credentialJobs = new Set<Promise<unknown>>();
    readonly #providers: Provider[];
    #providerFaux: Provider | undefined;

    constructor(
        harness: Harness,
        registry: Registry,
        models: MutableModels,
        channel: Channel,
        available: Provider[],
    ) {
        this.#harness = harness;
        this.#registry = registry;
        this.#models = models;
        this.#channel = channel;
        this.#providers = available;
        this.#recovery = recoveryTask(harness, channel);
        this.#gate = {
            credentials: (scope, provider) =>
                linkCredentials(channel, scope, provider, this.#credentialJobs),
            finished: async (scope) => {
                await channel.request({
                    type: "credential_request",
                    scope,
                    action: "end",
                });
            },
            selected: (model) => {
                SETTINGS.compaction = {
                    reserveTokens: Math.round(model.contextWindow * 0.2),
                    backgroundTokens: Math.round(model.contextWindow * 0.1),
                };
            },
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
                const input = await this.#input(known.conversation);
                const answer = await channel.request({
                    type: "attempt",
                    conversation: String(known.conversation),
                    key,
                    model,
                    ...(input === undefined ? {} : { task: input }),
                });
                switch (answer.type) {
                    case "allowance":
                        return {
                            ok: true,
                            key,
                            ...("model" in answer
                                ? { model: answer.model }
                                : {}),
                            ...(answer.credential?.type !== "api_key" ||
                            answer.credential.key === undefined
                                ? {}
                                : { apiKey: answer.credential.key }),
                        };
                    case "stopped": {
                        const live = await harness.snapshot(
                            LiveDoc,
                            known.conversation,
                            BACKGROUND_CONTEXT,
                        );
                        if (live?.compactions?.some((item) => item.blocking)) {
                            await harness.commit(async (tx) => {
                                (
                                    await tx.doc(
                                        RecoveryDoc,
                                        known.conversation,
                                    )
                                ).stopped = true;
                            }, BACKGROUND_CONTEXT);
                            return { ok: false, words: answer.words };
                        }
                        return {
                            ok: false,
                            words: answer.words,
                            ...(answer.notice === undefined
                                ? {}
                                : {
                                      stop: (signal) =>
                                          this.#stopCall(
                                              known,
                                              answer.notice as Mark,
                                              signal,
                                          ),
                                  }),
                        };
                    }
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
            scrub: async <T>(value: T): Promise<T> => {
                const answer = await channel.request({ type: "scrub", value });
                if (answer.type !== "scrubbed")
                    throw new Error("The server could not scrub model output.");
                return answer.value as T;
            },
        };
    }

    async start(
        tools: readonly ToolSpec[],
        faux: string | undefined,
        windows: Record<string, number>,
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
        for (const [key, bound] of Object.entries(bindings?.byKey ?? {})) {
            const known = await this.#remember(key, bound, context);
            this.#window(known, windows[key]);
        }
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
            const changed = new Set<string>();
            // Told after what the conversation does, so the server reads a settled input against its live state.
            const settled: Outgoing[] = [];
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
                } else if (change.type === "submission") {
                    const { value } = change;
                    const conversation = String(value.conversationId);
                    if (
                        !this.#known.has(conversation) ||
                        value.requestId === undefined ||
                        (value.status !== "done" &&
                            value.status !== "unanswered")
                    )
                        continue;
                    settled.push({
                        type: "settled",
                        conversation,
                        request: value.requestId,
                        status: value.status,
                        ...(value.status === "unanswered"
                            ? {
                                  reason: value.reason,
                                  ...(value.detail === undefined
                                      ? {}
                                      : { detail: value.detail }),
                              }
                            : {}),
                    });
                } else if (
                    change.type === "document" &&
                    (change.record.kind === LiveDoc.definition.kind ||
                        change.record.kind === InboxDoc.definition.kind) &&
                    change.conversationId !== undefined
                ) {
                    const conversation = String(change.conversationId);
                    if (!this.#known.has(conversation)) continue;
                    const doing = this.#doing.get(conversation) ?? {};
                    if (change.record.kind === LiveDoc.definition.kind)
                        doing.live = (change.value ?? undefined) as
                            | LiveState
                            | undefined;
                    else
                        doing.inbox = (change.value ?? undefined) as
                            | InboxState
                            | undefined;
                    this.#doing.set(conversation, doing);
                    changed.add(conversation);
                }
            }
            for (const conversation of changed) this.#tell(conversation);
            for (const message of settled) this.#channel.send(message);
        });
        this.#channel.serve(
            (request) => this.#serve(request),
            // No agent runs without its server (S21); what was cut off carries on in the next runtime.
            () => process.exit(0),
        );
        this.#harness.resume();
    }

    /** Tells the server what `conversation` does, when that changed. */
    #tell(conversation: string): void {
        const doing = this.#doing.get(conversation);
        if (doing === undefined) return;
        const message = doing.live?.generation?.message as
            | AssistantMessage
            | undefined;
        const partial: Partial | undefined =
            message === undefined
                ? undefined
                : { at: message.timestamp, blocks: blocksOf(message) };
        const live = {
            type: "live" as const,
            conversation,
            running: doing.live?.run !== undefined,
            pending:
                doing.inbox?.items.filter((item) => item.mode !== "write")
                    .length ?? 0,
            ...(partial === undefined ? {} : { partial }),
        };
        const told = JSON.stringify(live);
        if (doing.told === told) return;
        doing.told = told;
        this.#channel.send(live);
    }

    /** The runtime's submission id of the input whose turn `conversation` runs now. */
    async #input(conversation: ConversationId): Promise<string | undefined> {
        const live = await this.#harness.snapshot(
            LiveDoc,
            conversation,
            BACKGROUND_CONTEXT,
        );
        const input = live?.run?.inputs[0];
        return input === undefined ? undefined : String(input);
    }

    /**
     * Ends the turn of a model call the server stopped: keeps `notice`, ends the call's generation as stopped, and
     * resolves once the call's signal says so.
     */
    async #stopCall(
        known: Known,
        notice: Mark,
        signal: AbortSignal | undefined,
    ): Promise<void> {
        const conversation = String(known.conversation);
        await this.#harness.commit(async (tx) => {
            await tx.appendEntry(known.conversation, {
                kind: notice.kind,
                data: notice.data as never,
            });
        }, BACKGROUND_CONTEXT);
        const task = this.#generations.get(conversation);
        if (task !== undefined)
            void this.#harness
                .abortTask(task, BACKGROUND_CONTEXT)
                .catch(() => {});
        if (signal === undefined || signal.aborted) return;
        await new Promise<void>((resolve) =>
            signal.addEventListener("abort", () => resolve(), { once: true }),
        );
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
            this.#models.setProvider(
                modelProvider(bound.model, this.#gate, this.#providers),
            );
        return known;
    }

    #window(known: Known, window: number | undefined): void {
        if (window === undefined) return;
        const model = this.#models.getModel(
            known.model.provider,
            known.model.id,
        );
        if (model === undefined) return;
        model.contextWindow = window;
        this.#gate.selected?.(model);
    }

    #extension(tools: readonly ToolSpec[]) {
        const channel = this.#channel;
        const names = new Set(tools.map((tool) => tool.name));
        return defineExtension({
            name: "hamlet",
            tasks: [this.#recovery],
            tools: tools.map((spec) =>
                defineTool({
                    name: spec.name,
                    description: spec.description,
                    parameters: Type.Unsafe<Record<string, unknown>>(
                        spec.parameters,
                    ),
                    replay: spec.replay,
                    // The server bounds a command's output to its end; pi keeps it as given.
                    ...(spec.name === "bash"
                        ? { outputLimits: { retain: "tail" as const } }
                        : {}),
                    execute: async (args, api, context) => {
                        const key = String(api.taskId);
                        await fault("before_operation");
                        const input = await this.#input(api.conversationId);
                        for (;;) {
                            const answer = await abortable(
                                channel.request({
                                    type: "operation",
                                    conversation: String(api.conversationId),
                                    key,
                                    ...(input === undefined ? {} : { input }),
                                    tool: spec.name,
                                    arguments: args,
                                }),
                                context,
                            );
                            switch (answer.type) {
                                case "result":
                                    return {
                                        output: [
                                            { type: "text", text: answer.text },
                                        ],
                                        isError: answer.error,
                                        ...(answer.details === undefined
                                            ? {}
                                            : {
                                                  details:
                                                      answer.details as never,
                                              }),
                                        ...(answer.diagnostics === undefined
                                            ? {}
                                            : {
                                                  diagnostics:
                                                      answer.diagnostics,
                                              }),
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
                    async (prompt, context) => {
                        const input = await this.#input(prompt.conversationId);
                        for (;;) {
                            const answer = await abortable(
                                channel.request({
                                    type: "prompt",
                                    conversation: String(prompt.conversationId),
                                    ...(input === undefined ? {} : { input }),
                                }),
                                context,
                            );
                            if (answer.type === "waiting") {
                                await this.#held(
                                    `prompt:${prompt.conversationId}:${input}`,
                                    context,
                                );
                                continue;
                            }
                            if (answer.type !== "sections")
                                throw new Error(unexpected(answer));
                            return answer.sections
                                .map((part) => part.text)
                                .join("\n\n");
                        }
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
                        const input = await this.#input(api.conversationId);
                        for (;;) {
                            const answer = await abortable(
                                channel.request({
                                    type: "allow",
                                    conversation: String(api.conversationId),
                                    key,
                                    ...(input === undefined ? {} : { input }),
                                    tool: call.name,
                                    arguments: call.arguments,
                                }),
                                context,
                            );
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
                hook(GenerationTask, {
                    beforeRequest: async (_request, api, context) => {
                        const live = await this.#harness.snapshot(
                            LiveDoc,
                            api.conversationId,
                            context,
                        );
                        if (live?.run?.taskId !== api.taskId)
                            return untilAborted(context);
                        this.#generations.set(
                            String(api.conversationId),
                            api.taskId,
                        );
                        return undefined;
                    },
                    // The server says whether a final answer stands; one that fails is kept with its words, and its
                    // turn ends as aborted, as Hamlet's own empty-answer check ends it.
                    onYield: async (message, api, context) => {
                        const conversation = String(api.conversationId);
                        if (!this.#known.has(conversation)) return undefined;
                        const input = await this.#input(api.conversationId);
                        const view = await this.#harness.conversation(
                            api.conversationId,
                            context,
                        );
                        const active = await view?.context(context);
                        const recovery = await this.#harness.snapshot(
                            RecoveryDoc,
                            api.conversationId,
                            context,
                        );
                        const answer = await abortable(
                            channel.request({
                                type: "yield",
                                conversation,
                                ...(input === undefined ? {} : { input }),
                                answer: assistantItem("yield", message),
                                history:
                                    (active?.messages.filter(
                                        (m) => m.role !== "system",
                                    ).length ?? 0) > 1,
                                retried:
                                    input !== undefined &&
                                    recovery?.retried[input] === true,
                            }),
                            context,
                        );
                        if (answer.type !== "verdict")
                            throw new Error(unexpected(answer));
                        if (answer.compact === true && input !== undefined) {
                            await this.#harness.commit(async (tx) => {
                                const live = await tx.doc(
                                    LiveDoc,
                                    api.conversationId,
                                );
                                if (live.run?.taskId !== api.taskId) return;
                                const recovery = await tx.doc(
                                    RecoveryDoc,
                                    api.conversationId,
                                );
                                recovery.retried[input] = true;
                                recovery.stopped = false;
                                if (live.generation !== undefined)
                                    live.generation.message = JSON.parse(
                                        JSON.stringify(message),
                                    ) as never;
                                live.run.taskId = await tx.createTask(
                                    this.#recovery,
                                    {
                                        generation: api.taskId,
                                        input,
                                        answer: assistantItem("yield", message),
                                    },
                                    {
                                        ownership: { kind: "conversation" },
                                        conversationId: api.conversationId,
                                    },
                                );
                            }, context);
                            return untilAborted(context);
                        }
                        const { words, notice } = answer;
                        if (words === undefined) return undefined;
                        await this.#harness.commit(async (tx) => {
                            const live = await tx.doc(
                                LiveDoc,
                                api.conversationId,
                            );
                            // Kept whole when the abort converts it, with its metered usage.
                            if (live.generation !== undefined)
                                live.generation.message = JSON.parse(
                                    JSON.stringify({
                                        ...message,
                                        errorMessage: words,
                                    }),
                                ) as never;
                            if (notice !== undefined)
                                await tx.appendEntry(api.conversationId, {
                                    kind: notice.kind,
                                    data: notice.data as never,
                                });
                        }, context);
                        void this.#harness
                            .abortTask(api.taskId, BACKGROUND_CONTEXT)
                            .catch(() => {});
                        return untilAborted(context);
                    },
                }),
            ],
        });
    }

    /** Waits until the server says the wait `key` was answered, or a while has passed. */
    async #held(key: string, context: Context): Promise<void> {
        context.abortSignal?.throwIfAborted();
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
        context.abortSignal?.throwIfAborted();
    }

    /** Handles `request` after the earlier requests of its conversation. */
    #serve(request: ServerRequest): void {
        if (request.type === "provider_control") {
            this.#signins.get(request.scope)?.control(request);
            return;
        }
        if (request.type === "provider_request")
            this.#signins.set(request.id, signIn(this.#channel, request.id));
        const line =
            request.type === "provider_request"
                ? request.id
                : request.type === "create" || request.type === "fork"
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
            case "tools":
                this.#registry.install(this.#extension(request.tools));
                return;
            case "provider_request": {
                const session = this.#signins.get(re);
                if (session === undefined)
                    throw new Error("Missing provider session");
                try {
                    const result = await providerWork(
                        request,
                        {
                            interaction: session.interaction,
                            providers: this.#providers,
                            credentials: linkCredentials(
                                this.#channel,
                                re,
                                request.provider,
                                this.#credentialJobs,
                            ),
                        },
                        this.#providerFaux,
                    );
                    this.#channel.send({
                        type: "provider_result",
                        re,
                        ...result,
                    });
                } finally {
                    session.stop();
                    this.#signins.delete(re);
                }
                return;
            }
            case "provider_control":
                return;
            case "create": {
                const known = await this.#create(request, context);
                this.#channel.send({
                    type: "created",
                    re,
                    conversation: String(known.conversation),
                });
                return;
            }
            case "fork": {
                const known = await this.#fork(request, re, context);
                if (known === undefined) return;
                this.#channel.send({
                    type: "created",
                    re,
                    conversation: String(known.conversation),
                });
                return;
            }
            case "submit":
            case "write": {
                const conversation = await this.#conversation(
                    request.conversation,
                    re,
                    context,
                );
                if (conversation === undefined) return;
                const known = this.#known.get(request.conversation);
                if (request.type === "submit" && known !== undefined)
                    this.#window(known, request.window);
                await fault("before_admit");
                const submission = await conversation.submit(
                    request.type === "submit"
                        ? {
                              type: "input",
                              content: request.content,
                              requestId: request.request,
                              whenBusy: request.steer ? "steer" : "followUp",
                              ...(request.mark === undefined
                                  ? {}
                                  : {
                                        entry: {
                                            kind: request.mark.kind,
                                            data: request.mark.data as never,
                                        },
                                    }),
                          }
                        : {
                              type: "write",
                              requestId: request.request,
                              entry: {
                                  kind: request.mark.kind,
                                  data: request.mark.data as never,
                                  // A write that changes the context runs a boundary, which places queued inputs.
                                  ...(request.boundary === true
                                      ? { edits: [] }
                                      : {}),
                              },
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
                    ...(record.status === "unanswered"
                        ? {
                              reason: record.reason,
                              ...(record.detail === undefined
                                  ? {}
                                  : { detail: record.detail }),
                          }
                        : {}),
                });
                return;
            }
            case "configure": {
                const known = this.#known.get(request.conversation);
                if (known === undefined) {
                    this.#channel.send({
                        type: "error",
                        re,
                        code: "unknown_conversation",
                        message: `No conversation ${request.conversation} in this runtime.`,
                    });
                    return;
                }
                const { tools } = request;
                const registered = this.#registry
                    .snapshot()
                    .tools()
                    .map(({ tool }) => tool);
                const named = (names: readonly string[]) =>
                    registered.filter((tool) => names.includes(tool.name));
                await this.#harness.commit(
                    (tx) =>
                        configure(tx, known.conversation, {
                            tools:
                                "only" in tools
                                    ? named(tools.only)
                                    : { remove: named(tools.remove) },
                        }),
                    context,
                );
                this.#channel.send({ type: "configured", re });
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
                // The crash fixture can withhold the acknowledgement while later requests still arrive.
                void fault("before_cancelled").then(() =>
                    this.#channel.send({ type: "cancelled", re }),
                );
                return;
            }
            case "history": {
                const conversation = await this.#conversation(
                    request.conversation,
                    re,
                    context,
                );
                if (conversation === undefined) return;
                const before =
                    request.before === undefined
                        ? undefined
                        : Number(request.before);
                if (before !== undefined && !Number.isSafeInteger(before)) {
                    this.#channel.send({
                        type: "error",
                        re,
                        code: "malformed",
                        message: `${request.before} is not an entry of this runtime.`,
                    });
                    return;
                }
                // By entry ids, which every runtime of this store reads alike.
                const page = await conversation.entries(
                    {
                        order: "descending",
                        ...(before === undefined
                            ? {}
                            : { maxEntryId: (before - 1) as EntryId }),
                    },
                    request.limit,
                    undefined,
                    context,
                );
                const items = page.items.map((entry) => itemOf(entry));
                this.#channel.send({
                    type: "page",
                    re,
                    items: items.reverse(),
                    earlier: page.next !== undefined,
                });
                return;
            }
            case "close": {
                // Running work is signalled and waited on; what it did not finish carries on in the next runtime.
                for (const session of this.#signins.values()) session.stop();
                await this.#harness.close(context).catch(() => {});
                while (this.#credentialJobs.size)
                    await Promise.allSettled([...this.#credentialJobs]);
                process.exit(0);
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

    /** A copy of a bound conversation up to one of its entries, made once per key; it has a provider session of its own. */
    async #fork(
        request: Extract<ServerRequest, { type: "fork" }>,
        re: string,
        context: Context,
    ): Promise<Known | undefined> {
        const { key } = request;
        const already = [...this.#known.values()].find(
            (known) => known.key === key,
        );
        if (already !== undefined) return already;
        const parent = this.#known.get(request.conversation);
        const at = Number(request.at);
        if (parent === undefined || !Number.isSafeInteger(at)) {
            this.#channel.send({
                type: "error",
                re,
                code: "unknown_conversation",
                message: `No entry ${request.at} of a conversation ${request.conversation} in this runtime.`,
            });
            return undefined;
        }
        const bound = await this.#harness.commit(async (tx) => {
            const bindings = await tx.doc(BindingsDoc);
            const kept = bindings.byKey[key];
            if (kept !== undefined) return { ...kept };
            const record = await tx.forkConversation(
                parent.conversation,
                at as EntryId,
                { ownership: { kind: "ownerless" } },
            );
            await configure(tx, record.id, {
                model: {
                    provider: parent.model.provider,
                    modelId: parent.model.id,
                },
            });
            await tx.doc(ProviderDoc, record.id);
            const made: Bound = {
                conversation: record.id,
                agent: parent.agent,
                errand: parent.errand,
                model: parent.model,
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

/** Rejects with the abort's reason once `context` is aborted. */
function untilAborted(context: Context): Promise<never> {
    return new Promise((_resolve, reject) => {
        const signal = context.abortSignal;
        if (signal === undefined)
            reject(new Error("The work cannot be stopped without its signal."));
        else if (signal.aborted) reject(signal.reason);
        else
            signal.addEventListener("abort", () => reject(signal.reason), {
                once: true,
            });
    });
}

/** `promise`, or the abort of `context` if that comes first. */
function abortable<T>(promise: Promise<T>, context: Context): Promise<T> {
    if (context.abortSignal === undefined) return promise;
    const aborted = untilAborted(context);
    aborted.catch(() => {});
    return Promise.race([promise, aborted]);
}

/** Hamlet's kind and data of an entry, for one that is not pi's own. */
function markOf(entry: EntryRecord): Mark | undefined {
    if (entry.kind.startsWith("pi.")) return undefined;
    const data = entry.data;
    return {
        kind: entry.kind,
        data:
            typeof data === "object" && data !== null && !Array.isArray(data)
                ? { ...data }
                : {},
    };
}

/** The blocks of an answer, in its order. */
function blocksOf(message: AssistantMessage): Block[] {
    return message.content.flatMap((part): Block[] => {
        switch (part.type) {
            case "text":
                return [{ type: "text", text: part.text }];
            case "thinking":
                return [{ type: "thinking", thinking: part.thinking }];
            case "toolCall":
                return [
                    {
                        type: "call",
                        id: part.id,
                        name: part.name,
                        arguments: { ...part.arguments },
                    },
                ];
            default:
                return [];
        }
    });
}

/** An answer as the link shows it, under the entry id `entry`. */
function assistantItem(entry: string, message: AssistantMessage): Item {
    const { usage } = message;
    return {
        entry,
        kind: "assistant",
        at: message.timestamp,
        text: message.content
            .map((part) => (part.type === "text" ? part.text : ""))
            .join(""),
        blocks: blocksOf(message),
        stop: message.stopReason,
        ...(message.errorMessage === undefined
            ? {}
            : { failure: message.errorMessage }),
        usage: {
            input: usage.input,
            output: usage.output,
            cacheRead: usage.cacheRead,
            cacheWrite: usage.cacheWrite,
            cost: usage.cost.total,
        },
    };
}

/** A kept entry as the link shows it. */
function itemOf(entry: EntryRecord, request?: string): Item {
    const id = String(entry.id);
    const mark = markOf(entry);
    const data = mark?.data;
    const marked = {
        ...(request === undefined ? {} : { request }),
        ...(mark === undefined ? {} : { mark }),
    };
    const messages: readonly Message[] = entry.model ?? [];
    const [message] = messages;
    const at =
        typeof data?.at === "number" && Number.isSafeInteger(data.at)
            ? Math.max(0, data.at)
            : (message?.timestamp ?? 0);
    if (message === undefined || messages.length > 1)
        return {
            entry: id,
            kind: entry.kind === "pi.compaction" ? "summary" : "other",
            at,
            ...marked,
            text: "",
        };
    switch (message.role) {
        case "user":
            return {
                entry: id,
                kind: "user",
                at,
                ...marked,
                text:
                    typeof message.content === "string"
                        ? message.content
                        : message.content
                              .map((part) =>
                                  part.type === "text" ? part.text : "",
                              )
                              .join("\n"),
            };
        case "assistant":
            return { ...assistantItem(id, message), ...marked };
        case "toolResult": {
            const details = message.details;
            return {
                entry: id,
                kind: "tool_result",
                at,
                ...marked,
                text: message.content
                    .map((part) => (part.type === "text" ? part.text : ""))
                    .join("\n"),
                call: message.toolCallId,
                tool: message.toolName,
                error: message.isError,
                ...(message.durationMs === undefined
                    ? {}
                    : { ms: Math.max(0, Math.round(message.durationMs)) }),
                ...(typeof details === "object" &&
                details !== null &&
                !Array.isArray(details)
                    ? { details: { ...details } }
                    : {}),
            };
        }
        case "system":
            return {
                entry: id,
                kind: entry.kind === "pi.compaction" ? "summary" : "other",
                at,
                ...marked,
                text: "",
            };
        default: {
            const unhandled: never = message;
            throw new Error(`Unhandled message ${String(unhandled)}`);
        }
    }
}

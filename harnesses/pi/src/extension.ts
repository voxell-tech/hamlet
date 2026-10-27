// The hamlet extension: the server's tools, prompt section and say over each call of a conversation.
import { setTimeout as sleep } from "node:timers/promises";
import type { Context } from "@earendil-works/chord/context";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    type ConversationId,
    defineExtension,
    defineTool,
    GenerationTask,
    type Harness,
    hook,
    LiveDoc,
    section,
    type TaskId,
    ToolTask,
} from "@earendil-works/pi-durable";
import { Type } from "typebox";
import {
    type Channel,
    type Outgoing,
    type ServerReply,
    unexpected,
} from "./channel.ts";
import { fault } from "./faults.ts";
import { assistantItem } from "./items.ts";
import type { ToolSpec, Waiting } from "./protocol.ts";
import { RecoveryDoc, type recoveryTask } from "./recovery.ts";

/** How long a held wait sleeps before asking again when no answer is announced. */
const ASK_AGAIN_MS = 2_000;

/** What the extension needs of the runtime it is installed in. */
export interface Host {
    readonly channel: Channel;
    readonly harness: Harness;
    readonly recovery: ReturnType<typeof recoveryTask>;
    readonly waits: Waits;
    /** The runtime's submission id of the input whose turn `conversation` runs now. */
    input(conversation: ConversationId): Promise<string | undefined>;
    /** Whether the server bound `conversation`. */
    knows(conversation: string): boolean;
    /** Keeps `task` as the model call in flight of `conversation`, which a stopped call ends. */
    generating(conversation: string, task: TaskId): void;
}

/** Held waits by the key the server wakes them with. */
export class Waits {
    readonly #waits = new Map<string, () => void>();

    /** Waits until the server says `key` was answered, or a while has passed. */
    async held(key: string, context: Context): Promise<void> {
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

    /** Wakes the wait `key`, if one is held. */
    answered(key: string): void {
        this.#waits.get(key)?.();
    }
}

/** The extension that runs `tools` on the server and asks it before each step of a conversation. */
export function hamletExtension(host: Host, tools: readonly ToolSpec[]) {
    const names = new Set(tools.map((tool) => tool.name));
    return defineExtension({
        name: "hamlet",
        tasks: [host.recovery],
        tools: tools.map((spec) =>
            defineTool({
                name: spec.name,
                description: spec.description,
                parameters: Type.Unsafe<Record<string, unknown>>(
                    spec.parameters,
                ),
                replay: spec.replay,
                ...(spec.retain === undefined
                    ? {}
                    : { outputLimits: { retain: spec.retain } }),
                execute: async (args, api, context) => {
                    await fault("before_operation");
                    const input = await host.input(api.conversationId);
                    const answer = await askUntilDecided(
                        host,
                        {
                            type: "operation",
                            conversation: String(api.conversationId),
                            key: String(api.taskId),
                            ...(input === undefined ? {} : { input }),
                            tool: spec.name,
                            arguments: args,
                        },
                        context,
                    );
                    switch (answer.type) {
                        case "result":
                            return {
                                output: [{ type: "text", text: answer.text }],
                                isError: answer.error,
                                ...(answer.details === undefined
                                    ? {}
                                    : { details: answer.details as never }),
                                ...(answer.diagnostics === undefined
                                    ? {}
                                    : { diagnostics: answer.diagnostics }),
                            };
                        case "refusal":
                            return {
                                output: [{ type: "text", text: answer.words }],
                                isError: true,
                            };
                        default:
                            throw new Error(unexpected(answer));
                    }
                },
            }),
        ),
        sections: [
            section(
                "hamlet",
                async (prompt, context) => {
                    const input = await host.input(prompt.conversationId);
                    const answer = await askUntilDecided(
                        host,
                        {
                            type: "prompt",
                            conversation: String(prompt.conversationId),
                            ...(input === undefined ? {} : { input }),
                        },
                        context,
                    );
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
                    const input = await host.input(api.conversationId);
                    const answer = await askUntilDecided(
                        host,
                        {
                            type: "allow",
                            conversation: String(api.conversationId),
                            key: String(api.taskId),
                            ...(input === undefined ? {} : { input }),
                            tool: call.name,
                            arguments: call.arguments,
                        },
                        context,
                    );
                    switch (answer.type) {
                        case "allowed":
                            return undefined;
                        case "refusal":
                            return { block: answer.words };
                        default:
                            return { block: unexpected(answer) };
                    }
                },
                afterTool: async () => {
                    await fault("after_receive");
                    return undefined;
                },
            }),
            hook(GenerationTask, {
                beforeRequest: async (_request, api, context) => {
                    const live = await host.harness.snapshot(
                        LiveDoc,
                        api.conversationId,
                        context,
                    );
                    if (live?.run?.taskId !== api.taskId)
                        return untilAborted(context);
                    host.generating(String(api.conversationId), api.taskId);
                    return undefined;
                },
                // The server says whether a final answer stands; one that fails is kept with its words, and its
                // turn ends as aborted, as Hamlet's own empty-answer check ends it.
                onYield: async (message, api, context) => {
                    const conversation = String(api.conversationId);
                    if (!host.knows(conversation)) return undefined;
                    const { harness } = host;
                    const input = await host.input(api.conversationId);
                    const view = await harness.conversation(
                        api.conversationId,
                        context,
                    );
                    const active = await view?.context(context);
                    const recovery = await harness.snapshot(
                        RecoveryDoc,
                        api.conversationId,
                        context,
                    );
                    const answer = await abortable(
                        host.channel.request({
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
                        await harness.commit(async (tx) => {
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
                                host.recovery,
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
                    await harness.commit(async (tx) => {
                        const live = await tx.doc(LiveDoc, api.conversationId);
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
                    void harness
                        .abortTask(api.taskId, BACKGROUND_CONTEXT)
                        .catch(() => {});
                    return untilAborted(context);
                },
            }),
        ],
    });
}

/** Asks `request` until the server decides it, holding each wait until the server wakes its key. */
async function askUntilDecided(
    host: Host,
    request: Outgoing,
    context: Context,
): Promise<Exclude<ServerReply, Waiting>> {
    for (;;) {
        const answer = await abortable(host.channel.request(request), context);
        if (answer.type !== "waiting") return answer;
        await host.waits.held(answer.key, context);
    }
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

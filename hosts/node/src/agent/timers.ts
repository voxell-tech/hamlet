/**
 * pi-durable keeps each timer's sleep, so the agent keeps no timer of its
 * own, and the wrapper keeps one host alarm for the earliest one.
 */
import type { Context } from "@earendil-works/chord/context";
import {
    type ConversationId,
    defineExtension,
    defineTask,
    defineTool,
    type Extension,
    type Harness,
} from "@earendil-works/pi-durable";
import { callOf, draftOf, failed, words } from "./turn.ts";

type WakeInput = { call: string; at: number; note: string };

const KIND = "hamlet.wake";

/**
 * Its second phase admits the input and ends the task in one commit, under
 * the request id `w<task id>@<at>`, so a phase that a kill cut off admits
 * once when it runs again.
 */
export const Wake = defineTask<
    WakeInput,
    { phase: "sleep" } | { phase: "admit" },
    null
>({
    name: KIND,
    version: 1,
    initial: () => ({ phase: "sleep" }),
    phases: {
        sleep: (task, runtime, context) =>
            runtime.sleepUntil(task.input.at, { phase: "admit" }, context),
        admit: async (task, runtime, context) => {
            const conversation = await runtime.conversation(
                runtime.conversationId,
                context,
            );
            await runtime.commit(async (tx) => {
                await conversation?.submit(
                    tx,
                    draftOf({
                        kind: "wake",
                        id: `w${task.id}@${task.input.at}`,
                        content: `Your timer fired: ${task.input.note}`,
                    }),
                );
                return {
                    status: "terminal",
                    outcome: { status: "completed", result: null },
                };
            }, context);
        },
    },
    abort: (_task, runtime, context) =>
        runtime.commit(
            () => ({ status: "terminal", outcome: { status: "aborted" } }),
            context,
        ),
});

/** Ends every timer `conversation` set that has not fired. */
export async function timersEnded(
    harness: Harness,
    conversation: ConversationId,
    context: Context,
): Promise<void> {
    const set = await harness.commit(
        (tx) =>
            tx.scanTasks({ conversationId: conversation, kind: KIND }, 1000),
        context,
    );
    for (const task of set.items) {
        if (task.state.outcome === undefined) {
            await harness.abortTask(task.id, context);
        }
    }
}

export function timers(): Extension {
    return defineExtension({
        name: "hamlet.timers",
        tasks: [Wake],
        tools: [
            defineTool({
                name: "wake",
                description:
                    "Set a timer. When it fires you get a new input holding your note. It does not hold your turn.",
                parameters: {
                    type: "object",
                    required: ["in_seconds", "note"],
                    properties: {
                        in_seconds: {
                            type: "number",
                            description: "How long from now, in seconds.",
                        },
                        note: {
                            type: "string",
                            description: "What to remind yourself of.",
                        },
                    },
                },
                replay: "safe",
                execute: async (
                    args: { in_seconds: number; note: string },
                    api,
                    context,
                ) => {
                    // One commit finds the timer this call made, or makes it. A rerun
                    // finds it, so one call makes one timer. It is among the newest:
                    // only the later calls of its own reply made timers after it.
                    const call = callOf(api);
                    const at = await api.commit(async (tx) => {
                        const set = await tx.scanTasks(
                            {
                                conversationId: api.conversationId,
                                kind: KIND,
                                order: "descending",
                            },
                            1000,
                        );
                        const mine = set.items.find(
                            (task) => (task.input as WakeInput).call === call,
                        );
                        if (mine !== undefined)
                            return (mine.input as WakeInput).at;
                        const at =
                            api.now() + Math.round(args.in_seconds * 1000);
                        if (!dated(at)) return undefined;
                        await tx.createTask(
                            Wake,
                            { call, at, note: args.note },
                            {
                                ownership: { kind: "conversation" },
                                conversationId: api.conversationId,
                                background: true,
                            },
                        );
                        return at;
                    }, context);
                    if (at === undefined) {
                        return failed(
                            "`in_seconds` is out of range: no time that far off has a date. No timer is set.",
                        );
                    }
                    return words(
                        `A timer is set for ${new Date(at).toISOString()}.`,
                    );
                },
            }),
        ],
    });
}

function dated(at: number): boolean {
    return Number.isFinite(new Date(at).getTime());
}

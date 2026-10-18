import {
    type CompactionResult,
    CompactionTask,
    defineDoc,
    defineTask,
    GenerationTask,
    type Harness,
    LiveDoc,
    type TaskId,
} from "@earendil-works/pi-durable";
import type { Channel } from "./channel.ts";
import { fault } from "./faults.ts";
import type { Item } from "./link.ts";

/** Cut-off recovery belongs to the original input, including its one permitted retry (C28). */
export const RecoveryDoc = defineDoc<{
    retried: Record<string, boolean>;
    stopped: boolean;
}>({
    kind: "hamlet.recovery",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ retried: {}, stopped: false }),
});

type Input = { generation: TaskId; input: string; answer: Item };
type Checkpoint =
    | { phase: "stop" }
    | { phase: "summarized"; summary: TaskId<CompactionResult> };

/** A durable run owner that stops the cut-off generation, summarises, and hands its inputs to one successor. */
export function recoveryTask(harness: Harness, channel: Channel) {
    return defineTask<Input, Checkpoint, Record<string, never>>({
        name: "hamlet.recovery",
        version: 1,
        initial: () => ({ phase: "stop" }),
        phases: {
            stop: async (task, runtime, context) => {
                await fault("before_recovery_stop");
                await harness.abortTask(task.input.generation, context);
                await runtime.commit(async (tx) => {
                    const summary = await tx.createTask(
                        CompactionTask,
                        { reason: "manual" },
                        { ownership: { kind: "task", taskId: runtime.taskId } },
                    );
                    const live = await tx.doc(LiveDoc, runtime.conversationId);
                    live.compactions ??= [];
                    live.compactions.push({
                        taskId: summary,
                        reason: "manual",
                        blocking: true,
                        attempt: 1,
                    });
                    return {
                        status: "waiting",
                        checkpoint: { phase: "summarized", summary },
                        on: [summary],
                        policy: "allSettled",
                    };
                }, context);
            },
            summarized: async (task, runtime, context) => {
                const [outcome] = await runtime.outcomes(
                    [task.state.checkpoint.summary],
                    context,
                );
                if (
                    outcome?.status === "completed" &&
                    outcome.result.entryId !== undefined
                ) {
                    await fault("after_summary");
                    await runtime.commit(async (tx) => {
                        await tx.appendEntry(runtime.conversationId, {
                            kind: "hamlet.notice",
                            data: {
                                text: "The conversation grew longer than the model can hold, so the earlier part of it was summarised.",
                                at: runtime.now(),
                            },
                        });
                        const live = await tx.doc(
                            LiveDoc,
                            runtime.conversationId,
                        );
                        if (live.run?.taskId === runtime.taskId) {
                            live.run.taskId = await tx.createTask(
                                GenerationTask,
                                {},
                                {
                                    ownership: { kind: "conversation" },
                                    conversationId: runtime.conversationId,
                                },
                            );
                        }
                        return {
                            status: "terminal",
                            outcome: { status: "completed", result: {} },
                        };
                    }, context);
                    return;
                }
                const stopped = (
                    await runtime.snapshot(
                        RecoveryDoc,
                        runtime.conversationId,
                        context,
                    )
                )?.stopped;
                const failure =
                    !stopped &&
                    (outcome?.status === "failed" ||
                        outcome?.status === "faulted")
                        ? outcome.error.message
                        : "";
                const answer = await channel.request({
                    type: "yield",
                    conversation: String(runtime.conversationId),
                    input: task.input.input,
                    answer: task.input.answer,
                    retried: true,
                    history: true,
                    summaryFailure: failure,
                });
                if (answer.type !== "verdict")
                    throw new Error(
                        "The server did not answer the failed summary.",
                    );
                await runtime.commit(async (tx) => {
                    if (answer.notice !== undefined)
                        await tx.appendEntry(runtime.conversationId, {
                            kind: answer.notice.kind,
                            data: answer.notice.data as never,
                        });
                    const live = await tx.doc(LiveDoc, runtime.conversationId);
                    if (live.run?.taskId === runtime.taskId) {
                        for (const input of live.run.inputs)
                            tx.settleSubmission(input, {
                                status: "unanswered",
                                reason: "model_error",
                            });
                        delete live.run;
                    }
                    return {
                        status: "terminal",
                        outcome: { status: "completed", result: {} },
                    };
                }, context);
            },
        },
        abort: async (task, runtime, context) => {
            await harness.abortTask(task.input.generation, context);
            await runtime.commit(async (tx) => {
                const live = await tx.doc(LiveDoc, runtime.conversationId);
                if (live.run?.taskId === runtime.taskId) {
                    for (const input of live.run.inputs)
                        tx.settleSubmission(input, {
                            status: "unanswered",
                            reason: "aborted",
                        });
                    delete live.run;
                }
                return { status: "terminal", outcome: { status: "aborted" } };
            }, context);
        },
    });
}
